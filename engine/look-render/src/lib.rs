//! Applica un `HarmonicLook` ai pixel di un'immagine, producendo un'anteprima
//! renderizzata visibile nella UI — non solo un preset `.xmp` da aprire altrove.
//! Vedi `docs/ARCHITECTURE.md` §3.2 per la pipeline "di riferimento" (stage GPU
//! via wgpu/WGSL, già scritta e validata in `gpu-pipe`, ma non ancora collegata
//! alla UI: servirebbe gestire un device GPU reale attraverso UniFFI/JNA su
//! entrambe le piattaforme, un lavoro sostanzialmente più grande di questo
//! incremento). Qui la stessa sequenza di stage gira su CPU (`rayon`, nessuna
//! GPU necessaria) — più lenta su immagini a piena risoluzione, ma sufficiente
//! per un'anteprima, ed è l'unica via testabile in questo ambiente di sviluppo
//! (nessuna GPU disponibile qui).
//!
//! **Semplificazioni deliberate rispetto alla pipeline GPU completa**
//! (documentate qui, non nascoste): il bilanciamento del bianco
//! (`white_balance.temp`/`tint`) è applicato come guadagno per canale in
//! spazio lineare (approssimazione da color grading, non colorimetrica) —
//! una resa assoluta corretta richiederebbe un profilo colore della
//! fotocamera (matrice o DCP) che questo motore non ha ancora, ma per
//! trasferire lo STILE caldo/freddo di un look è sufficiente. Sharpening resta
//! pianificato per la Fase 3-4 della roadmap (§8); la riduzione del rumore
//! (luminanza + colore) è implementata qui sotto (`apply_noise_reduction`).
//!
//! **Pipeline interna esclusivamente in `f32` (aggiunto in questo giro,
//! richiesta esplicita dell'utente per un uso editoriale — "non è ammessa la
//! minima imperfezione")**: `render_look_core` (il vero motore, sotto) lavora
//! SEMPRE su un buffer `f32` per canale, dall'ingresso all'uscita, qualunque
//! sia la precisione dell'immagine sorgente — mai un arrotondamento a 8 bit
//! nel mezzo della catena. `render_preview_with_look` (anteprima interattiva,
//! invariata nella firma) resta una conversione u8 -> f32 -> u8 attorno a
//! questo stesso motore: economica, pensata per essere ridisegnata ad ogni
//! tick di uno slider, quindi il suo output finale a 8 bit va comunque bene
//! (è solo per lo schermo). `render_full_resolution_with_look` (nuova) è
//! invece l'unica pensata per il file consegnato all'utente: stesso motore,
//! ma senza MAI quantizzare a 8 bit — restituisce `DynamicImage::ImageRgb32F`,
//! che `rawforge-ffi` codifica come JPEG (per la consegna) e come TIFF a 16
//! bit senza perdita (per il "master") a partire dallo STESSO rendering,
//! calcolato una sola volta.

use color_science::{hsl_to_rgb, linear_rgb_to_lab, lab_to_linear_rgb, linear_to_srgb, rgb_to_hsl, srgb_to_linear};
use core_types::{HarmonicLook, MaskTarget};
use harmonic::compute_saliency_map;
use image::DynamicImage;
use rayon::prelude::*;

const HUE_BANDS: usize = 8;

/// Buffer di lavoro interno: RGBA in virgola mobile a 32 bit, canali 0.0..1.0
/// (sRGB-encoded, NON lineare — stessa convenzione dei valori u8/255 che
/// sostituisce). Il canale alpha non ha un significato fotografico reale (le
/// foto non hanno mai trasparenza) — è mantenuto solo perché tutto il resto
/// del motore, ereditato dalla pipeline u8 preesistente, ragiona a passi di 4
/// canali (`chunks_exact(4)`); per il percorso a piena risoluzione resta
/// sempre 1.0 e viene scartato alla codifica finale.
type RgbaF32 = image::ImageBuffer<image::Rgba<f32>, Vec<f32>>;

/// Converte QUALUNQUE variante di `DynamicImage` (8 bit o già `f32`, con o
/// senza alpha) nel buffer di lavoro interno — punto di ingresso unico sia per
/// l'anteprima interattiva (sempre 8 bit in ingresso) sia per il rendering a
/// piena risoluzione (`ImageRgb32F`, dal demosaic RAW vero in `raw-decode`, o
/// ancora 8 bit per una foto JPEG/PNG già sviluppata — vedi
/// `render_full_resolution_with_look`).
fn to_rgba_f32(image: &DynamicImage) -> RgbaF32 {
    match image {
        DynamicImage::ImageRgba32F(buf) => buf.clone(),
        DynamicImage::ImageRgb32F(buf) => rgb32f_to_rgba_f32(buf),
        other => u8_rgba_to_f32(&other.to_rgba8()),
    }
}

fn u8_rgba_to_f32(rgba8: &image::RgbaImage) -> RgbaF32 {
    let (width, height) = rgba8.dimensions();
    let mut out = vec![0f32; rgba8.as_raw().len()];
    out.par_iter_mut()
        .zip(rgba8.as_raw().par_iter())
        .for_each(|(o, &i)| *o = i as f32 / 255.0);
    image::ImageBuffer::from_raw(width, height, out).expect("stessa dimensione del buffer sorgente")
}

fn rgb32f_to_rgba_f32(buf: &image::ImageBuffer<image::Rgb<f32>, Vec<f32>>) -> RgbaF32 {
    let (width, height) = buf.dimensions();
    let src = buf.as_raw();
    let mut out = vec![0f32; src.len() / 3 * 4];
    out.par_chunks_mut(4).zip(src.par_chunks(3)).for_each(|(o, i)| {
        o[0] = i[0];
        o[1] = i[1];
        o[2] = i[2];
        o[3] = 1.0;
    });
    image::ImageBuffer::from_raw(width, height, out).expect("stessa dimensione del buffer sorgente")
}

/// Quantizza il buffer di lavoro a RGBA 8 bit — usata solo dal percorso
/// dell'anteprima interattiva (`render_preview_with_look`) e per derivare uno
/// snapshot 8 bit ad uso interno della maschera di salienza (vedi
/// `render_look_core`), MAI dal percorso a piena risoluzione.
fn rgba_f32_to_u8(buf: &RgbaF32) -> image::RgbaImage {
    let (width, height) = buf.dimensions();
    let mut out = vec![0u8; buf.as_raw().len()];
    out.par_iter_mut()
        .zip(buf.as_raw().par_iter())
        .for_each(|(o, &i)| *o = (i.clamp(0.0, 1.0) * 255.0).round() as u8);
    image::ImageBuffer::from_raw(width, height, out).expect("stessa dimensione del buffer sorgente")
}

/// Scarta il canale alpha (sempre 1.0, senza significato fotografico) e
/// clampa a 0.0..1.0 — l'UNICA quantizzazione del percorso a piena
/// risoluzione è quella che fa poi `rawforge-ffi` codificando JPEG (8 bit) o
/// TIFF (16 bit): qui restiamo in `f32` fino all'ultimo momento utile.
fn rgba_f32_to_rgb32f(buf: &RgbaF32) -> image::ImageBuffer<image::Rgb<f32>, Vec<f32>> {
    let (width, height) = buf.dimensions();
    let src = buf.as_raw();
    let mut out = vec![0f32; src.len() / 4 * 3];
    out.par_chunks_mut(3).zip(src.par_chunks(4)).for_each(|(o, i)| {
        o[0] = i[0].clamp(0.0, 1.0);
        o[1] = i[1].clamp(0.0, 1.0);
        o[2] = i[2].clamp(0.0, 1.0);
    });
    image::ImageBuffer::from_raw(width, height, out).expect("stessa dimensione del buffer sorgente")
}

/// Soglia di salienza (0..1) sopra la quale un pixel è considerato parte del
/// "Soggetto" da `apply_subject_mask` — sotto, parte dello "Sfondo". Scelta
/// empiricamente sulla mappa di salienza di una foto vera (auto su asfalto,
/// vedi `PROVA_saliency.png`, generata da `compute_subject_saliency_preview`):
/// col prior di centratura attivo il grosso dello sfondo scende ben sotto
/// 0.35, mentre il soggetto centrale resta sopra — non è una soglia
/// "universale" (nessuna soglia fissa può esserlo per un'euristica globale
/// per-immagine come questa, vedi i limiti dichiarati su
/// `harmonic::compute_saliency_map`), ma un valore ragionevole di partenza.
const SALIENCY_MASK_THRESHOLD: f32 = 0.35;
/// Ampiezza (in valore di salienza) della transizione morbida intorno alla
/// soglia: senza sfumatura, un confine netto sul valore di salienza
/// produrrebbe un bordo di maschera visibilmente "a scalino" (lo stesso
/// principio già applicato ai bordi di scena in `apply_noise_reduction`, qui
/// applicato al bordo della maschera stessa invece che a un gradiente di
/// luminanza).
const SALIENCY_MASK_FEATHER: f32 = 0.15;

