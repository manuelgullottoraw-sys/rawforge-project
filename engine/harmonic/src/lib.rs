//! Sintesi Armonica Automatica (docs/ARCHITECTURE.md, §4.1).
//!
//! Estrae da un'immagine di riferimento (look cinematografico o foto guida) un
//! `HarmonicLook`: tone curve, palette per zona tonale (-> split toning), stima
//! del bilanciamento del bianco e del contrasto. Le formule di normalizzazione
//! (baseline di contrasto/crominanza, mappatura EV) sono euristiche calibrabili
//! empiricamente, non misure fotometriche assolute — sono commentate come tali
//! ovunque compaiono, così è chiaro cosa va tarato con un vero corpus di foto.

use color_science::{lab_ab_to_hue_chroma, linear_rgb_to_lab, rgb_to_hsl, srgb_to_linear};
use core_types::{HarmonicLook, HslAdjustments, SplitToning};
use image::DynamicImage;

const ANALYSIS_MAX_DIM: u32 = 512;
const HUE_BANDS: usize = 8;
/// Sotto questa popolazione di pixel una banda HSL viene lasciata a zero: la
/// media di pochi pixel è rumore, non uno stile da copiare (stesso principio
/// del guardrail su `min_band_pixels` più sotto).
const MIN_BAND_PIXELS: u64 = 40;
/// Luminanza Lab "tipica" per un grigio medio (18%), usata come pivot per la
/// stima (euristica) dell'esposizione relativa.
const NEUTRAL_L: f32 = 50.0;

/// Deviazione standard di L attesa per un'immagine "normocontrastata": baseline
/// empirica da ricalibrare su un corpus reale.
const BASELINE_CONTRAST_STD: f32 = 20.0;

/// Chroma Lab media attesa per una scena fotografica "normale": baseline per
/// il bias GLOBALE di vibrance/saturation. **Ricalibrata in questo giro**: era
/// 18.0, una stima "a occhio" mai verificata su una foto vera. Misurata ora su
/// due foto reali fornite dall'utente: chroma media effettiva ~4.5 e ~8.5 —
/// MENO DELLA METÀ della vecchia baseline in entrambi i casi. Con 18.0 anche
/// una foto normale (non deliberatamente scarica di colore) risultava sempre
/// giudicata "poco satura", producendo un `vibrance` fortemente negativo quasi
/// sempre: una delle due cause (insieme al bug di `hsl_sat` qui sotto) per cui
/// "Incolla impostazioni" desaturava la foto target ben oltre la foto
/// campione stessa. 10.0 è ancora una stima (due foto non sono un corpus), ma
/// ora ancorata a una misura reale invece che a un'ipotesi.
const BASELINE_CHROMA: f32 = 10.0;

/// Chroma Lab "tipica" attesa per la zona ombre o luci di una foto QUALUNQUE,
/// anche senza alcuna intenzione di color grading — stessa logica di
/// `BASELINE_CHROMA`, ma per una singola zona tonale (ombre O luci, non
/// l'intera foto) invece che per l'immagine intera. **Bug reale scoperto e
/// corretto in un giro precedente**:
/// a differenza di `hsl_sat`/`vibrance` (entrambi già uno scarto RELATIVO a
/// una baseline), `split_toning.shadow_sat`/`highlight_sat` usava la chroma
/// Lab GREZZA della zona, senza sottrarre nulla — quindi anche una foto
/// campione scattata alla luce del giorno, senza alcuna gradazione voluta
/// (solo la normale differenza di colore fra cielo/ombra e sole diretto/luce
/// che ha QUALSIASI scatto), produceva uno split toning non trascurabile.
/// Con "Incolla impostazioni" questo valore viene copiato per intero sul
/// target (mai sull'editing manuale, dove split toning parte sempre da 0) —
/// applicato per giunta su zone tonali ampie (ombre sotto luma 0.4, luci
/// sopra 0.6: in molte foto reali la maggioranza dei pixel), quindi anche un
/// valore moderato tinge una porzione ampia del fotogramma. Combinato con
/// `hsl_sat` (fisso in questo stesso giro), è una seconda causa reale e
/// distinta della dominante diffusa segnalata dall'utente — presente SOLO
/// quando si incollano le impostazioni, mai con l'editing manuale, perché lo
/// split toning manuale parte sempre da 0/0 e questo bug riguarda solo
/// l'ESTRAZIONE automatica.
const BASELINE_SPLIT_CHROMA: f32 = 6.0;

