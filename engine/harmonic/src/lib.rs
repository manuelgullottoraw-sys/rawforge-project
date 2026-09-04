//! Sintesi Armonica Automatica (docs/ARCHITECTURE.md, §4.1).
//!
//! Estrae da un'immagine di riferimento (look cinematografico o foto guida) un
//! `HarmonicLook`: tone curve, palette per zona tonale (-> split toning), stima
//! del bilanciamento del bianco e del contrasto. Le formule di normalizzazione
//! (baseline di contrasto/crominanza, mappatura EV) sono euristiche calibrabili
//! empiricamente, non misure fotometriche assolute — sono commentate come tali
//! ovunque compaiono, così è chiaro cosa va tarato con un vero corpus di foto.

use color_science::{lab_ab_to_hue_chroma, linear_rgb_to_lab, srgb_to_linear};
use core_types::{HarmonicLook, SplitToning};
use image::DynamicImage;

const ANALYSIS_MAX_DIM: u32 = 512;

/// Luminanza Lab "tipica" per un grigio medio (18%), usata come pivot per la
/// stima (euristica) dell'esposizione relativa.
const NEUTRAL_L: f32 = 50.0;

/// Deviazione standard di L attesa per un'immagine "normocontrastata": baseline
/// empirica da ricalibrare su un corpus reale.
const BASELINE_CONTRAST_STD: f32 = 20.0;

/// Chroma Lab media attesa per una scena fotografica "normale": baseline
/// empirica per il bias di vibrance/saturation.
const BASELINE_CHROMA: f32 = 18.0;

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

    for px in rgba.pixels() {
        let lin = [
            srgb_to_linear(px[0] as f32 / 255.0),
            srgb_to_linear(px[1] as f32 / 255.0),
            srgb_to_linear(px[2] as f32 / 255.0),
        ];
        sum_lin[0] += lin[0] as f64;
        sum_lin[1] += lin[1] as f64;
        sum_lin[2] += lin[2] as f64;

        let lab = linear_rgb_to_lab(lin);
        l_values.push(lab[0]);
        ab_values.push((lab[1], lab[2]));

        let (_, chroma) = lab_ab_to_hue_chroma(lab[1], lab[2]);
        sum_chroma += chroma as f64;
    }

    let n = l_values.len().max(1) as f64;

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

    let mut look = HarmonicLook {
        name: name.to_string(),
        exposure_ev,
        contrast,
        vibrance,
        split_toning,
        tone_curve,
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