/// Converte un valore di salienza grezzo (0..1) in un peso di maschera "verso
/// il Soggetto" (0..1), con una transizione lineare morbida invece di un
/// confine netto su `SALIENCY_MASK_THRESHOLD`. Il peso "verso lo Sfondo" è
/// semplicemente `1.0 - `questo (vedi `mask_weight_for_target`).
fn saliency_to_subject_weight(saliency: f32) -> f32 {
    let lo = SALIENCY_MASK_THRESHOLD - SALIENCY_MASK_FEATHER;
    let hi = SALIENCY_MASK_THRESHOLD + SALIENCY_MASK_FEATHER;
    ((saliency - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// Peso finale (0..1) della maschera per un dato target, da moltiplicare per
/// l'intensità della regolazione locale in `apply_subject_mask`.
fn mask_weight_for_target(saliency: f32, target: MaskTarget) -> f32 {
    let subject_weight = saliency_to_subject_weight(saliency);
    match target {
        MaskTarget::Subject => subject_weight,
        MaskTarget::Background => 1.0 - subject_weight,
    }
}

/// Istogramma di luminanza a 256 bin, nel formato atteso da
/// `smartbatch::compute_scene_descriptors` — è il ponte tra un'immagine
/// decodificata (RAW o già sviluppata) e l'analisi di scena del batch adattivo.
pub fn luminance_histogram(image: &DynamicImage) -> [u32; 256] {
    let rgba = image.to_rgba8();
    let mut hist = [0u32; 256];
    for pixel in rgba.pixels() {
        let luma = (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
            .round()
            .clamp(0.0, 255.0) as usize;
        hist[luma] += 1;
    }
    hist
}

/// LUT 1D a 256 voci dai punti di controllo della tone curve (interpolazione
/// lineare a tratti sui punti, già monotoni per costruzione — non la spline
/// di Hermite usata in fase di estrazione, ma sufficiente per il rendering).
fn build_tone_curve_lut(points: &[(u8, u8)]) -> [f32; 256] {
    let mut lut = [0f32; 256];
    if points.len() < 2 {
        for (i, slot) in lut.iter_mut().enumerate() {
            *slot = i as f32 / 255.0;
        }
        return lut;
    }
    let mut sorted: Vec<(f32, f32)> = points.iter().map(|(x, y)| (*x as f32, *y as f32)).collect();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    for (i, slot) in lut.iter_mut().enumerate() {
        let x = i as f32;
        let mut y = sorted.last().unwrap().1;
        if x <= sorted[0].0 {
            y = sorted[0].1;
        } else {
            for w in sorted.windows(2) {
                let (x0, y0) = w[0];
                let (x1, y1) = w[1];
                if x >= x0 && x <= x1 {
                    let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
                    y = y0 + (y1 - y0) * t;
                    break;
                }
            }
        }
        *slot = (y / 255.0).clamp(0.0, 1.0);
    }
    lut
}

fn sample_lut(lut: &[f32; 256], v: f32) -> f32 {
    let scaled = (v.clamp(0.0, 1.0) * 255.0).clamp(0.0, 255.0);
    let idx = scaled as usize;
    let frac = scaled - idx as f32;
    let a = lut[idx.min(255)];
    let b = lut[(idx + 1).min(255)];
    a + (b - a) * frac
}

/// Interpola linearmente e circolarmente (wrap a 360°) tra i due valori di
/// banda più vicini alla tonalità `hue` (0..360) del pixel, invece di
/// applicare in blocco il valore dell'UNICA banda a cui il pixel
/// "appartiene". **Bug corretto in questo giro**: la versione precedente
/// assegnava ogni pixel a una sola delle 8 bande (`floor(hue / 45) % 8`) e ne
/// applicava l'aggiustamento per intero — un confine NETTO ogni 45°. Su
/// un'immagine con tonalità che varia con continuità (fogliame, cielo) questo
/// produceva bordi artificiali visibili ovunque la tonalità attraversasse un
/// confine, anche fra pixel visivamente quasi identici da una parte e
/// dall'altra — il "posterizzato a blocchi" segnalato dall'utente, tanto più
/// evidente quante più le 8 bande hanno valori diversi fra loro (es. dopo
/// "Incolla impostazioni" su una foto con una gamma di verdi ampia). Qui ogni
/// banda ha il suo pieno effetto solo al CENTRO del proprio intervallo di
/// 45°; ai bordi fra due bande l'effetto sfuma linearmente 50/50, e la somma
/// dei pesi resta sempre 1 — nessun salto, nessuna banda "invisibile" nella
/// transizione.
fn interpolate_hsl_band(values: &[i32; 8], hue: f32) -> f32 {
    let band_width = 360.0 / HUE_BANDS as f32; // 45°
    // Coordinata "spostata" di mezza banda: il CENTRO della banda i cade
    // esattamente sull'intero i in questo sistema di coordinate, così
    // `floor` trova sempre il centro-banda immediatamente precedente.
    let shifted = hue.rem_euclid(360.0) / band_width - 0.5;
    let low = shifted.floor();
    let frac = shifted - low;
    let low_idx = (low.rem_euclid(HUE_BANDS as f32)) as usize % HUE_BANDS;
    let high_idx = (low_idx + 1) % HUE_BANDS;
    let low_v = values[low_idx] as f32;
    let high_v = values[high_idx] as f32;
    low_v + (high_v - low_v) * frac
}

/// Sotto quale CROMA ASSOLUTA (0..1, vedi `hue_band_weight`) del pixel
/// l'aggiustamento HSL per banda viene attenuato invece che applicato a
/// piena forza. Vedi il commento esteso su `hue_band_weight` per il bug
/// reale che questa soglia corregge e per il motivo per cui è la croma
/// ASSOLUTA — non la saturazione HSL — la quantità giusta da usare qui.
const HUE_BAND_LOW_CHROMA_RAMP: f32 = 0.05;

/// Peso (0..1) con cui applicare l'aggiustamento hue-selettivo per banda
/// (estratto dalla Sintesi Armonica) a un pixel di croma assoluta `chroma`
/// (0..1 — la `d = max(R,G,B) - min(R,G,B)` di `rgb_to_hsl`, PRIMA di
/// qualunque aggiustamento). **Quarto bug reale, distinto dal precedente
/// "salto ripido fra bande" già corretto**: `interpolate_hsl_band` sceglie
/// l'aggiustamento in base alla TONALITÀ del pixel — ma per un pixel quasi
/// grigio (poco o nulla colorato: cielo uniforme, asfalto in ombra, sotto lo
/// scocco dell'auto) la tonalità è numericamente instabile. Quando R, G e B
/// sono tutti vicini fra loro E vicini a zero, il minimo rumore del sensore
/// o della compressione JPEG (presente in QUALUNQUE foto reale) fa oscillare
/// selvaggiamente quale canale risulti max/min — e quindi la tonalità
/// calcolata può saltare di decine o centinaia di gradi da un pixel al
/// successivo, pur essendo i due pixel visivamente identici (grigio scuro).
///
/// **Un primo tentativo di questo fix pesava in base a `hsl[1]` (la
/// saturazione HSL classica) e non ha funzionato**: misurato sulla foto
/// vera, il glitch nel paraurti scuro restava quasi identico. La causa è
/// nella formula stessa di `rgb_to_hsl`: `s = d / (1 - |2L - 1|)`, che ha un
/// polo esattamente a L=0 e L=1 (nero e bianco puri) — vicino a L=0 il
/// denominatore tende a zero, quindi anche una croma assoluta `d`
/// minuscola (rumore reale, pochi millesimi) produce una saturazione HSL
/// riportata vicina a 1.0, cioè l'OPPOSTO di "poco saturo": pesare su
/// `hsl[1]` lasciava questi pixel a piena forza proprio dove serviva
/// proteggerli di più. La croma assoluta `d` non ha questo polo (è sempre
/// in 0..1, proporzionale alla vera differenza fra i canali) ed è la
/// quantità che la Sintesi Armonica avrebbe dovuto guardare fin da
/// principio per decidere "quanto è colorato davvero questo pixel".
///
/// La correzione applica lo stesso principio di qualunque editor HSL
/// selettivo: un pixel che non ha (quasi) colore non ha nemmeno una
/// tonalità affidabile da cui decidere QUANTO aggiustarlo, quindi va
/// toccato poco o nulla, indipendentemente da cosa dice la tonalità
/// calcolata. Sotto `HUE_BAND_LOW_CHROMA_RAMP` di croma il peso sale con
/// uno smoothstep (0 a croma=0, 1 a croma=`HUE_BAND_LOW_CHROMA_RAMP`)
/// invece di un taglio netto — un gradino produrrebbe comunque un contorno
/// visibile ovunque la croma attraversasse quella soglia, lo stesso tipo di
/// bug già corretto altrove per i confini fra bande. Sopra la soglia il
/// peso resta 1.0: pixel già chiaramente colorati (pelle, cielo azzurro,
/// fogliame) mantengono l'aggiustamento hue-selettivo pieno e immutato.
fn hue_band_weight(chroma: f32) -> f32 {
    let t = (chroma / HUE_BAND_LOW_CHROMA_RAMP).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t) // smoothstep
}

/// Peso (0..1) di quanto un pixel di luminanza `luma` (0..1, spazio sRGB)
/// appartiene alla zona "ombre": pieno sotto 0.0, zero da 0.4 in su.
fn shadow_mask(luma: f32) -> f32 {
    (1.0 - luma * 2.5).clamp(0.0, 1.0)
}

/// Come [`shadow_mask`] ma per la zona "luci": zero sotto 0.6, pieno a 1.0.
fn highlight_mask(luma: f32) -> f32 {
    ((luma - 0.6) * 2.5).clamp(0.0, 1.0)
}

/// **Bug reale scoperto e corretto in questo giro**: `look.whites`/`look.blacks`
/// (gli slider "Bianchi"/"Neri" della UI, distinti da "Luci"/"Ombre") esistevano
/// nel modello dati, attraversavano FFI e l'export `.xmp`, ma non venivano MAI
/// letti da questo renderer — non avevano alcun effetto sull'immagine mostrata.
/// Per l'utente che aveva impostato Neri=-60 in risposta all'avviso "ombre
/// schiacciate" delle slider sicure, questo significava che l'unico strumento a
/// disposizione per correggere l'avviso non faceva nulla, lasciando il problema
/// visibile invariato.
///
/// `blacks_mask`/`whites_mask` seguono lo stesso schema di `shadow_mask`/
/// `highlight_mask` ma con zone più STRETTE, mirate ai soli estremi tonali veri
/// (nero/bianco pieno) invece dell'ampia metà inferiore/superiore del range
/// tonale coperta da ombre/luci — la stessa distinzione concettuale che in
/// Lightroom separa "Ombre"/"Luci" (zone ampie, morbide) da "Neri"/"Bianchi"
/// (solo gli estremi, per fissare il punto di nero/bianco).
fn blacks_mask(luma: f32) -> f32 {
    (1.0 - luma * (1.0 / 0.12)).clamp(0.0, 1.0)
}

/// Come [`blacks_mask`] ma per la zona "bianchi": zero sotto 0.88, pieno a 1.0.
fn whites_mask(luma: f32) -> f32 {
    ((luma - 0.88) * (1.0 / 0.12)).clamp(0.0, 1.0)
}

/// Frazione (0.0..1.0) di pixel "vicini al nero puro" (luma <= 2) e "vicini al
/// bianco puro" (luma >= 253) in `image` — pensato per essere chiamato
/// sull'immagine GIÀ RENDERIZZATA (non sull'originale), così la UI può
/// segnalare in tempo reale quando il valore ATTUALE di uno slider sta
/// bruciando le luci o schiacciando le ombre ("slider sicuri"). Deliberatamente
/// non calcola questo per l'intero range di uno slider (richiederebbe
/// ri-renderizzare l'immagine una volta per ogni valore possibile, troppo
/// costoso per un feedback dal vivo) — solo per il valore corrente.
pub fn clipping_fractions(image: &DynamicImage) -> (f32, f32) {
    let rgba = image.to_rgba8();
    let mut shadow_clipped = 0u64;
    let mut highlight_clipped = 0u64;
    let mut total = 0u64;
    for pixel in rgba.pixels() {
        let luma = 0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32;
        if luma <= 2.0 {
            shadow_clipped += 1;
        }
        if luma >= 253.0 {
            highlight_clipped += 1;
        }
        total += 1;
    }
    if total == 0 {
        return (0.0, 0.0);
    }
    (shadow_clipped as f32 / total as f32, highlight_clipped as f32 / total as f32)
}

/// Guadagno per canale (R, G, B) in spazio lineare per un dato bilanciamento
/// del bianco — vedi la nota nel commento di modulo su questa approssimazione
/// deliberata. Estratto in funzione a parte perché il bilanciamento del
/// bianco a gradiente ne calcola DUE (zona A e zona B) invece di uno solo.
fn compute_wb_gain(wb: &core_types::WhiteBalance) -> [f32; 3] {
    const WB_STRENGTH: f32 = 0.35;
    let temp_shift = ((wb.temp as f32 - 5500.0) / 5000.0).clamp(-1.0, 1.0);
    let tint_shift = (wb.tint as f32 / 100.0).clamp(-1.0, 1.0);
    [
        1.0 + temp_shift * WB_STRENGTH,
        1.0 - tint_shift * (WB_STRENGTH * 0.6),
        1.0 - temp_shift * WB_STRENGTH,
    ]
}

/// Fattore di miscela (0.0 = zona A pura, 1.0 = zona B pura) per il pixel in
/// posizione `(x, y)` di un'immagine `width` x `height`, secondo l'asse
/// (`wb_gradient_vertical`), la posizione del centro della transizione
/// (`wb_gradient_position`, 0..100) e la sua ampiezza (`wb_gradient_spread`,
/// 0..100: 0 = bordo netto, 100 = sfumatura sull'intero fotogramma) del Look.
fn gradient_blend_factor(x: u32, y: u32, width: u32, height: u32, look: &HarmonicLook) -> f32 {
    let axis_len = if look.wb_gradient_vertical { height } else { width };
    let coord = if look.wb_gradient_vertical { y } else { x };
    if axis_len <= 1 {
        return 0.0;
    }
    let normalized = coord as f32 / (axis_len - 1) as f32;
    let position = (look.wb_gradient_position as f32 / 100.0).clamp(0.0, 1.0);
    let spread = ((look.wb_gradient_spread as f32 / 100.0).clamp(0.0, 1.0)).max(0.01);
    let t = (normalized - position) / spread + 0.5;
    t.clamp(0.0, 1.0)
}

/// Sfocatura gaussiana separabile (orizzontale poi verticale) su un buffer a
/// canale singolo in virgola mobile, row-major (`width * height` elementi) —
/// non `image::imageops::blur` (pensato per buffer RGBA a 8 bit): qui serve
/// operare sui canali Lab in float senza un giro perdita-precisione
/// conversione-u8 a ogni passata. Ai bordi dell'immagine il campionamento
/// blocca l'indice al pixel più vicino (clamp), non wrap né nero: un bordo
/// fisico della foto non ha "fuori scena" da mescolare. `sigma <= 0` restituisce
/// una copia identica (nessun bordo speciale da gestire a monte).
fn gaussian_blur_channel(data: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if sigma <= 0.0 || width == 0 || height == 0 {
        return data.to_vec();
    }
    let radius = (sigma * 3.0).ceil().max(1.0) as isize;
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let kernel_sum: f32 = kernel.iter().sum();
    for v in kernel.iter_mut() {
        *v /= kernel_sum;
    }

    let mut horizontal = vec![0f32; data.len()];
    horizontal
        .par_chunks_mut(width)
        .zip(data.par_chunks(width))
        .for_each(|(out_row, in_row)| {
            for x in 0..width {
                let mut acc = 0f32;
                for (k, &kv) in kernel.iter().enumerate() {
                    let dx = k as isize - radius;
                    let sx = (x as isize + dx).clamp(0, width as isize - 1) as usize;
                    acc += in_row[sx] * kv;
                }
                out_row[x] = acc;
            }
        });

    let mut vertical = vec![0f32; data.len()];
    vertical
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, out_row)| {
            for x in 0..width {
                let mut acc = 0f32;
                for (k, &kv) in kernel.iter().enumerate() {
                    let dy = k as isize - radius;
                    let sy = (y as isize + dy).clamp(0, height as isize - 1) as usize;
                    acc += horizontal[sy * width + x] * kv;
                }
                out_row[x] = acc;
            }
        });
    vertical
}

/// Riduzione del rumore (luminanza + colore, ognuna 0..100, docs/ARCHITECTURE.md
/// §8 — pianificata per una fase successiva, implementata qui): lavora in Lab
/// perché è lo spazio in cui luminanza (`L`) e colore (`a`/`b`) sono
/// davvero separati — a differenza di RGB, dove sfocare un canale sfoca
/// SEMPRE anche un po' di luminosità. Applicata come primo stage della
/// pipeline, PRIMA di bilanciamento del bianco/esposizione/contrasto/HSL: il
/// resto della pipeline amplifica differenze locali minuscole (lo stesso
/// principio, misurato più volte su foto vere in questo motore, dietro sia il
/// bug della croma instabile vicino al nero sia quello del salto ripido fra
/// bande HSL) — ridurre il rumore ATTIVAMENTE PRIMA di quell'amplificazione è
/// l'unico momento in cui ha un effetto pulito, farlo dopo lo renderebbe
/// meno efficace e più visibile come sfocatura.
///
/// Entrambi i canali sono sfocati con un raggio gaussiano proporzionale
/// all'intensità (0 = nessuna sfocatura, esattamente il valore originale —
/// stesso principio "zero = invariato" di `apply_texture_bands`), poi
/// miscelati con l'originale in proporzione a `1 - edge_weight`: `edge_weight`
/// (derivato dal gradiente locale di `L`, quindi dai bordi reali della SCENA,
/// non dal colore) protegge i contorni netti dalla sfocatura — altrimenti
/// ridurre il rumore comporterebbe sempre perdita di nitidezza sui bordi, non
/// solo nelle zone piatte dove serve davvero. La riduzione CROMATICA riusa
/// deliberatamente lo stesso `edge_weight` calcolato dalla luminanza (non un
/// proprio bordo calcolato su a/b): è così che si evita che il colore
/// "sbordi" oltre un contorno netto (es. il rosso di un soggetto che tinge lo
/// sfondo vicino), lo stesso principio con cui qualunque riduzione rumore
/// cromatica reale è guidata dai bordi di luminanza, non dai propri.
fn apply_noise_reduction(base: &RgbaF32, look: &HarmonicLook) -> RgbaF32 {
    if look.noise_reduction_luma <= 0 && look.noise_reduction_color <= 0 {
        return base.clone();
    }
    const MAX_LUMA_SIGMA: f32 = 2.5;
    const MAX_COLOR_SIGMA: f32 = 7.0;
    // Soglia (in unità L, 0..100) sopra la quale un gradiente locale è
    // considerato un bordo reale della scena da proteggere per intero — non
    // una misura fotometrica, una scelta empirica: abbastanza bassa da
    // proteggere anche contorni a basso contrasto (pelle, cielo/orizzonte),
    // abbastanza alta da non trattare ogni minima variazione di tono come un
    // "bordo" (altrimenti la sfocatura non avrebbe mai un pixel su cui agire).
    const EDGE_GRADIENT_THRESHOLD: f32 = 6.0;

    let (width, height) = base.dimensions();
    let w = width as usize;
    let h = height as usize;
    let n = w * h;

    let mut l_ch = vec![0f32; n];
    let mut a_ch = vec![0f32; n];
    let mut b_ch = vec![0f32; n];
    let mut alpha_ch = vec![0f32; n];
    l_ch.par_iter_mut()
        .zip(a_ch.par_iter_mut())
        .zip(b_ch.par_iter_mut())
        .zip(alpha_ch.par_iter_mut())
        .zip(base.as_raw().par_chunks(4))
        .for_each(|((((l, a), b), alpha), px)| {
            let lin = [
                srgb_to_linear(px[0].clamp(0.0, 1.0)),
                srgb_to_linear(px[1].clamp(0.0, 1.0)),
                srgb_to_linear(px[2].clamp(0.0, 1.0)),
            ];
            let lab = linear_rgb_to_lab(lin);
            *l = lab[0];
            *a = lab[1];
            *b = lab[2];
            *alpha = px[3];
        });

    let luma_strength = (look.noise_reduction_luma.clamp(0, 100) as f32) / 100.0;
    let color_strength = (look.noise_reduction_color.clamp(0, 100) as f32) / 100.0;
    let blurred_l = gaussian_blur_channel(&l_ch, w, h, luma_strength * MAX_LUMA_SIGMA);
    let blurred_a = gaussian_blur_channel(&a_ch, w, h, color_strength * MAX_COLOR_SIGMA);
    let blurred_b = gaussian_blur_channel(&b_ch, w, h, color_strength * MAX_COLOR_SIGMA);

    let mut out = base.clone();
    out.par_chunks_mut(4)
        .enumerate()
        .for_each(|(i, out_px)| {
            let x = i % w;
            let y = i / w;
            // Gradiente centrale di L (non un vero Sobel, sufficiente per una
            // stima di "bordo sì/no"): campiona i vicini bloccando l'indice
            // al bordo dell'immagine invece di uscire fuori scena.
            let xm = x.saturating_sub(1);
            let xp = (x + 1).min(w - 1);
            let ym = y.saturating_sub(1);
            let yp = (y + 1).min(h - 1);
            let gx = l_ch[y * w + xp] - l_ch[y * w + xm];
            let gy = l_ch[yp * w + x] - l_ch[ym * w + x];
            let gradient = (gx * gx + gy * gy).sqrt();
            let edge_weight = (gradient / EDGE_GRADIENT_THRESHOLD).clamp(0.0, 1.0);
            let flat_weight = 1.0 - edge_weight;

            let l_final = l_ch[i] + (blurred_l[i] - l_ch[i]) * flat_weight;
            let a_final = a_ch[i] + (blurred_a[i] - a_ch[i]) * flat_weight;
            let b_final = b_ch[i] + (blurred_b[i] - b_ch[i]) * flat_weight;

            let lin = lab_to_linear_rgb([l_final, a_final, b_final]);
            out_px[0] = linear_to_srgb(lin[0]).clamp(0.0, 1.0);
            out_px[1] = linear_to_srgb(lin[1]).clamp(0.0, 1.0);
            out_px[2] = linear_to_srgb(lin[2]).clamp(0.0, 1.0);
            out_px[3] = alpha_ch[i];
        });
    out
}

/// Applica i tre controlli di "texture" (fine/media/grossa, -100..100) via
/// separazione di frequenza gaussiana: sfoca `base` a tre raggi crescenti,
/// ricava le bande di dettaglio per differenza tra sfocature successive
/// (quella più sfocata di tutte è il "residuo" a bassa frequenza — colore e
/// tono di base, mai toccato), poi ricompone scalando ogni banda di
/// `1 + amount/100`. Con tutti gli amount a 0 la ricostruzione è esatta
/// (residuo + somma delle differenze = l'immagine originale), quindi
/// un'immagine a tinta unita (senza dettaglio da nessuna banda) resta
/// invariata qualunque sia l'amount — solo il DETTAGLIO locale cambia
/// ampiezza, non la luminosità media, a differenza di "Chiarezza"/contrasto.
fn apply_texture_bands(base: &RgbaF32, look: &HarmonicLook) -> RgbaF32 {
    if look.texture_fine == 0 && look.texture_medium == 0 && look.texture_coarse == 0 {
        return base.clone();
    }
    const SIGMA_FINE: f32 = 1.2;
    const SIGMA_MEDIUM: f32 = 4.0;
    const SIGMA_COARSE: f32 = 10.0;

    // `image::imageops::blur` è generica sul tipo di subpixel (richiede solo
    // `Into<f32> + From<f32>`, soddisfatto da `f32` stessa banalmente) — nessun
    // giro perdita-precisione conversione-u8 qui, a differenza di quando
    // questa funzione operava su `RgbaImage` a 8 bit.
    let blur_fine = image::imageops::blur(base, SIGMA_FINE);
    let blur_medium = image::imageops::blur(base, SIGMA_MEDIUM);
    let blur_coarse = image::imageops::blur(base, SIGMA_COARSE);

    let fine_mul = 1.0 + look.texture_fine as f32 / 100.0;
    let medium_mul = 1.0 + look.texture_medium as f32 / 100.0;
    let coarse_mul = 1.0 + look.texture_coarse as f32 / 100.0;

    let (width, _height) = base.dimensions();
    let row_stride = 4 * width as usize;
    let mut out = base.clone();
    out.par_chunks_mut(row_stride)
        .zip(base.par_chunks(row_stride))
        .zip(blur_fine.par_chunks(row_stride))
        .zip(blur_medium.par_chunks(row_stride))
        .zip(blur_coarse.par_chunks(row_stride))
        .for_each(|((((out_row, base_row), bf_row), bm_row), bc_row)| {
            for i in 0..width as usize {
                let px = i * 4;
                for c in 0..3 {
                    let base_v = base_row[px + c];
                    let bf = bf_row[px + c];
                    let bm = bm_row[px + c];
                    let bc = bc_row[px + c];
                    let f_detail = base_v - bf;
                    let m_detail = bf - bm;
                    let c_detail = bm - bc;
                    let reconstructed = bc + f_detail * fine_mul + m_detail * medium_mul + c_detail * coarse_mul;
                    out_row[px + c] = reconstructed.clamp(0.0, 1.0);
                }
                out_row[px + 3] = base_row[px + 3];
            }
        });
    out
}

/// Applica un `HarmonicLook` ai pixel di `image`, restituendo una nuova
/// immagine della stessa dimensione. Ordine degli stage (docs/ARCHITECTURE.md
/// §3.2): riduzione del rumore (luminanza + colore, in Lab) -> bilanciamento
/// del bianco + esposizione -> highlights/shadows -> tone curve -> contrasto
/// -> HSL per banda + split toning -> vibrance/saturazione globale ->
/// maschera Soggetto/Sfondo (esposizione/contrasto/saturazione locali,
/// `SubjectMask`) -> texture.
/// La riduzione rumore va PRIMA di tutto il resto: è un'operazione spaziale
/// (serve il vicinato del pixel, non un guadagno per-pixel) e i suoi
/// benefici si perdono se applicata dopo che contrasto/HSL hanno già
/// amplificato le differenze locali che genera il rumore stesso. La maschera
/// Soggetto/Sfondo va invece per ULTIMA fra gli stage per-pixel (prima solo
/// della texture, anch'essa spaziale): è pensata come un raffinamento LOCALE
/// sopra il Look già completo, non un sostituto delle regolazioni globali.
pub fn render_preview_with_look(image: &DynamicImage, look: &HarmonicLook) -> DynamicImage {
    let input = to_rgba_f32(image);
    let out = render_look_core(&input, look);
    DynamicImage::ImageRgba8(rgba_f32_to_u8(&out))
}

/// Come [`render_preview_with_look`], ma per il file consegnato all'utente
/// (JPEG di esportazione + master TIFF 16 bit), non per lo schermo: stesso
/// motore (`render_look_core`), stessa qualsiasi sorgente accettata (8 bit o
/// già `f32`), ma **nessuna quantizzazione a 8 bit qui** — restituisce
/// `DynamicImage::ImageRgb32F`. È `rawforge-ffi` a fare l'unica
/// quantizzazione finale, una volta per la JPEG (8 bit) e una volta per il
/// TIFF (16 bit), a partire dallo STESSO rendering `f32` calcolato qui una
/// sola volta (vedi `PhotoEditSession::render_full_resolution_export`).
pub fn render_full_resolution_with_look(image: &DynamicImage, look: &HarmonicLook) -> DynamicImage {
    let input = to_rgba_f32(image);
    let out = render_look_core(&input, look);
    DynamicImage::ImageRgb32F(rgba_f32_to_rgb32f(&out))
}

/// Il vero motore di rendering, condiviso da [`render_preview_with_look`]
/// (anteprima interattiva, ridisegnata ad ogni tick di uno slider) e
/// [`render_full_resolution_with_look`] (file consegnato all'utente):
/// un'unica implementazione dell'algoritmo, in `f32` dall'ingresso
/// all'uscita — le due funzioni pubbliche differiscono SOLO per come
/// convertono l'immagine sorgente in ingresso e il risultato in uscita, mai
/// per la matematica del rendering stesso. Vedi il commento di modulo in
/// testa al file per il perché di questa scelta.
fn render_look_core(rgba: &RgbaF32, look: &HarmonicLook) -> RgbaF32 {
    let rgba = apply_noise_reduction(rgba, look);
    let (width, height) = rgba.dimensions();
    let row_stride = 4 * width as usize;

    // Mappa di salienza calcolata UNA volta sola (non per-pixel dentro il
    // loop parallelo sotto) — stessa euristica di `compute_saliency_map`
    // esposta all'utente da `compute_subject_saliency_preview`, qui applicata
    // alla risoluzione reale di rendering (non ridotta a 512px come
    // nell'anteprima diagnostica: qui serve una maschera da APPLICARE, non
    // solo da mostrare). Calcolata solo se la maschera è attiva: è un costo
    // non trascurabile (scansione completa dell'immagine più un confronto
    // fra tutti i bin occupati) da evitare quando nessuna maschera è in uso.
    //
    // **Semplificazione deliberata**: `compute_saliency_map` (crate
    // `harmonic`) accetta solo un buffer 8 bit — qui le passiamo uno snapshot
    // quantizzato del buffer f32 corrente invece di propagare `f32` anche
    // dentro `harmonic`. È una scelta ragionata, non una svista: il risultato
    // è un PESO di maschera 0..1 (quanto un pixel appartiene al "Soggetto"),
    // non un valore di colore finale — la precisione fotografica che questo
    // intero giro di lavoro persegue riguarda i PIXEL consegnati all'utente,
    // non un peso intermedio derivato da un'euristica di contrasto globale
    // già dichiaratamente approssimativa (vedi i limiti documentati su
    // `compute_saliency_map` stessa). Propagare `f32` anche lì avrebbe un
    // costo di manutenzione reale (un'altra API cross-crate da mantenere in
    // sincronia) per un guadagno di qualità non misurabile su questo output.
    let mask_weights: Option<Vec<f32>> = if look.subject_mask.enabled {
        Some(compute_saliency_map(&rgba_f32_to_u8(&rgba)))
    } else {
        None
    };

    let exposure_mul = 2f32.powf(look.exposure_ev);
    let tone_curve_lut = build_tone_curve_lut(&look.tone_curve);
    let contrast_amount = 1.0 + (look.contrast as f32 / 100.0);
    let shadows_amount = look.shadows as f32 / 100.0;
    let highlights_amount = look.highlights as f32 / 100.0;
    let blacks_amount = look.blacks as f32 / 100.0;
    let whites_amount = look.whites as f32 / 100.0;

    // Bilanciamento del bianco: un guadagno per canale in spazio lineare, non
    // un vero profilo colore camera (matrice o DCP) — quello richiederebbe
    // conoscere la risposta della fotocamera che l'ha scattata, cosa che
    // questo motore non ha. Come approssimazione DICHIARATA, sufficiente a
    // trasferire lo STILE caldo/freddo di un look (non una resa colorimetrica
    // assoluta): `temp` (convenzione Lightroom, valori più alti = più caldo)
    // e `tint` (positivo = magenta) diventano guadagni simmetrici su R/B e G.
    // Se `wb_gradient_enabled` è attivo, il guadagno effettivo di ogni pixel
    // sfuma tra la zona A (`wb_gain_a`) e la zona B (`wb_gain_b`) secondo
    // `gradient_blend_factor` — altrimenti resta sempre la zona A, identico
    // al comportamento pre-esistente a guadagno singolo.
    let wb_gain_a = compute_wb_gain(&look.white_balance);
    let wb_gain_b = compute_wb_gain(&look.white_balance_b);
    // `saturation` resta un moltiplicatore piatto (uniforme su ogni pixel,
    // qualunque sia la sua saturazione di partenza) — è lo slider esplicito
    // "Saturazione" dell'utente, un intento diretto da applicare così com'è.
    let saturation_mul = 1.0 + (look.saturation as f32 / 100.0);
    // `vibrance` invece NON è più un moltiplicatore piatto (era il bug reale
    // scoperto in questo giro, segnalato dall'utente con due foto vere: dopo
    // "Incolla impostazioni" i sedili rossi del campione — già molto saturi —
    // uscivano PIÙ desaturati della foto target originale, l'opposto di
    // quanto ci si aspetterebbe copiando lo stile di una foto che quei rossi
    // li aveva vividi). Misurato sulle foto vere: chroma Lab dei sedili
    // 21.98 (target originale) -> 18.17 (dopo incolla, con il vecchio
    // moltiplicatore piatto) invece di avvicinarsi ai 27.23 del campione.
    // Causa: `vibrance` viene estratto come UNA media sull'intera foto
    // campione, dominata qui dall'ampio asfalto grigio quasi neutro (che
    // fa scendere la chroma media a ~4.5, ben sotto BASELINE_CHROMA=10,
    // producendo vibrance=-55) — ma applicato poi come moltiplicatore PIATTO
    // su OGNI pixel del target, quello stesso -55 colpiva in valore assoluto
    // proprio i pixel già più saturi (i sedili rossi) più forte dei pixel
    // già quasi grigi (che hanno poca saturazione da perdere) — l'opposto di
    // cosa significa "vibrance" in qualunque editor fotografico reale (a
    // differenza di "saturation", la vibrance protegge i colori già vividi e
    // agisce di più su quelli spenti, proprio per evitare che uno sfondo
    // neutro schiacci un soggetto colorato). Il vecchio guardrail (clamp
    // 0.35..2.5 sul moltiplicatore) attutiva l'effetto ma restava piatto:
    // stessa percentuale di riduzione per un pixel quasi grigio e per un
    // pixel rosso vivo, quindi il soggetto saturo perdeva comunque più
    // saturazione assoluta dello sfondo. Corretto sostituendo il
    // moltiplicatore piatto con la formula standard di vibrance non lineare
    // (vedi uso più sotto, vicino a `hsl[1] = ...`): l'effetto per-pixel ora
    // dipende dalla saturazione ATTUALE di quel pixel, protegge quasi del
    // tutto i pixel già molto saturi (moltiplicatore -> 1.0 quando
    // `base_sat` -> 1.0, qualunque sia `vibrance`) e agisce quasi per intero
    // su quelli quasi neutri — dove non è comunque percepibile. Per questo
    // il vecchio clamp guardrail (0.35..2.5) non serve più: la formula è già
    // limitata per costruzione (mai sotto 0 né sopra circa 2 per i range
    // leciti di `vibrance`), niente da ricalibrare con un numero arbitrario.
    let vibrance_amount = (look.vibrance as f32 / 100.0).clamp(-1.0, 1.0);

    let mut out = rgba.clone();
    out.par_chunks_mut(row_stride)
        .zip(rgba.par_chunks(row_stride))
        .enumerate()
        .for_each(|(y, (out_row, in_row))| {
            for (x, (out_px, in_px)) in out_row.chunks_exact_mut(4).zip(in_row.chunks_exact(4)).enumerate() {
                // Bilanciamento del bianco + esposizione: guadagni scalari (uno
                // per canale per il WB, uno unico per l'esposizione) in spazio
                // lineare. Il guadagno WB dipende dalla posizione del pixel
                // solo quando il gradiente è attivo (vedi `gradient_blend_factor`).
                let wb_gain = if look.wb_gradient_enabled {
                    let t = gradient_blend_factor(x as u32, y as u32, width, height, look);
                    [
                        wb_gain_a[0] + (wb_gain_b[0] - wb_gain_a[0]) * t,
                        wb_gain_a[1] + (wb_gain_b[1] - wb_gain_a[1]) * t,
                        wb_gain_a[2] + (wb_gain_b[2] - wb_gain_a[2]) * t,
                    ]
                } else {
                    wb_gain_a
                };
                let mut linear = [
                    srgb_to_linear(in_px[0].clamp(0.0, 1.0)),
                    srgb_to_linear(in_px[1].clamp(0.0, 1.0)),
                    srgb_to_linear(in_px[2].clamp(0.0, 1.0)),
                ];
                for (c, gain) in linear.iter_mut().zip(wb_gain.iter()) {
                    *c = (*c * exposure_mul * gain).clamp(0.0, 1.0);
                }

                let mut srgb = [
                    linear_to_srgb(linear[0]),
                    linear_to_srgb(linear[1]),
                    linear_to_srgb(linear[2]),
                ];

                // Highlights/shadows: lift mascherato per zona tonale (positivo
                // = schiarisce quella zona, come in Lightroom per le ombre;
                // per le luci il segno è invertito, "highlights" negativo =
                // recupero luci bruciate). Bianchi/Neri seguono la STESSA
                // convenzione di segno (positivo = schiarisce quella zona) ma
                // agiscono solo sugli estremi veri (vedi `blacks_mask`/
                // `whites_mask`) — per questo Neri POSITIVO (non negativo) è la
                // risposta corretta a un avviso di "ombre schiacciate": alza il
                // punto di nero invece di scurirlo ulteriormente.
                let luma = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
                let s_mask = shadow_mask(luma);
                let h_mask = highlight_mask(luma);
                let b_mask = blacks_mask(luma);
                let w_mask = whites_mask(luma);
                let lift = shadows_amount * s_mask * 0.25
                    + highlights_amount * h_mask * 0.25
                    + blacks_amount * b_mask * 0.25
                    + whites_amount * w_mask * 0.25;
                for c in srgb.iter_mut() {
                    *c = (*c + lift).clamp(0.0, 1.0);
                }

                // Tone curve e contrasto: applicati alla LUMA, non canale per
                // canale come prima. **Bug reale scoperto e corretto in
                // questo giro**, segnalato dall'utente con due foto vere: il
                // risultato di "Incolla impostazioni" risultava molto più
                // desaturato/spento della foto campione stessa, "inutilizzabile".
                // Isolando ogni stadio della pipeline sulle foto vere
                // dell'utente (script di debug dedicato, non solo ipotesi):
                // applicare la stessa LUT (o lo stesso riscalamento di
                // contrasto) in modo indipendente su R, G e B comprime le
                // DIFFERENZE fra i canali — cioè la chroma/saturazione — come
                // effetto collaterale non voluto, anche quando la curva/il
                // contrasto in sé sono miti: misurato, la tone curve da sola
                // tagliava la chroma Lab media di circa il 40%, il contrasto
                // da solo di un altro ~25%, e in sequenza (più il bias di
                // vibrance/hsl per banda, tutti nella stessa direzione su
                // questa foto) il risultato finale scendeva ben sotto la
                // chroma della foto campione che doveva copiare. La lift
                // ombre/luci qui sopra NON soffriva di questo problema perché
                // è additiva e uguale su tutti e tre i canali (sposta la luma
                // senza toccare le differenze fra i canali): stesso principio
                // applicato ora anche qui. La luma è una combinazione lineare
                // con pesi che sommano a 1 (0.2126+0.7152+0.0722=1.0): uno
                // shift additivo identico su R/G/B sposta la luma esattamente
                // di quello shift, lasciando intatte le differenze fra i
                // canali — cioè hue e chroma — invece di comprimerle. Si
                // ricalcola la luma dopo ogni stadio (non si riusa il valore
                // teorico pre-clamp) per restare corretti anche quando un
                // canale satura a 0 o 255.
                let luma_before_curve = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
                let luma_after_curve = sample_lut(&tone_curve_lut, luma_before_curve).clamp(0.0, 1.0);
                let curve_delta = luma_after_curve - luma_before_curve;
                for c in srgb.iter_mut() {
                    *c = (*c + curve_delta).clamp(0.0, 1.0);
                }

                let luma_before_contrast = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
                let luma_after_contrast = ((luma_before_contrast - 0.5) * contrast_amount + 0.5).clamp(0.0, 1.0);
                let contrast_delta = luma_after_contrast - luma_before_contrast;
                for c in srgb.iter_mut() {
                    *c = (*c + contrast_delta).clamp(0.0, 1.0);
                }

                // HSL per banda + split toning + saturazione/vibrance globale.
                // Interpolazione circolare fra bande adiacenti (vedi
                // `interpolate_hsl_band`): niente più confine netto ogni 45°.
                let mut hsl = rgb_to_hsl(srgb);
                // Peso hue-selettivo: vedi il commento esteso su
                // `hue_band_weight` per il quarto bug reale che corregge
                // (chiazze di rumore cromatico nelle zone scure/neutre dove
                // la tonalità calcolata è instabile). Si ricava la croma
                // ASSOLUTA (`d = max-min` di `rgb_to_hsl`) invertendo
                // algebricamente la formula della saturazione HSL
                // (`s = d / (1 - |2L-1|)`, quindi `d = s * (1 - |2L-1|)`)
                // invece di ricalcolarla da `srgb` daccapo — stesso valore,
                // niente di nuovo da importare. Va usata la croma assoluta e
                // non `hsl[1]` (la saturazione HSL) proprio perché
                // quest'ultima è instabile vicino al nero: vedi il commento
                // su `hue_band_weight` per la spiegazione completa.
                let chroma = hsl[1] * (1.0 - (2.0 * hsl[2] - 1.0).abs());
                let band_weight = hue_band_weight(chroma);
                let hue_adjust = interpolate_hsl_band(&look.hsl.hue, hsl[0]) * band_weight;
                let sat_adjust = interpolate_hsl_band(&look.hsl.sat, hsl[0]) * band_weight;
                let lum_adjust = interpolate_hsl_band(&look.hsl.lum, hsl[0]) * band_weight;
                hsl[0] = (hsl[0] + hue_adjust).rem_euclid(360.0);
                // Bias per banda (hue-selettivo, trasferito dal campione) e
                // saturazione piatta (intento esplicito dell'utente) prima,
                // come già facevano: solo l'ordine con cui entra la vibrance
                // (subito sotto) è cambiato.
                let base_sat = (hsl[1] * (1.0 + sat_adjust / 100.0) * saturation_mul).clamp(0.0, 1.0);
                // Vibrance non lineare: protegge i pixel già saturi (vedi
                // commento esteso sopra, vicino a `vibrance_amount`). A
                // `base_sat` -> 1.0 il moltiplicatore tende a 1.0 qualunque
                // sia `vibrance_amount` (il pixel è già pieno di colore, non
                // c'è altro da togliere né altro spazio per aggiungerne); a
                // `base_sat` -> 0.0 il moltiplicatore tende a
                // `1.0 + vibrance_amount` (pieno effetto, ma su un pixel
                // già quasi grigio dove non è comunque percepibile).
                // `protection` è il QUADRATO di `(1 - base_sat)`, non lineare:
                // misurato sulle foto vere dell'utente, una protezione lineare
                // (provata per prima) lasciava comunque un calo percepibile
                // sui sedili rossi del target — già quasi identici in
                // saturazione a quelli della foto campione (chroma Lab
                // misurata: 35.0 target originale vs 36.7 campione, quasi
                // uguali) — perché a saturazione "solo" medio-alta (non
                // ancora vicinissima a 1.0, es. ~0.65-0.7, tipica di pelle in
                // ombra) il fattore lineare `(1-base_sat)` è ancora abbastanza
                // grande da lasciar passare una riduzione notabile. Il
                // quadrato scende più ripidamente: a base_sat=0.68 la
                // protezione lineare vale 0.32 (riduzione ancora forte), il
                // quadrato vale 0.10 (riduzione quasi trascurabile) — cioè
                // protegge sul serio non solo i pixel "già al 100% saturi" in
                // senso stretto ma l'intera fascia alta di saturazione, dove
                // in pratica ricadono i soggetti colorati intenzionali di una
                // foto (pelle, tessuti, fiori...) a differenza dello sfondo
                // quasi neutro che la vibrance globale della foto campione
                // intende davvero attenuare.
                let protection = (1.0 - base_sat) * (1.0 - base_sat);
                let vibrance_mul = 1.0 + vibrance_amount * protection;
                hsl[1] = (base_sat * vibrance_mul).clamp(0.0, 1.0);
                hsl[2] = (hsl[2] + lum_adjust / 200.0).clamp(0.0, 1.0);

                let shadow_weight = shadow_mask(hsl[2]);
                let highlight_weight = highlight_mask(hsl[2]);
                if shadow_weight > 0.0 && look.split_toning.shadow_sat != 0 {
                    hsl = blend_toning(
                        hsl,
                        look.split_toning.shadow_hue as f32,
                        look.split_toning.shadow_sat as f32,
                        shadow_weight,
                    );
                }
                if highlight_weight > 0.0 && look.split_toning.highlight_sat != 0 {
                    hsl = blend_toning(
                        hsl,
                        look.split_toning.highlight_hue as f32,
                        look.split_toning.highlight_sat as f32,
                        highlight_weight,
                    );
                }

                // Maschera Soggetto/Sfondo: si calcola l'HSL "come se" la
                // regolazione locale si applicasse a piena forza ovunque
                // (`adjusted`), poi si sfuma UNA sola volta fra l'HSL
                // originale e quello regolato con il peso di maschera del
                // pixel — invece di applicare il peso separatamente a ogni
                // singolo passo (esposizione, poi contrasto, poi
                // saturazione), che comporrebbe tre sfumature indipendenti
                // anziché una: più semplice da ragionare e da testare, e la
                // hue non viene mai toccata (si sfumano solo lightness e
                // saturazione HSL), quindi la maschera non può introdurre
                // dominanti di colore innaturali sul bordo della sfumatura.
                if let Some(weights) = mask_weights.as_ref() {
                    let w = mask_weight_for_target(weights[y * width as usize + x], look.subject_mask.target);
                    if w > 0.0 {
                        let mut adjusted = hsl;
                        let mask_exposure_gain = 2f32.powf(look.subject_mask.exposure_ev);
                        adjusted[2] = (adjusted[2] * mask_exposure_gain).clamp(0.0, 1.0);
                        let mask_contrast_amount = 1.0 + (look.subject_mask.contrast as f32 / 100.0);
                        adjusted[2] = ((adjusted[2] - 0.5) * mask_contrast_amount + 0.5).clamp(0.0, 1.0);
                        adjusted[1] =
                            (adjusted[1] * (1.0 + look.subject_mask.saturation as f32 / 100.0)).clamp(0.0, 1.0);
                        hsl[1] += (adjusted[1] - hsl[1]) * w;
                        hsl[2] += (adjusted[2] - hsl[2]) * w;
                    }
                }

                let final_rgb = hsl_to_rgb(hsl);
                out_px[0] = final_rgb[0].clamp(0.0, 1.0);
                out_px[1] = final_rgb[1].clamp(0.0, 1.0);
                out_px[2] = final_rgb[2].clamp(0.0, 1.0);
                out_px[3] = in_px[3];
            }
        });

    // Texture (separazione di frequenza) è un'operazione spaziale, non
    // per-pixel: va applicata come passata separata sull'immagine già
    // color-gradata dal loop qui sopra, non dentro di esso.
    apply_texture_bands(&out, look)
}

/// Sposta l'hue di un pixel verso quello del toning e alza leggermente la
/// saturazione, pesato da `weight` (0 = fuori zona, 1 = pieno effetto) e
/// dall'intensità configurata (0..100 -> 0..1). Interpolazione lineare
/// diretta sull'hue (non circolare): approssimazione accettabile perché il
/// toning cinematografico tipico usa hue vicini tra loro, non agli antipodi.
fn blend_toning(hsl: [f32; 3], tone_hue: f32, tone_sat_amount: f32, weight: f32) -> [f32; 3] {
    let strength = (tone_sat_amount.abs() / 100.0).clamp(0.0, 1.0) * weight;
    let mut out = hsl;
    out[0] = (out[0] + (tone_hue - out[0]) * strength).rem_euclid(360.0);
    out[1] = (out[1] + strength * 0.3).clamp(0.0, 1.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::SubjectMask;
    use image::{GenericImageView, ImageBuffer, Rgba};

    fn solid_image(width: u32, height: u32, rgb: [u8; 3]) -> DynamicImage {
        let img = ImageBuffer::from_fn(width, height, |_, _| Rgba([rgb[0], rgb[1], rgb[2], 255]));
        DynamicImage::ImageRgba8(img)
    }

    fn mean_luma(image: &DynamicImage) -> f32 {
        let hist = luminance_histogram(image);
        let total: u64 = hist.iter().map(|&c| c as u64).sum();
        if total == 0 {
            return 0.0;
        }
        let sum: f64 = hist.iter().enumerate().map(|(bin, &c)| bin as f64 * c as f64).sum();
        (sum / total as f64) as f32
    }

    /// Immagine di sfondo uniforme con una piccola patch centrata di un altro
    /// colore — lo stesso scenario "piccolo soggetto colorato e centrato"
    /// già usato per verificare `compute_saliency_map` in isolamento
    /// (`harmonic::tests`), qui riusato per verificare che la maschera che ne
    /// deriva colpisca davvero la patch (il "soggetto") più dello sfondo.
    fn image_with_centered_patch(
        width: u32,
        height: u32,
        background: [u8; 3],
        patch: [u8; 3],
        patch_size: u32,
    ) -> DynamicImage {
        let (px0, py0) = ((width - patch_size) / 2, (height - patch_size) / 2);
        let img = ImageBuffer::from_fn(width, height, |x, y| {
            if x >= px0 && x < px0 + patch_size && y >= py0 && y < py0 + patch_size {
                Rgba([patch[0], patch[1], patch[2], 255])
            } else {
                Rgba([background[0], background[1], background[2], 255])
            }
        });
        DynamicImage::ImageRgba8(img)
    }

    fn mean_luma_of_rect(image: &image::RgbaImage, x0: u32, y0: u32, w: u32, h: u32) -> f64 {
        let mut sum = 0f64;
        let mut count = 0u64;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let px = image.get_pixel(x, y);
                sum += 0.2126 * px[0] as f64 + 0.7152 * px[1] as f64 + 0.0722 * px[2] as f64;
                count += 1;
            }
        }
        sum / count as f64
    }

    #[test]
    fn subject_mask_disabled_has_no_effect_even_with_a_nonzero_exposure_configured() {
        // Guardrail base: `enabled = false` (il default) deve annullare
        // completamente la maschera, qualunque sia il resto di `SubjectMask`
        // — nessuna "fuga" di esposizione locale se l'utente non ha ancora
        // attivato la sezione.
        let img = solid_image(8, 8, [100, 100, 100]);
        let mut look = HarmonicLook::default();
        look.subject_mask.exposure_ev = 2.0;
        look.subject_mask.contrast = 80;
        look.subject_mask.saturation = -80;
        assert!(!look.subject_mask.enabled);

        let baseline = mean_luma(&render_preview_with_look(&img, &HarmonicLook::default()));
        let with_disabled_mask = mean_luma(&render_preview_with_look(&img, &look));
        assert!(
            (baseline - with_disabled_mask).abs() < 0.01,
            "baseline={baseline} con maschera (disattivata)={with_disabled_mask}"
        );
    }

    #[test]
    fn subject_mask_at_the_exact_image_center_of_a_flat_image_applies_at_full_strength_for_subject_and_not_at_all_for_background() {
        // Anche su un'immagine perfettamente piatta, `compute_saliency_map`
        // NON è uniforme pixel-per-pixel: il prior di centratura pesa ogni
        // pixel in base alla propria posizione (non solo al proprio colore),
        // e la mappa viene rinormalizzata al proprio massimo — quindi SOLO
        // il pixel più vicino al centro geometrico tocca 1.0 esatto, mentre
        // pixel più periferici (angoli inclusi) hanno già una salienza
        // inferiore. Un limite onesto e già noto dell'euristica (vedi il
        // commento esteso su `compute_saliency_map`): il primo tentativo di
        // questo test misurava la media sull'intera immagine assumendo una
        // salienza uniforme, ed è stato smentito da questa stessa
        // rinormalizzazione (misurato: target Background dava comunque un
        // effetto misurabile sulla media, ~133 invece di 100, per via degli
        // angoli). Corretto misurando il pixel centrale ESATTO (8x8 -> centro
        // geometrico (4.0, 4.0), che coincide con l'indice del pixel (4,4)),
        // dove il comportamento è invece deterministico.
        let img = solid_image(8, 8, [100, 100, 100]);
        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();

        let mut look_subject = HarmonicLook::default();
        look_subject.subject_mask =
            SubjectMask { enabled: true, target: MaskTarget::Subject, exposure_ev: 1.0, contrast: 0, saturation: 0 };
        let mut look_background = HarmonicLook::default();
        look_background.subject_mask = SubjectMask {
            enabled: true,
            target: MaskTarget::Background,
            exposure_ev: 1.0,
            contrast: 0,
            saturation: 0,
        };

        let subject_rendered = render_preview_with_look(&img, &look_subject).to_rgba8();
        let background_rendered = render_preview_with_look(&img, &look_background).to_rgba8();

        let center_luma = |image: &image::RgbaImage| -> f64 {
            let px = image.get_pixel(4, 4);
            0.2126 * px[0] as f64 + 0.7152 * px[1] as f64 + 0.0722 * px[2] as f64
        };
        let baseline_center = center_luma(&baseline);
        let subject_center = center_luma(&subject_rendered);
        let background_center = center_luma(&background_rendered);

        assert!(
            subject_center > baseline_center + 5.0,
            "target Subject nel pixel centrale esatto deve schiarire a piena forza: baseline={baseline_center} risultato={subject_center}"
        );
        assert!(
            (background_center - baseline_center).abs() < 1.0,
            "target Background nel pixel centrale esatto non deve avere alcun effetto: baseline={baseline_center} risultato={background_center}"
        );
    }

    #[test]
    fn subject_mask_targeting_subject_brightens_the_salient_patch_more_than_the_background() {
        let img = image_with_centered_patch(32, 32, [80, 80, 80], [200, 40, 40], 8);
        let mut look = HarmonicLook::default();
        look.subject_mask =
            SubjectMask { enabled: true, target: MaskTarget::Subject, exposure_ev: 1.0, contrast: 0, saturation: 0 };

        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        let masked = render_preview_with_look(&img, &look).to_rgba8();

        let patch_delta = mean_luma_of_rect(&masked, 12, 12, 8, 8) - mean_luma_of_rect(&baseline, 12, 12, 8, 8);
        let background_delta = mean_luma_of_rect(&masked, 0, 0, 8, 8) - mean_luma_of_rect(&baseline, 0, 0, 8, 8);

        assert!(
            patch_delta > background_delta * 2.0,
            "il soggetto (patch centrata e colorata) deve schiarire molto più dello sfondo: patch_delta={patch_delta} background_delta={background_delta}"
        );
        assert!(patch_delta > 5.0, "il soggetto deve effettivamente schiarire: patch_delta={patch_delta}");
    }

    #[test]
    fn subject_mask_targeting_background_brightens_the_background_more_than_the_salient_patch() {
        let img = image_with_centered_patch(32, 32, [80, 80, 80], [200, 40, 40], 8);
        let mut look = HarmonicLook::default();
        look.subject_mask = SubjectMask {
            enabled: true,
            target: MaskTarget::Background,
            exposure_ev: 1.0,
            contrast: 0,
            saturation: 0,
        };

        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        let masked = render_preview_with_look(&img, &look).to_rgba8();

        let patch_delta = mean_luma_of_rect(&masked, 12, 12, 8, 8) - mean_luma_of_rect(&baseline, 12, 12, 8, 8);
        let background_delta = mean_luma_of_rect(&masked, 0, 0, 8, 8) - mean_luma_of_rect(&baseline, 0, 0, 8, 8);

        assert!(
            background_delta > patch_delta * 2.0,
            "invertendo il target, deve schiarire di più lo sfondo del soggetto: patch_delta={patch_delta} background_delta={background_delta}"
        );
        assert!(background_delta > 5.0, "lo sfondo deve effettivamente schiarire: background_delta={background_delta}");
    }

    #[test]
    fn subject_mask_saturation_only_desaturates_the_targeted_subject() {
        let img = image_with_centered_patch(32, 32, [80, 80, 80], [200, 40, 40], 8);
        let mut look = HarmonicLook::default();
        look.subject_mask =
            SubjectMask { enabled: true, target: MaskTarget::Subject, exposure_ev: 0.0, contrast: 0, saturation: -80 };

        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        let masked = render_preview_with_look(&img, &look).to_rgba8();

        let patch_px = masked.get_pixel(16, 16);
        let baseline_px = baseline.get_pixel(16, 16);
        let sat_masked = rgb_to_hsl([patch_px[0] as f32 / 255.0, patch_px[1] as f32 / 255.0, patch_px[2] as f32 / 255.0])[1];
        let sat_baseline =
            rgb_to_hsl([baseline_px[0] as f32 / 255.0, baseline_px[1] as f32 / 255.0, baseline_px[2] as f32 / 255.0])[1];

        assert!(
            sat_masked < sat_baseline * 0.9,
            "saturazione -80 sul soggetto deve desaturare visibilmente la patch: baseline={sat_baseline} masked={sat_masked}"
        );
    }

    #[test]
    fn subject_mask_never_changes_hue() {
        // La sfumatura di maschera tocca solo lightness e saturazione HSL
        // (mai hsl[0]): anche con esposizione, contrasto e saturazione della
        // maschera tutti spinti forte, la tonalità del pixel non deve
        // spostarsi (altrimenti la maschera introdurrebbe una dominante di
        // colore innaturale sul soggetto).
        let img = image_with_centered_patch(32, 32, [80, 80, 80], [200, 40, 40], 8);
        let mut look = HarmonicLook::default();
        look.subject_mask =
            SubjectMask { enabled: true, target: MaskTarget::Subject, exposure_ev: 1.0, contrast: 40, saturation: -50 };

        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        let masked = render_preview_with_look(&img, &look).to_rgba8();

        let patch_px = masked.get_pixel(16, 16);
        let baseline_px = baseline.get_pixel(16, 16);
        let hue_masked = rgb_to_hsl([patch_px[0] as f32 / 255.0, patch_px[1] as f32 / 255.0, patch_px[2] as f32 / 255.0])[0];
        let hue_baseline =
            rgb_to_hsl([baseline_px[0] as f32 / 255.0, baseline_px[1] as f32 / 255.0, baseline_px[2] as f32 / 255.0])[0];

        assert!(
            (hue_masked - hue_baseline).abs() < 5.0,
            "la maschera non deve alterare la tonalità: baseline={hue_baseline} masked={hue_masked}"
        );
    }

    #[test]
    fn negative_contrast_and_a_real_tone_curve_do_not_collapse_saturation() {
        // Bug reale scoperto e corretto in questo giro, segnalato dall'utente
        // con due foto vere: applicare tone curve e contrasto CANALE PER
        // CANALE (com'era prima) comprime le differenze fra i canali — cioè
        // la saturazione — come effetto collaterale non voluto, anche con
        // valori non estremi. Misurato sulle foto vere: solo la tone curve
        // tagliava la chroma media di ~40%, il contrasto di un altro ~25%.
        // Qui lo stesso principio isolato su un singolo pixel sintetico: un
        // arancione moderatamente saturo, contrasto negativo (-40, la stessa
        // direzione tipica di un campione "morbido") e una tone curve reale
        // (non identità: schiarisce le ombre, scurisce leggermente le luci).
        // La saturazione HSL del risultato deve restare vicina all'originale
        // — non collassare verso il grigio — perché ora la curva/il
        // contrasto si applicano solo alla luma (additivo, uguale su tutti i
        // canali) e non più canale per canale.
        let img = solid_image(4, 4, [180, 100, 60]);
        let original_sat = rgb_to_hsl([180.0 / 255.0, 100.0 / 255.0, 60.0 / 255.0])[1];

        let mut look = HarmonicLook::default();
        look.contrast = -40;
        look.tone_curve = vec![(0, 40), (64, 90), (128, 128), (192, 170), (255, 220)];

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let px = rendered.get_pixel(0, 0);
        let rendered_sat = rgb_to_hsl([px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0])[1];

        assert!(
            rendered_sat > original_sat * 0.8,
            "la saturazione non deve crollare: originale={original_sat} dopo tone curve+contrasto={rendered_sat}"
        );
    }

    #[test]
    fn neutral_look_leaves_image_essentially_unchanged() {
        let img = solid_image(8, 8, [120, 90, 60]);
        let look = HarmonicLook::default();
        let rendered = render_preview_with_look(&img, &look);
        let before = mean_luma(&img);
        let after = mean_luma(&rendered);
        assert!((before - after).abs() < 2.0, "before={before} after={after}");
    }

    #[test]
    fn positive_exposure_brightens_the_image() {
        let img = solid_image(8, 8, [100, 100, 100]);
        let mut look = HarmonicLook::default();
        look.exposure_ev = 1.0;
        let rendered = render_preview_with_look(&img, &look);
        assert!(mean_luma(&rendered) > mean_luma(&img));
    }

    #[test]
    fn negative_exposure_darkens_the_image() {
        let img = solid_image(8, 8, [150, 150, 150]);
        let mut look = HarmonicLook::default();
        look.exposure_ev = -1.0;
        let rendered = render_preview_with_look(&img, &look);
        assert!(mean_luma(&rendered) < mean_luma(&img));
    }

    #[test]
    fn rendered_image_has_same_dimensions_as_input() {
        let img = solid_image(16, 12, [10, 200, 50]);
        let look = HarmonicLook::default();
        let rendered = render_preview_with_look(&img, &look);
        assert_eq!(rendered.dimensions(), img.dimensions());
    }

    #[test]
    fn shadow_recovery_brightens_dark_pixels_more_than_bright_ones() {
        let dark = solid_image(4, 4, [10, 10, 10]);
        let bright = solid_image(4, 4, [230, 230, 230]);
        let mut look = HarmonicLook::default();
        look.shadows = 100;

        let dark_delta = mean_luma(&render_preview_with_look(&dark, &look)) - mean_luma(&dark);
        let bright_delta = mean_luma(&render_preview_with_look(&bright, &look)) - mean_luma(&bright);
        assert!(dark_delta > bright_delta, "dark_delta={dark_delta} bright_delta={bright_delta}");
        assert!(dark_delta > 0.0);
    }

    #[test]
    fn positive_blacks_lifts_near_black_pixels_more_than_midtones() {
        // Bug reale corretto in questo giro: `look.blacks` non veniva mai
        // letto dal renderer, quindi questo slider ("Neri") non aveva alcun
        // effetto — regressione diretta contro quel difetto.
        let near_black = solid_image(4, 4, [5, 5, 5]);
        let midtone = solid_image(4, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.blacks = 100;

        let black_delta = mean_luma(&render_preview_with_look(&near_black, &look)) - mean_luma(&near_black);
        let mid_delta = mean_luma(&render_preview_with_look(&midtone, &look)) - mean_luma(&midtone);
        assert!(black_delta > mid_delta, "black_delta={black_delta} mid_delta={mid_delta}");
        assert!(black_delta > 0.0);
    }

    #[test]
    fn negative_whites_pulls_near_white_pixels_down_more_than_midtones() {
        // Bug reale corretto in questo giro: `look.whites` ("Bianchi") era
        // anch'esso completamente inerte prima di questa modifica.
        let near_white = solid_image(4, 4, [250, 250, 250]);
        let midtone = solid_image(4, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.whites = -100;

        let white_delta = mean_luma(&render_preview_with_look(&near_white, &look)) - mean_luma(&near_white);
        let mid_delta = mean_luma(&render_preview_with_look(&midtone, &look)) - mean_luma(&midtone);
        assert!(white_delta < mid_delta, "white_delta={white_delta} mid_delta={mid_delta}");
        assert!(white_delta < 0.0);
    }

    #[test]
    fn identity_tone_curve_lut_is_approximately_linear() {
        let look = HarmonicLook::default(); // punti (0,0)-(64,64)-(128,128)-(192,192)-(255,255)
        let lut = build_tone_curve_lut(&look.tone_curve);
        for i in (0..256).step_by(17) {
            let expected = i as f32 / 255.0;
            assert!((lut[i] - expected).abs() < 0.01, "i={i} lut={} expected={expected}", lut[i]);
        }
    }

    #[test]
    fn warm_white_balance_raises_red_relative_to_blue() {
        let img = solid_image(4, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.white_balance.temp = 9000; // molto più caldo del neutro (5500)
        let rendered = render_preview_with_look(&img, &look);
        let px = rendered.to_rgba8().get_pixel(0, 0).0;
        assert!(px[0] > px[2], "un WB caldo deve alzare il rosso rispetto al blu: R={} B={}", px[0], px[2]);
    }

    #[test]
    fn cool_white_balance_raises_blue_relative_to_red() {
        let img = solid_image(4, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.white_balance.temp = 2500; // molto più freddo del neutro (5500)
        let rendered = render_preview_with_look(&img, &look);
        let px = rendered.to_rgba8().get_pixel(0, 0).0;
        assert!(px[2] > px[0], "un WB freddo deve alzare il blu rispetto al rosso: R={} B={}", px[0], px[2]);
    }

    #[test]
    fn magenta_tint_lowers_green_relative_to_neutral() {
        let img = solid_image(4, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.white_balance.tint = 100; // massima tinta magenta
        let rendered = render_preview_with_look(&img, &look);
        let px = rendered.to_rgba8().get_pixel(0, 0).0;
        assert!(px[1] < px[0], "una tinta magenta deve abbassare il verde rispetto al rosso/blu: G={} R={}", px[1], px[0]);
    }

    #[test]
    fn luminance_histogram_counts_every_pixel_exactly_once() {
        let img = solid_image(10, 7, [42, 200, 5]);
        let hist = luminance_histogram(&img);
        let total: u64 = hist.iter().map(|&c| c as u64).sum();
        assert_eq!(total, 70);
    }

    #[test]
    fn clipping_fractions_detects_pure_black_and_white_images() {
        let black = solid_image(4, 4, [0, 0, 0]);
        let (shadow, highlight) = clipping_fractions(&black);
        assert!((shadow - 1.0).abs() < 0.001, "shadow={shadow}");
        assert!(highlight < 0.001, "highlight={highlight}");

        let white = solid_image(4, 4, [255, 255, 255]);
        let (shadow2, highlight2) = clipping_fractions(&white);
        assert!(shadow2 < 0.001, "shadow2={shadow2}");
        assert!((highlight2 - 1.0).abs() < 0.001, "highlight2={highlight2}");
    }

    #[test]
    fn clipping_fractions_reports_zero_for_a_midtone_image() {
        let mid = solid_image(4, 4, [128, 128, 128]);
        let (shadow, highlight) = clipping_fractions(&mid);
        assert_eq!(shadow, 0.0);
        assert_eq!(highlight, 0.0);
    }

    /// Immagine di test per la texture: un solo pixel chiaro isolato in uno
    /// sfondo scuro uniforme — la separazione di frequenza ha un dettaglio
    /// concreto su cui agire solo se c'è un bordo, non su una tinta piatta.
    fn single_bright_point(size: u32) -> DynamicImage {
        let center = size / 2;
        let img = ImageBuffer::from_fn(size, size, |x, y| {
            if x == center && y == center {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([50, 50, 50, 255])
            }
        });
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn zero_texture_amounts_leave_a_solid_color_image_unchanged() {
        // Su una tinta piatta ogni banda di dettaglio è zero ovunque, quindi la
        // ricostruzione deve restare identica indipendentemente dagli amount:
        // qui verifichiamo il caso di default (tutti a zero).
        let img = solid_image(10, 10, [90, 140, 30]);
        let look = HarmonicLook::default();
        let rendered = render_preview_with_look(&img, &look);
        let before = mean_luma(&img);
        let after = mean_luma(&rendered);
        assert!((before - after).abs() < 2.0, "before={before} after={after}");
    }

    #[test]
    fn fully_negative_texture_smooths_an_isolated_bright_point_toward_its_surroundings() {
        let img = single_bright_point(16);
        let mut look = HarmonicLook::default();
        look.texture_fine = -100;
        look.texture_medium = -100;
        look.texture_coarse = -100;

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();

        let center = 8u32;
        let after = rendered.get_pixel(center, center)[0];
        let before = baseline.get_pixel(center, center)[0];
        assert!(
            after < before,
            "texture negativa al massimo deve smussare il punto isolato verso lo sfondo: before={before} after={after}"
        );
    }

    #[test]
    fn texture_pass_preserves_image_dimensions() {
        let img = single_bright_point(20);
        let mut look = HarmonicLook::default();
        look.texture_fine = 60;
        look.texture_coarse = -40;
        let rendered = render_preview_with_look(&img, &look);
        assert_eq!(rendered.dimensions(), img.dimensions());
    }

    #[test]
    fn gradient_white_balance_differs_between_left_and_right_zones_when_enabled() {
        let img = solid_image(30, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.wb_gradient_enabled = true;
        look.wb_gradient_vertical = false;
        look.wb_gradient_position = 50;
        look.wb_gradient_spread = 10;
        look.white_balance.temp = 9000; // zona A (sinistra): molto calda
        look.white_balance_b.temp = 2500; // zona B (destra): molto fredda

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let left = rendered.get_pixel(0, 0).0;
        let right = rendered.get_pixel(29, 0).0;
        assert!(left[0] > left[2], "zona sinistra deve restare calda: R={} B={}", left[0], left[2]);
        assert!(right[2] > right[0], "zona destra deve essere fredda: R={} B={}", right[2], right[0]);
    }

    #[test]
    fn hsl_band_interpolation_returns_exact_value_at_band_center() {
        // Banda "Verde" (indice 3, ordine Red/Orange/Yellow/Green/Aqua/Blue/
        // Purple/Magenta): il suo centro è a (3+0.5)*45 = 157.5°.
        let values = [0, 0, 0, 100, 0, 0, 0, 0];
        let at_center = interpolate_hsl_band(&values, 157.5);
        assert!((at_center - 100.0).abs() < 0.01, "at_center={at_center}");
    }

    #[test]
    fn hsl_band_interpolation_blends_evenly_exactly_at_a_boundary() {
        // Confine fra banda "Giallo" (indice 2, valore 0) e "Verde" (indice
        // 3, valore 100): a hue=135° (il confine esatto) l'atteso è la media.
        let values = [0, 0, 0, 100, 0, 0, 0, 0];
        let at_boundary = interpolate_hsl_band(&values, 135.0);
        assert!((at_boundary - 50.0).abs() < 0.01, "at_boundary={at_boundary}");
    }

    #[test]
    fn hsl_band_interpolation_wraps_around_360_degrees() {
        // Confine fra banda "Magenta" (indice 7, valore 10) e "Rosso"
        // (indice 0, valore 50), che cade a hue=0/360.
        let values = [50, 0, 0, 0, 0, 0, 0, 10];
        let at_wrap = interpolate_hsl_band(&values, 0.0);
        assert!((at_wrap - 30.0).abs() < 0.01, "at_wrap={at_wrap}");
    }

    #[test]
    fn hsl_band_interpolation_has_no_hard_jump_across_a_boundary() {
        // Il bug corretto in questo giro: con la vecchia implementazione a
        // bucket netto, due tonalità a un solo grado di distanza attorno al
        // confine (134° e 136°) avrebbero prodotto una differenza di 100
        // (l'intero salto di banda); con l'interpolazione devono restare
        // vicine.
        let values = [0, 0, 0, 100, 0, 0, 0, 0];
        let just_below = interpolate_hsl_band(&values, 134.0);
        let just_above = interpolate_hsl_band(&values, 136.0);
        assert!(
            (just_above - just_below).abs() < 10.0,
            "salto troppo grande attorno al confine: below={just_below} above={just_above}"
        );
    }

    #[test]
    fn render_preview_hsl_saturation_has_no_hard_jump_across_a_hue_band_boundary() {
        // Regressione end-to-end del bug segnalato dall'utente ("immagine
        // posterizzata a blocchi"): due tinte unite a tonalità quasi identica
        // (134° e 136°, appena ai due lati del confine banda Giallo/Verde),
        // con un Look che alza MOLTO la saturazione della sola banda "Verde"
        // (indice 3, +100) e lascia invariata quella "Giallo" (indice 2, 0).
        // Con il vecchio bucket netto le due immagini sarebbero finite con
        // saturazioni radicalmente diverse pur partendo da tonalità quasi
        // identiche; con l'interpolazione la differenza deve restare piccola.
        let mut look = HarmonicLook::default();
        look.hsl.sat[3] = 100;

        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let below_rgb = hsl_to_rgb([134.0, 0.5, 0.5]);
        let above_rgb = hsl_to_rgb([136.0, 0.5, 0.5]);
        let img_below = solid_image(4, 4, [to_u8(below_rgb[0]), to_u8(below_rgb[1]), to_u8(below_rgb[2])]);
        let img_above = solid_image(4, 4, [to_u8(above_rgb[0]), to_u8(above_rgb[1]), to_u8(above_rgb[2])]);

        let rendered_below = render_preview_with_look(&img_below, &look).to_rgba8();
        let rendered_above = render_preview_with_look(&img_above, &look).to_rgba8();

        let px_to_hsl = |px: image::Rgba<u8>| {
            rgb_to_hsl([px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0])
        };
        let hsl_below = px_to_hsl(*rendered_below.get_pixel(0, 0));
        let hsl_above = px_to_hsl(*rendered_above.get_pixel(0, 0));

        assert!(
            (hsl_above[1] - hsl_below[1]).abs() < 0.15,
            "salto di saturazione troppo grande attorno al confine banda: below={} above={}",
            hsl_below[1],
            hsl_above[1]
        );
    }

    #[test]
    fn negative_global_vibrance_protects_a_very_saturated_pixel_more_than_a_moderately_saturated_one() {
        // Bug reale scoperto e corretto in questo giro, segnalato dall'utente
        // con due foto vere (i sedili rossi di un'auto): "Incolla impostazioni"
        // da una foto campione con un ampio sfondo quasi neutro (es. asfalto
        // grigio) produce un `vibrance` globale molto negativo (misurato:
        // -55 su una foto vera) — ma applicarlo come moltiplicatore PIATTO
        // (com'era prima) colpisce in valore ASSOLUTO proprio i pixel già più
        // saturi (il soggetto colorato, es. pelle rossa dei sedili) più forte
        // dei pixel già quasi grigi (che di saturazione ne hanno poca da
        // perdere) — l'opposto di cosa significa "vibrance" in un editor
        // fotografico reale, a differenza di "saturation". Qui due pixel
        // sintetici con la STESSA tinta ma saturazione HSL di partenza
        // diversa (uno moderato, uno molto vicino al pieno): con lo stesso
        // `vibrance` molto negativo, il pixel più saturo deve perdere una
        // frazione RELATIVA della propria saturazione minore di quello
        // moderatamente saturo — mai il contrario.
        let mut look = HarmonicLook::default();
        look.vibrance = -55;

        let moderate_rgb = hsl_to_rgb([0.0, 0.45, 0.5]);
        let vivid_rgb = hsl_to_rgb([0.0, 0.95, 0.5]);
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let img_moderate = solid_image(4, 4, [to_u8(moderate_rgb[0]), to_u8(moderate_rgb[1]), to_u8(moderate_rgb[2])]);
        let img_vivid = solid_image(4, 4, [to_u8(vivid_rgb[0]), to_u8(vivid_rgb[1]), to_u8(vivid_rgb[2])]);

        let sat_of = |img: &DynamicImage| {
            let rendered = render_preview_with_look(img, &look).to_rgba8();
            let px = rendered.get_pixel(0, 0);
            rgb_to_hsl([px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0])[1]
        };

        let moderate_relative_drop = 1.0 - sat_of(&img_moderate) / 0.45;
        let vivid_relative_drop = 1.0 - sat_of(&img_vivid) / 0.95;

        assert!(
            vivid_relative_drop < moderate_relative_drop,
            "un pixel molto saturo deve perdere una frazione minore della propria saturazione di uno moderato: \
             calo relativo moderato={moderate_relative_drop:.3} vivido={vivid_relative_drop:.3}"
        );
        // Guardrail assoluto (non solo relativo): sulle foto vere che hanno
        // fatto scoprire il bug, i sedili (saturazione HSL originale ~0.6-0.7)
        // non devono perdere più del 15% della propria saturazione per un
        // `vibrance` di questa entità — prima della correzione ne perdevano
        // oltre il 30% (moltiplicatore piatto 0.725).
        assert!(
            vivid_relative_drop < 0.15,
            "un pixel molto saturo non deve perdere più del 15% della saturazione: calo={vivid_relative_drop:.3}"
        );
    }

    #[test]
    fn hue_band_weight_ramps_from_zero_to_one_and_stays_at_one_above_the_threshold() {
        assert_eq!(hue_band_weight(0.0), 0.0);
        assert_eq!(hue_band_weight(HUE_BAND_LOW_CHROMA_RAMP), 1.0);
        assert_eq!(hue_band_weight(1.0), 1.0);
        let mid = hue_band_weight(HUE_BAND_LOW_CHROMA_RAMP / 2.0);
        assert!(mid > 0.0 && mid < 1.0, "il peso a metà rampa deve essere strettamente fra 0 e 1: {mid}");
    }

    #[test]
    fn hue_band_weight_is_low_for_a_near_black_pixel_even_when_its_hsl_saturation_reads_high() {
        // Verifica diretta del motivo per cui il primo tentativo di questo
        // fix (pesare su `hsl[1]`, la saturazione HSL) non funzionava: vicino
        // al nero (L piccola) la formula di `rgb_to_hsl` fa esplodere `s`
        // anche per una croma assoluta minuscola. Un pixel con L=0.02 e
        // saturazione HSL RIPORTATA di 0.5 (che con la vecchia soglia
        // 0.12 avrebbe ricevuto peso 1.0, cioè PIENO effetto) ha in realtà
        // una croma assoluta di appena 0.5*(1-|2*0.02-1|) = 0.5*0.04 = 0.02
        // — praticamente nessun colore vero. Pesando sulla croma, il peso
        // deve restare basso.
        let l = 0.02_f32;
        let s = 0.5_f32;
        let chroma = s * (1.0 - (2.0 * l - 1.0).abs());
        assert!(chroma < 0.03, "croma attesa minuscola per questo caso: {chroma}");
        let weight = hue_band_weight(chroma);
        assert!(
            weight < 0.5,
            "un pixel quasi nero non deve ricevere piena forza solo perché la sua saturazione HSL riportata è alta: chroma={chroma} peso={weight}"
        );
    }

    #[test]
    fn near_black_pixels_are_shielded_from_per_band_hsl_noise_even_across_opposite_hue_bands() {
        // Regressione end-to-end del **quarto bug reale**, distinto dal
        // precedente "salto ripido fra bande": un pixel quasi nero (come il
        // paraurti scuro di una foto vera in ombra) ha una tonalità
        // numericamente instabile — il minimo rumore di sensore o JPEG lo fa
        // oscillare da una banda all'altra, E la sua saturazione HSL
        // riportata può risultare artificialmente alta (vedi il test sopra e
        // il commento esteso su `hue_band_weight`) anche quando la croma
        // assoluta è minuscola. Se l'aggiustamento per banda venisse
        // applicato a piena forza (o pesato sulla saturazione HSL invece che
        // sulla croma), due pixel quasi identici — stessa luminanza quasi
        // nera, stessa saturazione HSL "riportata", tonalità diversa solo
        // per via del rumore — finirebbero con saturazioni finali molto
        // diverse: la chiazza di rumore cromatico osservata sulla foto vera.
        // Qui si simula il caso peggiore: due pixel quasi neri (L=0.02) con
        // saturazione HSL riportata identica (0.5, volutamente alta per
        // testare proprio il caso che il fix basato su `hsl[1]` non
        // copriva) ma tonalità agli antipodi (10° contro 190°), con un Look
        // che ha un bias di saturazione per banda molto diverso da un lato
        // all'altro (+80 in una banda, -80 nella banda opposta).
        let mut look = HarmonicLook::default();
        look.hsl.sat[0] = 80; // banda centrata a 22.5° circa (copre l'hue=10°)
        look.hsl.sat[4] = -80; // banda opposta, circa 202.5° (copre l'hue=190°)

        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let low_hue_rgb = hsl_to_rgb([10.0, 0.5, 0.02]);
        let high_hue_rgb = hsl_to_rgb([190.0, 0.5, 0.02]);
        let img_low = solid_image(4, 4, [to_u8(low_hue_rgb[0]), to_u8(low_hue_rgb[1]), to_u8(low_hue_rgb[2])]);
        let img_high = solid_image(4, 4, [to_u8(high_hue_rgb[0]), to_u8(high_hue_rgb[1]), to_u8(high_hue_rgb[2])]);

        let rendered_low = render_preview_with_look(&img_low, &look).to_rgba8();
        let rendered_high = render_preview_with_look(&img_high, &look).to_rgba8();

        // Si confronta la CROMA ASSOLUTA finale, non la saturazione HSL: è
        // proprio l'instabilità di quest'ultima vicino al nero il punto che
        // questo test verifica, quindi usarla anche per l'assert
        // renderebbe il confronto inaffidabile esattamente dove conta.
        let px_to_chroma = |px: image::Rgba<u8>| {
            let rgb = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
            rgb.iter().cloned().fold(f32::MIN, f32::max) - rgb.iter().cloned().fold(f32::MAX, f32::min)
        };
        let chroma_low = px_to_chroma(*rendered_low.get_pixel(0, 0));
        let chroma_high = px_to_chroma(*rendered_high.get_pixel(0, 0));

        assert!(
            (chroma_low - chroma_high).abs() < 0.03,
            "due pixel quasi neri non devono divergere in croma solo per un bias di banda opposto: chroma_low={chroma_low} chroma_high={chroma_high}"
        );
    }

    #[test]
    fn gradient_white_balance_is_ignored_when_disabled() {
        // white_balance_b da solo, senza wb_gradient_enabled, non deve avere
        // alcun effetto: comportamento identico al singolo WB pre-esistente.
        let img = solid_image(4, 4, [128, 128, 128]);
        let mut look = HarmonicLook::default();
        look.white_balance_b.temp = 2500;
        assert!(!look.wb_gradient_enabled);

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        assert_eq!(rendered.get_pixel(0, 0), baseline.get_pixel(0, 0));
    }

    /// Immagine sintetica con rumore deterministico pseudo-casuale (nessuna
    /// dipendenza da `rand`): un pattern a scacchiera di piccola ampiezza,
    /// sovrapposto a un colore di base uniforme, su un canale scelto
    /// (0=R/G/B insieme = rumore di luminanza, altrimenti solo un canale =
    /// rumore quasi puramente cromatico). Deterministico e riproducibile,
    /// a differenza di un vero rumore casuale — sufficiente per verificare che
    /// la riduzione rumore SMORZI l'ampiezza pixel-a-pixel senza dipendere da
    /// un seed.
    fn noisy_image(width: u32, height: u32, base: [u8; 3], amplitude: i32, luma_noise: bool) -> DynamicImage {
        let img = ImageBuffer::from_fn(width, height, |x, y| {
            let sign = if (x + y) % 2 == 0 { 1 } else { -1 };
            let delta = sign * amplitude;
            if luma_noise {
                let r = (base[0] as i32 + delta).clamp(0, 255) as u8;
                let g = (base[1] as i32 + delta).clamp(0, 255) as u8;
                let b = (base[2] as i32 + delta).clamp(0, 255) as u8;
                Rgba([r, g, b, 255])
            } else {
                // Solo il canale rosso oscilla: variazione quasi puramente di
                // tinta/croma a parità di luminanza media, non di luminosità.
                let r = (base[0] as i32 + delta).clamp(0, 255) as u8;
                Rgba([r, base[1], base[2], 255])
            }
        });
        DynamicImage::ImageRgba8(img)
    }

    /// Rumore residuo di un'immagine renderizzata: deviazione standard dei
    /// valori di un canale su un'area piatta (nessun contenuto reale, solo il
    /// pattern di rumore sintetico) — una riduzione rumore efficace deve
    /// abbassarla, non lasciarla invariata né (peggio) alzarla.
    fn channel_std_dev(image: &image::RgbaImage, channel: usize) -> f64 {
        let values: Vec<f64> = image.pixels().map(|p| p[channel] as f64).collect();
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt()
    }

    #[test]
    fn noise_reduction_has_no_effect_at_zero_strength() {
        let img = noisy_image(32, 32, [120, 120, 120], 15, true);
        let mut look = HarmonicLook::default();
        look.noise_reduction_luma = 0;
        look.noise_reduction_color = 0;

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        assert_eq!(rendered, baseline, "a intensità 0 la riduzione rumore non deve cambiare nulla");
    }

    #[test]
    fn luma_noise_reduction_reduces_pixel_to_pixel_variation_in_a_flat_area() {
        // Rumore sui tre canali insieme (variazione di LUMINANZA): con
        // `noise_reduction_luma` alto, la deviazione standard del canale
        // rosso su quest'area piatta deve calare sensibilmente.
        let img = noisy_image(48, 48, [120, 120, 120], 20, true);
        let mut look = HarmonicLook::default();
        look.noise_reduction_luma = 100;

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let before = channel_std_dev(&img.to_rgba8(), 0);
        let after = channel_std_dev(&rendered, 0);
        assert!(
            after < before * 0.5,
            "atteso un calo sostanziale del rumore di luminanza: prima={before:.2} dopo={after:.2}"
        );
    }

    #[test]
    fn color_noise_reduction_reduces_chroma_variation_without_needing_luma_reduction() {
        // Rumore solo sul canale rosso (variazione quasi puramente cromatica):
        // con SOLO `noise_reduction_color` alto (luma a 0), la deviazione
        // standard del canale rosso deve comunque calare, perché il rumore è
        // portato dai canali a*/b* di Lab, non da L.
        let img = noisy_image(48, 48, [120, 120, 120], 20, false);
        let mut look = HarmonicLook::default();
        look.noise_reduction_luma = 0;
        look.noise_reduction_color = 100;

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let before = channel_std_dev(&img.to_rgba8(), 0);
        let after = channel_std_dev(&rendered, 0);
        assert!(
            after < before * 0.5,
            "atteso un calo sostanziale del rumore cromatico: prima={before:.2} dopo={after:.2}"
        );
    }

    #[test]
    fn noise_reduction_preserves_a_sharp_edge_instead_of_blurring_it_away() {
        // Un bordo netto (metà nera, metà bianca) con riduzione rumore al
        // massimo non deve spianarsi in un morbido grigio: la protezione ai
        // bordi (`edge_weight`) deve tenere i due lati quasi ai loro valori
        // originali, non fonderli.
        let img = ImageBuffer::from_fn(32, 32, |x, _| {
            if x < 16 {
                Rgba([10, 10, 10, 255])
            } else {
                Rgba([245, 245, 245, 255])
            }
        });
        let img = DynamicImage::ImageRgba8(img);
        let mut look = HarmonicLook::default();
        look.noise_reduction_luma = 100;
        look.noise_reduction_color = 100;

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let dark_side = rendered.get_pixel(4, 16)[0] as i32;
        let bright_side = rendered.get_pixel(27, 16)[0] as i32;
        assert!(dark_side < 40, "il lato scuro non deve schiarirsi verso il grigio: {dark_side}");
        assert!(bright_side > 220, "il lato chiaro non deve scurirsi verso il grigio: {bright_side}");
    }

    #[test]
    fn noise_reduction_on_a_solid_color_image_changes_nothing() {
        // Un'immagine a tinta piatta non ha rumore da ridurre: qualunque
        // sfocatura di un'area completamente uniforme restituisce lo stesso
        // valore, quindi il render deve restare identico a intensità 0.
        let img = solid_image(16, 16, [80, 140, 200]);
        let mut look = HarmonicLook::default();
        look.noise_reduction_luma = 100;
        look.noise_reduction_color = 100;

        let rendered = render_preview_with_look(&img, &look).to_rgba8();
        let baseline = render_preview_with_look(&img, &HarmonicLook::default()).to_rgba8();
        assert_eq!(
            rendered.get_pixel(8, 8),
            baseline.get_pixel(8, 8),
            "una tinta piatta non deve cambiare con la riduzione rumore, qualunque sia l'intensità"
        );
    }

    // --- Pipeline f32 a piena risoluzione (aggiunto in questo giro) ---

    #[test]
    fn render_full_resolution_with_look_returns_an_rgb32f_image_of_the_same_dimensions() {
        let img = solid_image(6, 4, [90, 150, 210]);
        let rendered = render_full_resolution_with_look(&img, &HarmonicLook::default());
        assert!(
            rendered.as_rgb32f().is_some(),
            "il rendering a piena risoluzione deve restituire ImageRgb32F, non 8 bit"
        );
        assert_eq!(rendered.dimensions(), img.dimensions());
    }

    #[test]
    fn full_resolution_render_preserves_sub_8bit_precision_not_snapped_to_a_255_step_grid() {
        // 85.5/255 cade ESATTAMENTE a metà fra due livelli 8 bit consecutivi
        // (85 e 86): se in un punto qualunque della pipeline il buffer
        // venisse arrotondato a 8 bit (come accadeva prima di questo giro,
        // con la riduzione rumore che scriveva un `RgbaImage` a metà
        // pipeline), il valore in uscita finirebbe forzatamente su UNO dei
        // due livelli — qui verifichiamo che non sia agganciato a nessuno dei
        // due (a differenza di un valore come 1/3, che per puro caso
        // numerico cade quasi esattamente su un livello già esistente,
        // 85/255, e non sarebbe un test valido).
        let value = 85.5_f32 / 255.0;
        let buf: image::ImageBuffer<image::Rgb<f32>, Vec<f32>> =
            ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([value, value, value]));
        let source = DynamicImage::ImageRgb32F(buf);

        let rendered = render_full_resolution_with_look(&source, &HarmonicLook::default());
        let out = rendered.as_rgb32f().expect("ImageRgb32F atteso");
        let out_v = out.get_pixel(0, 0)[0];

        let nearest_255_step = (out_v * 255.0).round() / 255.0;
        assert!(
            (out_v - nearest_255_step).abs() > 0.0005,
            "il valore f32 in uscita ({out_v}) non deve essere agganciato a un livello 0..255: pipeline non più esclusivamente f32?"
        );
        // Con un Look neutro il round-trip srgb<->lineare e HSL<->RGB deve
        // restituire (quasi) lo stesso valore in ingresso, non una versione
        // quantizzata: la differenza residua è solo l'errore in virgola
        // mobile dei round-trip matematici, non un arrotondamento a step fisso.
        assert!((out_v - value).abs() < 0.01, "un Look neutro non deve alterare percettibilmente il valore: {out_v} vs {value}");
    }

    #[test]
    fn render_full_resolution_and_render_preview_agree_within_8bit_rounding_on_an_8bit_source() {
        // Stesso motore (`render_look_core`) dietro entrambe le funzioni
        // pubbliche: partendo dalla STESSA sorgente 8 bit e applicando lo
        // STESSO Look, i due percorsi devono restituire lo stesso colore a
        // meno dell'arrotondamento finale a 8 bit del percorso anteprima.
        let img = solid_image(4, 4, [60, 130, 200]);
        let mut look = HarmonicLook::default();
        look.exposure_ev = 0.3;
        look.contrast = 15;

        let preview_px = render_preview_with_look(&img, &look).to_rgba8().get_pixel(0, 0).0;
        let full = render_full_resolution_with_look(&img, &look);
        let full_rgb32f = full.as_rgb32f().expect("ImageRgb32F atteso");
        let full_px = full_rgb32f.get_pixel(0, 0);

        for c in 0..3 {
            let full_as_u8 = (full_px[c].clamp(0.0, 1.0) * 255.0).round() as i32;
            let diff = (full_as_u8 - preview_px[c] as i32).abs();
            assert!(diff <= 1, "canale {c}: pieno={full_as_u8} anteprima={} (differenza oltre l'arrotondamento atteso)", preview_px[c]);
        }
    }

    #[test]
    fn u8_to_f32_and_back_round_trip_is_lossless_for_every_byte_value() {
        // La conversione u8 <-> f32 usata ai bordi della pipeline (anteprima
        // interattiva) non deve introdurre alcuna perdita: ogni singolo
        // valore 0..255 deve tornare esattamente identico dopo /255.0 poi
        // *255.0 arrotondato.
        let img: image::RgbaImage = ImageBuffer::from_fn(256, 1, |x, _| {
            let v = x as u8;
            Rgba([v, v, v, 255])
        });
        let as_f32 = u8_rgba_to_f32(&img);
        let back = rgba_f32_to_u8(&as_f32);
        assert_eq!(back, img, "il giro u8 -> f32 -> u8 deve essere perfettamente senza perdita per ogni livello");
    }

    #[test]
    fn rgb32f_and_rgba_f32_round_trip_preserves_color_and_forces_full_alpha() {
        // 0.25/0.5/0.75: frazioni binarie esatte (potenze di 2), scelte
        // apposta per poter confrontare i risultati con `assert_eq!` esatto
        // senza rischiare un falso negativo per errore di arrotondamento
        // dell'ultimo bit (cosa che capiterebbe con una frazione come 0.1,
        // non rappresentabile esattamente in virgola mobile binaria).
        let buf: image::ImageBuffer<image::Rgb<f32>, Vec<f32>> =
            ImageBuffer::from_fn(3, 2, |x, y| image::Rgb([x as f32 * 0.25, y as f32 * 0.25, 0.75]));
        let as_rgba = rgb32f_to_rgba_f32(&buf);
        assert_eq!(as_rgba.get_pixel(2, 1).0, [0.5, 0.25, 0.75, 1.0]);
        let back = rgba_f32_to_rgb32f(&as_rgba);
        assert_eq!(back.get_pixel(2, 1).0, [0.5, 0.25, 0.75]);
    }
}
