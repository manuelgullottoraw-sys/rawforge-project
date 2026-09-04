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
/// Saturazione HSL "tipica" attesa per una scena fotografica media (scala
/// 0..1, NON la chroma Lab di [`BASELINE_CHROMA`] — spazi colore diversi):
/// baseline empirica per il bias di saturazione per banda, da ricalibrare su
/// un corpus reale come le altre baseline di questo file.
const BASELINE_HSL_SATURATION: f32 = 0.35;

/// Luminanza Lab "tipica" per un grigio medio (18%), usata come pivot per la
/// stima (euristica) dell'esposizione relativa.
const NEUTRAL_L: f32 = 50.0;

/// Deviazione standard di L attesa per un'immagine "normocontrastata": baseline
/// empirica da ricalibrare su un corpus reale.
const BASELINE_CONTRAST_STD: f32 = 20.0;

/// Chroma Lab media attesa per una scena fotografica "normale": baseline
/// empirica per il bias di vibrance/saturation.
const BASELINE_CHROMA: f32 = 18.0;

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
        shadow_sat: shadow_chroma.clamp(0.0, 100.0).round() as i32,
        highlight_hue: highlight_hue.round() as i32,
        highlight_sat: highlight_chroma.clamp(0.0, 100.0).round() as i32,
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

        // Range stretto (+-50, non +-100): **bug reale scoperto e corretto in
        // questo giro**, causa della dominante magenta/viola diffusa segnalata
        // dall'utente su un rendering "incolla impostazioni". `BASELINE_HSL_
        // SATURATION` (0.35) è una stima di quanto sia "tipicamente satura" UNA
        // banda di tonalità in una foto qualunque — ma nella grande maggioranza
        // delle foto reali quasi tutte le 8 bande hanno una saturazione media
        // MOLTO più bassa di 0.35 (gran parte della scena è pavimentazione,
        // pelle, cielo, grigi quasi neutri con solo una leggerissima e
        // involontaria dominante di colore), quindi quasi ogni banda finiva
        // clampata al -100 estremo ("azzera del tutto questa tonalità"), mentre
        // la sola banda che per caso intercettava un'area davvero satura (anche
        // solo per una classificazione di hue vicina al confine, es. un rosso
        // con una lieve componente blu che cade nella banda "Magenta" invece di
        // "Rosso") finiva clampata al +100 opposto ("raddoppia questa
        // tonalità"). Il risultato, con l'interpolazione circolare fra bande
        // adiacenti (vedi `look-render::interpolate_hsl_band`), è che un'ampia
        // porzione della ruota dei colori finiva o quasi completamente
        // desaturata o vistosamente amplificata verso quell'unica tonalità
        // "vincente" — non uno stile estratto dalla foto campione, un
        // artefatto della formula. Dimezzare il range (+-50, moltiplicatore
        // 0.5x-1.5x invece di 0x-2x) tiene il segnale (quali tonalità la foto
        // campione enfatizza/desatura rispetto alle altre) senza più poter
        // azzerare o raddoppiare un'intera banda da solo. Non tocca il range
        // dello slider MANUALE nel pannello Develop (-100..100, in
        // `look-render`): lì è una scelta deliberata dell'utente, non
        // un'estrazione automatica da guardrail.
        hsl_sat[band] = (((band_mean_sat - BASELINE_HSL_SATURATION) / BASELINE_HSL_SATURATION) * 100.0)
            .clamp(-50.0, 50.0) as i32;
        // Range stretto (+-30): un ritocco di luminanza per banda, non una
        // riesposizione mascherata per colore.
        hsl_lum[band] = ((band_mean_lum - overall_hsl_lum) * 200.0).clamp(-30.0, 30.0) as i32;
        // Range ancora più stretto (+-15 gradi): uno scostamento di hue è il
        // ritocco HSL più visibile e rischioso, va tenuto sottile.
        hsl_hue[band] = (band_mean_hue - band_center_hue).clamp(-15.0, 15.0) as i32;
    }

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
        // Foto quasi interamente rossa satura, con un piccolo angolo neutro
        // (altrimenti nessuna banda avrebbe l'hue definito quando l'immagine
        // è un unico colore piatto... in realtà anche un solo colore basta,
        // ma un angolo neutro verifica anche che i pixel acromatici vengano
        // esclusi correttamente dal calcolo). Rosso puro (255,0,0) sRGB cade
        // in banda 0 (hue 0).
        let img = synthetic_image(|x, y| {
            if x < 4 && y < 4 {
                [128, 128, 128, 255] // angolo neutro: escluso dalle bande
            } else {
                [230, 20, 20, 255] // rosso molto saturo: banda 0
            }
        });
        let look = extract_look_from_reference(&img, "Red Test");
        assert!(look.hsl.sat[0] > 0, "banda rossa (0) deve avere un bias di saturazione positivo, got {}", look.hsl.sat[0]);
        // Una banda senza abbastanza pixel (es. la 4, Aqua) resta a zero: non
        // deve inventare uno stile per un colore assente dalla foto.
        assert_eq!(look.hsl.sat[4], 0, "banda assente dalla foto non deve avere bias, got {}", look.hsl.sat[4]);
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
}