/// Accumulatore per banda di tonalità (Red/Orange/Yellow/Green/Aqua/Blue/
/// Purple/Magenta, stesso schema a 8 bande usato da `look-render` per
/// applicare l'HSL) — media di saturazione, luminanza e hue dei pixel di
/// quella banda nella foto campione.
struct HueBandBucket {
    sum_sat: f64,
    sum_lum: f64,
    sum_hue: f64,
    count: u64,
}

impl HueBandBucket {
    fn new() -> Self {
        Self { sum_sat: 0.0, sum_lum: 0.0, sum_hue: 0.0, count: 0 }
    }
}

struct LumaBucket {
    sum_a: f64,
    sum_b: f64,
    count: u64,
}

impl LumaBucket {
    fn new() -> Self {
        Self { sum_a: 0.0, sum_b: 0.0, count: 0 }
    }

    fn push(&mut self, a: f32, b: f32) {
        self.sum_a += a as f64;
        self.sum_b += b as f64;
        self.count += 1;
    }

    fn centroid(&self) -> (f32, f32) {
        if self.count == 0 {
            (0.0, 0.0)
        } else {
            ((self.sum_a / self.count as f64) as f32, (self.sum_b / self.count as f64) as f32)
        }
    }
}

/// Media mobile circolare a 3 prese (60% valore corrente + 20% ciascuna
/// banda adiacente) sulle 8 bande di tonalità: vedi il commento esteso dove
/// viene chiamata, in [`extract_look_from_reference`], per il bug reale che
/// corregge (rumore/"glitch" di saturazione su un salto ripido fra bande
/// adiacenti). `round()` invece di troncamento perché questi sono già valori
/// piccoli (range -50..50 o meno): troncare introdurrebbe un bias sistematico
/// verso lo zero.
fn smooth_circular_bands(values: [i32; HUE_BANDS]) -> [i32; HUE_BANDS] {
    let mut out = [0i32; HUE_BANDS];
    for i in 0..HUE_BANDS {
        let prev = values[(i + HUE_BANDS - 1) % HUE_BANDS] as f32;
        let curr = values[i] as f32;
        let next = values[(i + 1) % HUE_BANDS] as f32;
        out[i] = (curr * 0.6 + prev * 0.2 + next * 0.2).round() as i32;
    }
    out
}

