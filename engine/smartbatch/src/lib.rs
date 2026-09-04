//! Smart-Batch Contestuale (docs/ARCHITECTURE.md, §4.2).
//!
//! Questo crate implementa la parte "algoritmica pura" (descrittori di scena +
//! calcolo dei delta adattivi). La classificazione di scena con modello on-device
//! (MobileNet/EfficientNet-Lite quantizzato) e il rilevamento volti restano
//! pianificati per la Fase 3 della roadmap: qui sotto trovi già, funzionante,
//! il percorso euristico basato su istogramma descritto nello stesso paragrafo,
//! che è anche la base necessaria su cui il classificatore verrà innestato.

use core_types::HarmonicLook;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneDescriptors {
    /// Luminanza media, 0.0 (nero) .. 1.0 (bianco).
    pub mean_luminance: f32,
    /// Deviazione standard della luminanza: proxy del dynamic range della scena.
    pub luminance_std: f32,
    /// Frazione di pixel con luminanza clippata in alto (>= 250/255).
    pub clipped_highlight_frac: f32,
    /// Frazione di pixel con luminanza schiacciata in basso (<= 5/255).
    pub crushed_shadow_frac: f32,
}

/// Calcola i descrittori di scena a partire da un istogramma di luminanza a 256 bin
/// (tipicamente ottenuto da una preview a bassa risoluzione, non dal RAW pieno).
pub fn compute_scene_descriptors(luminance_histogram: &[u32; 256]) -> SceneDescriptors {
    let total: u64 = luminance_histogram.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return SceneDescriptors::default();
    }

    let mut sum = 0f64;
    for (bin, &count) in luminance_histogram.iter().enumerate() {
        sum += bin as f64 * count as f64;
    }
    let mean_bin = sum / total as f64;

    let mut var_sum = 0f64;
    for (bin, &count) in luminance_histogram.iter().enumerate() {
        let d = bin as f64 - mean_bin;
        var_sum += d * d * count as f64;
    }
    let std_bin = (var_sum / total as f64).sqrt();

    let clipped: u64 = luminance_histogram[250..=255].iter().map(|&c| c as u64).sum();
    let crushed: u64 = luminance_histogram[0..=5].iter().map(|&c| c as u64).sum();

    SceneDescriptors {
        mean_luminance: (mean_bin / 255.0) as f32,
        luminance_std: (std_bin / 255.0) as f32,
        clipped_highlight_frac: clipped as f32 / total as f32,
        crushed_shadow_frac: crushed as f32 / total as f32,
    }
}

/// Coefficienti dell'algoritmo di adattamento (docs/ARCHITECTURE.md, §4.2, passo 2),
/// esposti come parametro in modo da poter essere collegati agli slider "avanzati"
/// della UI senza ricompilare il motore.
#[derive(Debug, Clone, Copy)]
pub struct AdaptationParams {
    pub k_exposure: f32,
    pub k_highlights: f32,
    pub k_shadows: f32,
    pub max_exposure_delta_ev: f32,
    pub max_tonal_delta: f32,
    /// Slider "Override Strength" della UI: 0.0 = applica il Look letterale,
    /// 1.0 = applica il massimo adattamento consentito dai guardrail sopra.
    pub override_strength: f32,
}

