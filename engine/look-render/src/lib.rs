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
//! trasferire lo STILE caldo/freddo di un look è sufficiente. Sharpening e
//! riduzione rumore restano pianificati per la Fase 3-4 della roadmap (§8).

use color_science::{hsl_to_rgb, linear_to_srgb, rgb_to_hsl, srgb_to_linear};
use core_types::HarmonicLook;
use image::DynamicImage;
use rayon::prelude::*;

const HUE_BANDS: usize = 8;

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
fn apply_texture_bands(base: &image::RgbaImage, look: &HarmonicLook) -> image::RgbaImage {
    if look.texture_fine == 0 && look.texture_medium == 0 && look.texture_coarse == 0 {
        return base.clone();
    }
    const SIGMA_FINE: f32 = 1.2;
    const SIGMA_MEDIUM: f32 = 4.0;
    const SIGMA_COARSE: f32 = 10.0;

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
                    let base_v = base_row[px + c] as f32;
                    let bf = bf_row[px + c] as f32;
                    let bm = bm_row[px + c] as f32;
                    let bc = bc_row[px + c] as f32;
                    let f_detail = base_v - bf;
                    let m_detail = bf - bm;
                    let c_detail = bm - bc;
                    let reconstructed = bc + f_detail * fine_mul + m_detail * medium_mul + c_detail * coarse_mul;
                    out_row[px + c] = reconstructed.round().clamp(0.0, 255.0) as u8;
                }
                out_row[px + 3] = base_row[px + 3];
            }
        });
    out
}

/// Applica un `HarmonicLook` ai pixel di `image`, restituendo una nuova
/// immagine della stessa dimensione. Ordine degli stage (docs/ARCHITECTURE.md
/// §3.2, sezione "Detail"/NR esclusa): bilanciamento del bianco + esposizione
/// -> highlights/shadows -> tone curve -> contrasto -> HSL per banda + split
/// toning -> vibrance/saturazione globale.
pub fn render_preview_with_look(image: &DynamicImage, look: &HarmonicLook) -> DynamicImage {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let row_stride = 4 * width as usize;

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
    // Guardrail: anche se `saturation`/`vibrance` in teoria arrivano da
    // `HarmonicLook` già limitati a +-100, mai spingere il moltiplicatore di
    // saturazione globale a un estremo che desaturi (quasi) completamente o
    // esploda l'immagine — un Look estratto da una scena con ampie zone quasi
    // neutre (es. asfalto, cielo uniforme) può produrre una stima di vibrance
    // molto negativa che, da sola, non rappresenta l'intento stilistico da
    // trasferire quanto un artefatto della composizione della foto campione.
    let global_sat_mul = (1.0 + (look.saturation as f32 / 100.0) + (look.vibrance as f32 / 200.0)).clamp(0.35, 2.5);

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
                    srgb_to_linear(in_px[0] as f32 / 255.0),
                    srgb_to_linear(in_px[1] as f32 / 255.0),
                    srgb_to_linear(in_px[2] as f32 / 255.0),
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
                let hue_adjust = interpolate_hsl_band(&look.hsl.hue, hsl[0]);
                let sat_adjust = interpolate_hsl_band(&look.hsl.sat, hsl[0]);
                let lum_adjust = interpolate_hsl_band(&look.hsl.lum, hsl[0]);
                hsl[0] = (hsl[0] + hue_adjust).rem_euclid(360.0);
                hsl[1] = (hsl[1] * (1.0 + sat_adjust / 100.0) * global_sat_mul).clamp(0.0, 1.0);
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

                let final_rgb = hsl_to_rgb(hsl);
                out_px[0] = (final_rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                out_px[1] = (final_rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                out_px[2] = (final_rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                out_px[3] = in_px[3];
            }
        });

    // Texture (separazione di frequenza) è un'operazione spaziale, non
    // per-pixel: va applicata come passata separata sull'immagine già
    // color-gradata dal loop qui sopra, non dentro di esso.
    let out = apply_texture_bands(&out, look);

    DynamicImage::ImageRgba8(out)
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
}