fn percentile(sorted: &[f32], p: f64) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Estrae un `HarmonicLook` da un'immagine di riferimento già caricata in memoria.
pub fn extract_look_from_reference(img: &DynamicImage, name: &str) -> HarmonicLook {
    let analysis = img.resize(ANALYSIS_MAX_DIM, ANALYSIS_MAX_DIM, image::imageops::FilterType::Triangle);
    let rgba = analysis.to_rgba8();

    let mut l_values: Vec<f32> = Vec::with_capacity(rgba.pixels().len());
    let mut ab_values: Vec<(f32, f32)> = Vec::with_capacity(rgba.pixels().len());

    let mut sum_lin = [0f64; 3];
    let mut sum_chroma = 0f64;
    let mut hue_bands: [HueBandBucket; HUE_BANDS] = std::array::from_fn(|_| HueBandBucket::new());
    let mut sum_hsl_lum = 0f64;
    let mut sum_hsl_sat = 0f64;

    for px in rgba.pixels() {
        let r = px[0] as f32 / 255.0;
        let g = px[1] as f32 / 255.0;
        let b = px[2] as f32 / 255.0;
        let lin = [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)];
        sum_lin[0] += lin[0] as f64;
        sum_lin[1] += lin[1] as f64;
        sum_lin[2] += lin[2] as f64;

        let lab = linear_rgb_to_lab(lin);
        l_values.push(lab[0]);
        ab_values.push((lab[1], lab[2]));

        let (_, chroma) = lab_ab_to_hue_chroma(lab[1], lab[2]);
        sum_chroma += chroma as f64;

        // HSL per banda (per il bias di saturazione/luminanza/hue più sotto):
        // calcolata dallo stesso pixel sRGB, non da Lab — deve corrispondere
        // esattamente allo spazio in cui `look-render` applica l'HSL per
        // banda, altrimenti un pixel finirebbe extratto in una banda e
        // renderizzato in un'altra.
        let hsl_px = rgb_to_hsl([r, g, b]);
        sum_hsl_lum += hsl_px[2] as f64;
        // A differenza di `bucket.sum_sat` più sotto (solo pixel cromatici,
        // per il bias PER BANDA), questa somma è su TUTTI i pixel, pixel
        // acromatici inclusi — serve a sapere quanto sia satura la foto nel
        // suo COMPLESSO, il confronto giusto per "questa banda è più/meno
        // satura del resto di QUESTA foto" (vedi `overall_hsl_sat` più sotto).
        sum_hsl_sat += hsl_px[1] as f64;
        if hsl_px[1] > 0.02 {
            // Pixel quasi acromatici esclusi dalle bande: il loro hue non è
            // affidabile (rumore numerico attorno a un colore grigio), e
            // includerli sposterebbe la media di una banda scelta quasi a
            // caso verso quel rumore.
            let band = (((hsl_px[0] / 45.0) as usize) % HUE_BANDS).min(HUE_BANDS - 1);
            let bucket = &mut hue_bands[band];
            bucket.sum_hue += hsl_px[0] as f64;
            bucket.sum_sat += hsl_px[1] as f64;
            bucket.sum_lum += hsl_px[2] as f64;
            bucket.count += 1;
        }
    }

    let n = l_values.len().max(1) as f64;
    let overall_hsl_lum = (sum_hsl_lum / n) as f32;
    let overall_hsl_sat = (sum_hsl_sat / n) as f32;

    // --- Tone curve: percentili della luminanza come control point ---
    let mut sorted_l = l_values.clone();
    sorted_l.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p25 = percentile(&sorted_l, 25.0);
    let p50 = percentile(&sorted_l, 50.0);
    let p75 = percentile(&sorted_l, 75.0);

    // Punti di controllo della tone curve: NON i percentili assoluti di
    // luminanza del campione, ma la loro posizione RELATIVA al punto mediano
    // (p50) del campione stesso, rimappata attorno al pivot neutro 128. Usare
    // i percentili assoluti (come in una versione precedente) duplicava, in
    // modo non guardrailed, l'informazione già portata da `exposure_ev`: una
    // foto campione scura (p50 basso) produceva una tone curve che schiacciva
    // il midtone di QUALSIASI target verso il basso, sommandosi all'esposizione
    // e aggravando esattamente il tipo di scurimento/appiattimento eccessivo
    // segnalato applicando questo Look a una scena diversa da quella campione.
    // Così la curva trasporta solo la FORMA del roll-off ombre/luci (quanto è
    // "morbido" o "contrastato" il look), lasciando all'asse esposizione
    // (guardrailato da Smart-Batch in fase di adattamento) l'unico compito di
    // spostare la luminosità assoluta.
    let relative_to_u8 = |l_percent: f32| -> u8 { (128.0 + (l_percent - p50) * 2.55).clamp(0.0, 255.0) as u8 };
    let tone_curve = vec![
        (0u8, 0u8),
        (64, relative_to_u8(p25)),
        (128, 128u8),
        (192, relative_to_u8(p75)),
        (255, 255),
    ];

    // --- Contrasto: deviazione standard di L rispetto alla baseline ---
    let mean_l = l_values.iter().map(|&v| v as f64).sum::<f64>() / n;
    let var_l = l_values.iter().map(|&v| (v as f64 - mean_l).powi(2)).sum::<f64>() / n;
    let std_l = var_l.sqrt() as f32;
    let contrast = (((std_l - BASELINE_CONTRAST_STD) / BASELINE_CONTRAST_STD) * 100.0).clamp(-100.0, 100.0) as i32;

    // --- Esposizione: posizione della mediana rispetto al grigio medio ---
    let exposure_ev = (((p50 - NEUTRAL_L) / NEUTRAL_L) * 2.0).clamp(-2.0, 2.0);

    // --- Palette per zona tonale -> split toning ---
    let mut shadow = LumaBucket::new();
    let mut highlight = LumaBucket::new();
    for (i, &l) in l_values.iter().enumerate() {
        let (a, b) = ab_values[i];
        if l < 33.0 {
            shadow.push(a, b);
        } else if l >= 66.0 {
            highlight.push(a, b);
        }
    }

    let (shadow_a, shadow_b) = shadow.centroid();
    let (highlight_a, highlight_b) = highlight.centroid();
    let (shadow_hue, shadow_chroma) = lab_ab_to_hue_chroma(shadow_a, shadow_b);
    let (highlight_hue, highlight_chroma) = lab_ab_to_hue_chroma(highlight_a, highlight_b);

    let split_toning = SplitToning {
        shadow_hue: shadow_hue.round() as i32,
        // Sottrae la baseline (vedi `BASELINE_SPLIT_CHROMA`) prima di
        // clampare, e con un range dimezzato (0..50, non 0..100) — stessa
        // proporzione già applicata a `hsl_sat` in questo giro, per lo stesso
        // motivo: senza guardrail, la sola chroma "grezza" di una zona tonale
        // non distingue uno stile di grading intenzionale da una normale
        // variazione di colore ambientale.
        shadow_sat: ((shadow_chroma - BASELINE_SPLIT_CHROMA).max(0.0)).clamp(0.0, 50.0).round() as i32,
        highlight_hue: highlight_hue.round() as i32,
        highlight_sat: ((highlight_chroma - BASELINE_SPLIT_CHROMA).max(0.0)).clamp(0.0, 50.0).round() as i32,
        balance: 0,
    };

    // --- Vibrance/Saturation: bias globale dalla chroma media rispetto alla baseline ---
    let mean_chroma = (sum_chroma / n) as f32;
    let vibrance = (((mean_chroma - BASELINE_CHROMA) / BASELINE_CHROMA) * 100.0).clamp(-100.0, 100.0) as i32;

    // --- White balance: stima gray-world (euristica, non un vero CCT solver) ---
    let avg_r = (sum_lin[0] / n) as f32;
    let avg_b = (sum_lin[2] / n) as f32;
    // Positivo => immagine mediamente più calda del neutro; delta espresso come
    // scostamento Kelvin approssimativo da applicare rispetto a un default 5500K.
    let temp_delta = ((avg_r - avg_b) * 2000.0).clamp(-1500.0, 1500.0);
    let temp = (5500.0 + temp_delta).clamp(2000.0, 12000.0) as u32;

    // --- HSL per banda colore: finora `HarmonicLook.hsl` restava sempre a
    // zero (mai popolato), cioè la parte più "da color grading" della
    // Sintesi Armonica — quali colori il look enfatizza/desatura/sposta di
    // hue per zona cromatica — non veniva mai copiata dalla foto campione,
    // solo tone curve/contrasto/vibrance/split-toning globali. Qui si
    // confronta ogni banda con l'INTERA foto campione (non un pivot
    // assoluto), stesso principio già applicato a tone curve ed esposizione:
    // una banda "più chiara/satura del resto della FOTO CAMPIONE" è uno
    // stile da copiare, "questa banda è assolutamente chiara" no.
    let mut hsl_hue = [0i32; HUE_BANDS];
    let mut hsl_sat = [0i32; HUE_BANDS];
    let mut hsl_lum = [0i32; HUE_BANDS];
    for (band, bucket) in hue_bands.iter().enumerate() {
        if bucket.count < MIN_BAND_PIXELS {
            continue;
        }
        let n_band = bucket.count as f64;
        let band_mean_sat = (bucket.sum_sat / n_band) as f32;
        let band_mean_lum = (bucket.sum_lum / n_band) as f32;
        let band_mean_hue = (bucket.sum_hue / n_band) as f32;
        let band_center_hue = band as f32 * 45.0 + 22.5;

        // **Secondo bug reale, più profondo del precedente, scoperto in questo
        // giro confrontando il risultato con le foto vere dell'utente**:
        // l'utente ha misurato (fuori da questo motore) che dopo "Incolla
        // impostazioni" la foto target risultava MOLTO più desaturata della
        // stessa foto campione che doveva copiare (saturazione HSL media:
        // campione ~0.095, target dopo il paste ~0.017 — quasi 6 volte meno
        // satura del campione). Causa: questa riga confrontava
        // `band_mean_sat` con `BASELINE_HSL_SATURATION`, una costante FISSA
        // (0.35) — un confronto ESTERNO, incoerente con `hsl_lum`/`hsl_hue`
        // qui sotto, che invece confrontano ciascuna banda con la media
        // dell'INTERA foto campione (`overall_hsl_lum`/`band_center_hue`) — un
        // confronto INTERNO/relativo. Su una foto uniformemente poco satura
        // (la normalità: gran parte di una scena è pavimentazione, pelle,
        // cielo, grigi quasi neutri) questo spingeva PRATICAMENTE TUTTE le
        // bande verso l'estremo negativo del clamp (misurato: 6 bande su 8 a
        // -50 su una foto vera dell'utente) — e quel bias per-banda si
        // SOMMAVA moltiplicativamente a `vibrance` (bias GLOBALE, calcolato
        // dalla stessa identica caratteristica "la foto è poco colorata"): due
        // meccanismi diversi che penalizzavano DUE VOLTE lo stesso fatto,
        // portando la desaturazione finale ben oltre quella della foto
        // campione stessa. **Corretto** rendendo `hsl_sat` internamente
        // relativo come i suoi due fratelli, invece che ancorato a una
        // costante esterna: una banda viene spinta solo se è più/meno satura
        // del RESTO di QUESTA STESSA foto, non se la foto nel suo insieme è
        // "poco satura in assoluto" (quel giudizio spetta solo a `vibrance`,
        // così il segnale non viene più contato due volte). Scala (150.0,
        // scelta perché uno scarto di banda "notevole" per una foto vera,
        // ~0.3 su scala HSL 0..1, arrivi vicino al tetto del clamp) e range
        // (+-50, invariato dal giro precedente) restano prudenti. Non tocca il
        // range dello slider MANUALE nel pannello Develop (-100..100, in
        // `look-render`): lì è una scelta deliberata dell'utente.
        hsl_sat[band] = ((band_mean_sat - overall_hsl_sat) * 150.0).clamp(-50.0, 50.0) as i32;
        // Range stretto (+-30): un ritocco di luminanza per banda, non una
        // riesposizione mascherata per colore.
        hsl_lum[band] = ((band_mean_lum - overall_hsl_lum) * 200.0).clamp(-30.0, 30.0) as i32;
        // Range ancora più stretto (+-15 gradi): uno scostamento di hue è il
        // ritocco HSL più visibile e rischioso, va tenuto sottile.
        hsl_hue[band] = (band_mean_hue - band_center_hue).clamp(-15.0, 15.0) as i32;
    }

    // **Terzo bug reale scoperto in questo giro, distinto dai due precedenti
    // (vibrance globale piatta, tone curve/contrasto per canale — entrambi già
    // corretti)**: l'utente ha continuato a segnalare un "glitch"/rumore
    // visibile sulla pelle dei sedili anche dopo la correzione della
    // desaturazione globale — non un calo uniforme ma rumore A CHIAZZE,
    // localizzato. Isolato misurando su una foto vera: `hsl_sat` aveva un
    // salto di 45 punti fra due bande ADIACENTI (Viola=-3, Magenta=42) più un
    // altro salto di 25 punti verso Rosso=17 — proprio nella zona di tonalità
    // (300-360°) dove cade la maggior parte dei pixel dei sedili rossi di
    // questa foto. `interpolate_hsl_band` (in `look-render`) rende quella
    // transizione CONTINUA (niente salti netti, già corretto in un giro
    // precedente) ma non ne riduce la PENDENZA: una minuscola variazione di
    // tonalità fra due pixel ADIACENTI (texture della pelle, subsampling
    // cromatico JPEG, rumore del sensore — presente in qualunque foto reale,
    // mai perfettamente uniforme) attraversa quella pendenza ripida e viene
    // amplificata in un salto ben più grande di saturazione applicata —
    // misurato: fino a 49 punti di differenza fra il fattore applicato a due
    // pixel adiacenti, entrambi genuinamente pelle rossa, non un bordo. Questo
    // smoothing (media mobile circolare a 3 prese, 60% banda corrente + 20%
    // ciascuna vicina) riduce la pendenza massima possibile fra due bande
    // vicine SENZA azzerare l'intento originale — una banda chiaramente
    // più/meno satura delle altre nella foto campione resta la più/meno
    // satura anche dopo, solo con un picco più dolce — stesso principio con
    // cui `interpolate_hsl_band` già smussa la transizione ENTRO una banda,
    // applicato qui a monte, fra le bande stesse. Applicato a tutte e tre le
    // dimensioni (hue/sat/lum): stesso meccanismo, stesso rischio di
    // amplificare rumore su un salto ripido fra bande adiacenti.
    let hsl_hue = smooth_circular_bands(hsl_hue);
    let hsl_sat = smooth_circular_bands(hsl_sat);
    let hsl_lum = smooth_circular_bands(hsl_lum);

    let mut look = HarmonicLook {
        name: name.to_string(),
        exposure_ev,
        contrast,
        vibrance,
        split_toning,
        tone_curve,
        hsl: HslAdjustments { hue: hsl_hue, sat: hsl_sat, lum: hsl_lum },
        ..HarmonicLook::default()
    };
    look.white_balance.temp = temp;
    look
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn synthetic_image(pixel_fn: impl Fn(u32, u32) -> [u8; 4]) -> DynamicImage {
        let buf = ImageBuffer::from_fn(64, 64, |x, y| {
            let p = pixel_fn(x, y);
            Rgba(p)
        });
        DynamicImage::ImageRgba8(buf)
    }

    #[test]
    fn does_not_panic_on_uniform_gray() {
        let img = synthetic_image(|_, _| [128, 128, 128, 255]);
        let look = extract_look_from_reference(&img, "Gray Test");
        assert_eq!(look.tone_curve.len(), 5);
        assert_eq!(look.split_toning.shadow_sat, 0, "grigio puro non deve produrre saturazione nelle ombre");
    }

    #[test]
    fn dark_reference_yields_negative_exposure() {
        let img = synthetic_image(|_, _| [20, 20, 20, 255]);
        let look = extract_look_from_reference(&img, "Dark Test");
        assert!(look.exposure_ev < 0.0, "un'immagine di riferimento scura deve dare exposure_ev negativo, got {}", look.exposure_ev);
    }

    #[test]
    fn bright_reference_yields_positive_exposure() {
        let img = synthetic_image(|_, _| [235, 235, 235, 255]);
        let look = extract_look_from_reference(&img, "Bright Test");
        assert!(look.exposure_ev > 0.0, "un'immagine di riferimento chiara deve dare exposure_ev positivo, got {}", look.exposure_ev);
    }

    #[test]
    fn dark_reference_tone_curve_midpoint_stays_neutral() {
        // Il bug segnalato: una foto campione scura (basso-chiave) non deve
        // "trascinare" la tone curve verso il basso — quel compito spetta solo
        // a `exposure_ev` (guardrailato in fase di adattamento). Il midpoint
        // (128 -> 128) resta il pivot neutro indipendentemente da quanto è
        // scura o chiara la foto campione.
        let dark = synthetic_image(|_, _| [20, 20, 20, 255]);
        let bright = synthetic_image(|_, _| [235, 235, 235, 255]);
        let dark_look = extract_look_from_reference(&dark, "Dark");
        let bright_look = extract_look_from_reference(&bright, "Bright");
        assert_eq!(dark_look.tone_curve[2], (128, 128), "midpoint non neutro per campione scuro: {:?}", dark_look.tone_curve);
        assert_eq!(bright_look.tone_curve[2], (128, 128), "midpoint non neutro per campione chiaro: {:?}", bright_look.tone_curve);
    }

    #[test]
    fn contrasty_reference_still_produces_asymmetric_curve_shape() {
        // La decorrelazione dall'esposizione assoluta non deve annullare la
        // capacità della curva di rappresentare la FORMA del contrasto: una
        // scena con forte separazione ombre/luci deve comunque produrre punti
        // di controllo distinti dall'identità.
        let img = synthetic_image(|_, y| if y < 32 { [10, 10, 10, 255] } else { [245, 245, 245, 255] });
        let look = extract_look_from_reference(&img, "Contrasty");
        assert_ne!(look.tone_curve[1], (64, 64), "punto ombre non deve restare sull'identita' per una scena molto contrastata");
        assert_ne!(look.tone_curve[3], (192, 192), "punto luci non deve restare sull'identita' per una scena molto contrastata");
    }

    #[test]
    fn saturated_band_gets_a_positive_saturation_bias_others_stay_at_zero() {
        // Da quando `hsl_sat` è RELATIVO al resto della stessa foto (non più
        // ancorato a una costante fissa — vedi il commento sul bug reale
        // sopra la formula), una banda ottiene un bias positivo solo se è
        // più satura del RESTO della foto, non se è "satura in assoluto": per
        // questo la foto qui sotto ha una metà genuinamente neutra (non solo
        // un angolino) che tiene bassa `overall_hsl_sat`, e una metà rosso
        // molto saturo (230,20,20 sRGB cade in banda 0) che deve risultare
        // notevolmente più satura del resto.
        let img = synthetic_image(|x, _| {
            if x < 32 {
                [128, 128, 128, 255] // metà neutra: esclusa dalle bande, ma abbassa overall_hsl_sat
            } else {
                [230, 20, 20, 255] // metà rosso molto saturo: banda 0
            }
        });
        let look = extract_look_from_reference(&img, "Red Test");
        assert!(look.hsl.sat[0] > 0, "banda rossa (0) deve avere un bias di saturazione positivo, got {}", look.hsl.sat[0]);
        // Una banda senza abbastanza pixel (es. la 4, Aqua) resta a zero: non
        // deve inventare uno stile per un colore assente dalla foto.
        assert_eq!(look.hsl.sat[4], 0, "banda assente dalla foto non deve avere bias, got {}", look.hsl.sat[4]);
    }

    #[test]
    fn uniformly_saturated_photo_gives_no_per_band_bias_only_global_vibrance() {
        // Il bug reale corretto in questo giro, isolato in un test: una foto
        // interamente (non solo per metà) di un unico colore saturo non deve
        // ricevere ALCUN bias per-banda — non c'è "resto della foto" più o
        // meno saturo con cui confrontarsi, quindi il segnale è tutto e solo
        // in `vibrance` (globale), mai duplicato anche per banda.
        let img = synthetic_image(|_, _| [230, 20, 20, 255]);
        let look = extract_look_from_reference(&img, "Uniform Red");
        assert_eq!(look.hsl.sat, [0; 8], "foto uniformemente satura non deve avere bias PER BANDA, got {:?}", look.hsl.sat);
    }

    #[test]
    fn near_neutral_gray_leaves_all_hsl_bands_at_zero() {
        let img = synthetic_image(|_, _| [128, 128, 128, 255]);
        let look = extract_look_from_reference(&img, "Gray HSL Test");
        assert_eq!(look.hsl.hue, [0; 8], "grigio puro non deve popolare l'hue di nessuna banda");
        assert_eq!(look.hsl.sat, [0; 8], "grigio puro non deve popolare la saturazione di nessuna banda");
    }

    #[test]
    fn warm_reference_increases_color_temperature() {
        let warm = synthetic_image(|_, _| [200, 120, 60, 255]);
        let cool = synthetic_image(|_, _| [60, 120, 200, 255]);
        let warm_look = extract_look_from_reference(&warm, "Warm");
        let cool_look = extract_look_from_reference(&cool, "Cool");
        assert!(
            warm_look.white_balance.temp > cool_look.white_balance.temp,
            "warm={} cool={}", warm_look.white_balance.temp, cool_look.white_balance.temp
        );
    }

    #[test]
    fn mild_incidental_color_variation_does_not_produce_split_toning() {
        // Bug reale corretto in questo giro: prima della baseline
        // (`BASELINE_SPLIT_CHROMA`), anche una foto SENZA alcuna intenzione
        // di grading — solo la normale, lieve differenza di colore fra
        // ombra/cielo e luce diretta che ha qualunque scatto diurno —
        // produceva uno split toning non trascurabile, copiato per intero su
        // "Incolla impostazioni". Qui le ombre hanno solo un lievissimo cast
        // freddo (+5 sul blu) e le luci un lievissimo cast caldo (+5 sul
        // rosso): una variazione realistica ma non uno stile deliberato.
        let img = synthetic_image(|_, y| {
            if y < 20 {
                [15, 18, 23, 255] // ombra: cast blu appena percettibile
            } else if y < 44 {
                [140, 140, 140, 255]
            } else {
                [235, 232, 227, 255] // luce: cast caldo appena percettibile
            }
        });
        let look = extract_look_from_reference(&img, "Mild Cast Test");
        assert_eq!(look.split_toning.shadow_sat, 0, "cast lieve non deve produrre split toning nelle ombre, got {}", look.split_toning.shadow_sat);
        assert_eq!(look.split_toning.highlight_sat, 0, "cast lieve non deve produrre split toning nelle luci, got {}", look.split_toning.highlight_sat);
    }

    #[test]
    fn teal_and_orange_split_produces_distinct_shadow_and_highlight_hues() {
        // Ombre tendenti al teal (blu-verde), luci tendenti all'arancio: il classico
        // "look cinematografico" citato nei requisiti.
        let img = synthetic_image(|_, y| {
            if y < 32 {
                [20, 60, 70, 255] // ombre: teal
            } else {
                [220, 150, 90, 255] // luci: arancio
            }
        });
        let look = extract_look_from_reference(&img, "Teal & Orange");
        assert!(look.split_toning.shadow_sat > 0);
        assert!(look.split_toning.highlight_sat > 0);
        assert_ne!(look.split_toning.shadow_hue, look.split_toning.highlight_hue);
    }

    #[test]
    fn smooth_circular_bands_softens_a_steep_cliff_but_keeps_the_dominant_band_dominant() {
        // Bug reale scoperto in questo giro, segnalato dall'utente come
        // "glitch"/rumore sulla pelle dei sedili di una foto vera anche dopo
        // aver corretto la desaturazione globale: questi sono gli ESATTI
        // valori di `hsl.sat` misurati su quella foto (banda Viola=-3 seguita
        // da Magenta=42, un salto di 45 punti in appena 90° di tonalità — la
        // banda 7 è circolarmente adiacente sia alla 6 che alla 0).
        let raw = [17, -6, -9, -5, -1, -8, -3, 42];
        let smoothed = smooth_circular_bands(raw);

        let max_adjacent_diff = |values: &[i32; HUE_BANDS]| {
            (0..HUE_BANDS)
                .map(|i| (values[i] - values[(i + 1) % HUE_BANDS]).abs())
                .max()
                .unwrap()
        };
        let before = max_adjacent_diff(&raw);
        let after = max_adjacent_diff(&smoothed);
        assert_eq!(before, 45, "il salto originale su questa foto vera era di 45 punti");
        assert!(
            after < 30,
            "lo smoothing deve ridurre sensibilmente il salto massimo fra bande adiacenti: prima={before} dopo={after}"
        );
        // Non deve però appiattire tutto: la banda Magenta (indice 7) resta
        // la più satura anche dopo lo smoothing — l'intento stilistico
        // originale (questa banda È più satura delle altre nella foto
        // campione) va preservato, solo con un picco meno ripido.
        let max_idx = (0..HUE_BANDS).max_by_key(|&i| smoothed[i]).unwrap();
        assert_eq!(max_idx, 7, "la banda dominante non deve cambiare, solo appiattirsi: {smoothed:?}");
    }
}