impl Default for AdaptationParams {
    fn default() -> Self {
        Self {
            k_exposure: 2.0,
            k_highlights: 60.0,
            k_shadows: 60.0,
            max_exposure_delta_ev: 0.5,
            max_tonal_delta: 15.0,
            override_strength: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AdaptiveDeltas {
    pub exposure_ev: f32,
    pub highlights: i32,
    pub shadows: i32,
}

/// Calcola i delta adattivi per una singola immagine rispetto ai descrittori del
/// Look di riferimento (immagine guida della Sintesi Armonica, o media del batch).
pub fn compute_adaptive_deltas(
    reference: &SceneDescriptors,
    image: &SceneDescriptors,
    params: &AdaptationParams,
) -> AdaptiveDeltas {
    let raw_exposure = params.k_exposure * (reference.mean_luminance - image.mean_luminance);
    let exposure_ev = raw_exposure.clamp(-params.max_exposure_delta_ev, params.max_exposure_delta_ev)
        * params.override_strength;

    let raw_highlights = -params.k_highlights * image.clipped_highlight_frac;
    let highlights = (raw_highlights.clamp(-params.max_tonal_delta, params.max_tonal_delta)
        * params.override_strength) as i32;

    let raw_shadows = params.k_shadows * image.crushed_shadow_frac;
    let shadows = (raw_shadows.clamp(-params.max_tonal_delta, params.max_tonal_delta)
        * params.override_strength) as i32;

    AdaptiveDeltas { exposure_ev, highlights, shadows }
}

/// Applica i delta calcolati a un `HarmonicLook` di base, producendo il Look
/// specifico per una singola immagine del batch (docs/ARCHITECTURE.md, §4.2, passo 3).
pub fn apply_deltas(base: &HarmonicLook, deltas: &AdaptiveDeltas) -> HarmonicLook {
    let mut look = base.clone();
    look.exposure_ev += deltas.exposure_ev;
    look.highlights = (look.highlights + deltas.highlights).clamp(-100, 100);
    look.shadows = (look.shadows + deltas.shadows).clamp(-100, 100);
    look
}

#[cfg(test)]
mod tests {
    use super::*;

    fn histogram_with_peak(peak_bin: usize, spread: usize, total_samples: u32) -> [u32; 256] {
        let mut hist = [0u32; 256];
        let lo = peak_bin.saturating_sub(spread);
        let hi = (peak_bin + spread).min(255);
        let bins = (hi - lo + 1) as u32;
        let per_bin = total_samples / bins;
        for b in lo..=hi {
            hist[b] = per_bin;
        }
        hist
    }

    #[test]
    fn empty_histogram_yields_default_descriptors() {
        let hist = [0u32; 256];
        let d = compute_scene_descriptors(&hist);
        assert_eq!(d, SceneDescriptors::default());
    }

    #[test]
    fn bright_backlit_scene_has_high_clipped_fraction() {
        // Istogramma con un grosso picco vicino al bianco: simula un controluce.
        let hist = histogram_with_peak(253, 2, 10_000);
        let d = compute_scene_descriptors(&hist);
        assert!(d.clipped_highlight_frac > 0.5, "atteso alto clipping, got {}", d.clipped_highlight_frac);
        assert!(d.mean_luminance > 0.9);
    }

    #[test]
    fn dark_scene_gets_positive_exposure_and_shadow_delta() {
        let reference = SceneDescriptors { mean_luminance: 0.55, luminance_std: 0.2, ..Default::default() };
        let dark_image = SceneDescriptors {
            mean_luminance: 0.15,
            luminance_std: 0.1,
            crushed_shadow_frac: 0.4,
            clipped_highlight_frac: 0.0,
        };
        let params = AdaptationParams::default();
        let deltas = compute_adaptive_deltas(&reference, &dark_image, &params);

        assert!(deltas.exposure_ev > 0.0, "un'immagine più scura del target deve ricevere +EV");
        assert!(deltas.shadows > 0, "ombre schiacciate devono ricevere recovery positivo");
        assert!(deltas.exposure_ev <= params.max_exposure_delta_ev, "guardrail exposure violato");
    }

    #[test]
    fn override_strength_zero_disables_adaptation() {
        let reference = SceneDescriptors { mean_luminance: 0.8, ..Default::default() };
        let image = SceneDescriptors { mean_luminance: 0.1, crushed_shadow_frac: 0.9, ..Default::default() };
        let params = AdaptationParams { override_strength: 0.0, ..Default::default() };
        let deltas = compute_adaptive_deltas(&reference, &image, &params);
        assert_eq!(deltas, AdaptiveDeltas::default());
    }

    #[test]
    fn apply_deltas_respects_clamping() {
        let mut base = HarmonicLook::default();
        base.highlights = 95;
        let deltas = AdaptiveDeltas { exposure_ev: 0.2, highlights: 20, shadows: 0 };
        let result = apply_deltas(&base, &deltas);
        assert_eq!(result.highlights, 100, "highlights non deve superare 100");
    }
}
