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
//! (`white_balance.temp`/`tint`) non viene applicato in valore assoluto —
//! richiederebbe un profilo colore della fotocamera (matrice o DCP) che questo
//! motore non ha ancora — mentre esposizione, tone curve, contrasto,
//! highlights/shadows, HSL per banda, split toning e vibrance/saturazione sono
//! applicati per intero, perché non dipendono da un profilo camera. Sharpening
//! e riduzione rumore restano pianificati per la Fase 3-4 della roadmap (§8).

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

/// Peso (0..1) di quanto un pixel di luminanza `luma` (0..1, spazio sRGB)
/// appartiene alla zona "ombre": pieno sotto 0.0, zero da 0.4 in su.
fn shadow_mask(luma: f32) -> f32 {
    (1.0 - luma * 2.5).clamp(0.0, 1.0)
}

/// Come [`shadow_mask`] ma per la zona "luci": zero sotto 0.6, pieno a 1.0.
fn highlight_mask(luma: f32) -> f32 {
    ((luma - 0.6) * 2.5).clamp(0.0, 1.0)
}

/// Applica un `HarmonicLook` ai pixel di `image`, restituendo una nuova
/// immagine della stessa dimensione. Ordine degli stage (docs/ARCHITECTURE.md
/// §3.2, sezione "Detail"/NR esclusa): esposizione -> highlights/shadows ->
/// tone curve -> contrasto -> HSL per banda + split toning -> vibrance/
/// saturazione globale.
pub fn render_preview_with_look(image: &DynamicImage, look: &HarmonicLook) -> DynamicImage {
    let rgba = image.to_rgba8();
    let (width, _height) = rgba.dimensions();
    let row_stride = 4 * width as usize;

    let exposure_mul = 2f32.powf(look.exposure_ev);
    let tone_curve_lut = build_tone_curve_lut(&look.tone_curve);
    let contrast_amount = 1.0 + (look.contrast as f32 / 100.0);
    let shadows_amount = look.shadows as f32 / 100.0;
    let highlights_amount = look.highlights as f32 / 100.0;
    let global_sat_mul = (1.0 + (look.saturation as f32 / 100.0) + (look.vibrance as f32 / 200.0)).max(0.0);

    let mut out = rgba.clone();
    out.par_chunks_mut(row_stride)
        .zip(rgba.par_chunks(row_stride))
        .for_each(|(out_row, in_row)| {
            for (out_px, in_px) in out_row.chunks_exact_mut(4).zip(in_row.chunks_exact(4)) {
                // Esposizione: guadagno scalare in spazio lineare.
                let mut linear = [
                    srgb_to_linear(in_px[0] as f32 / 255.0),
                    srgb_to_linear(in_px[1] as f32 / 255.0),
                    srgb_to_linear(in_px[2] as f32 / 255.0),
                ];
                for c in linear.iter_mut() {
                    *c = (*c * exposure_mul).clamp(0.0, 1.0);
                }

                let mut srgb = [
                    linear_to_srgb(linear[0]),
                    linear_to_srgb(linear[1]),
                    linear_to_srgb(linear[2]),
                ];

                // Highlights/shadows: lift mascherato per zona tonale (positivo
                // = schiarisce quella zona, come in Lightroom per le ombre;
                // per le luci il segno è invertito, "highlights" negativo =
                // recupero luci bruciate).
                let luma = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
                let s_mask = shadow_mask(luma);
                let h_mask = highlight_mask(luma);
                let lift = shadows_amount * s_mask * 0.25 + highlights_amount * h_mask * 0.25;
                for c in srgb.iter_mut() {
                    *c = (*c + lift).clamp(0.0, 1.0);
                }

                // Tone curve.
                for c in srgb.iter_mut() {
                    *c = sample_lut(&tone_curve_lut, *c);
                }

                // Contrasto attorno al pivot 0.5.
                for c in srgb.iter_mut() {
                    *c = ((*c - 0.5) * contrast_amount + 0.5).clamp(0.0, 1.0);
                }

                // HSL per banda + split toning + saturazione/vibrance globale.
                let mut hsl = rgb_to_hsl(srgb);
                let band = (((hsl[0] / 45.0) as usize) % HUE_BANDS).min(HUE_BANDS - 1);
                hsl[0] = (hsl[0] + look.hsl.hue[band] as f32).rem_euclid(360.0);
                hsl[1] = (hsl[1] * (1.0 + look.hsl.sat[band] as f32 / 100.0) * global_sat_mul).clamp(0.0, 1.0);
                hsl[2] = (hsl[2] + look.hsl.lum[band] as f32 / 200.0).clamp(0.0, 1.0);

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
    fn identity_tone_curve_lut_is_approximately_linear() {
        let look = HarmonicLook::default(); // punti (0,0)-(64,64)-(128,128)-(192,192)-(255,255)
        let lut = build_tone_curve_lut(&look.tone_curve);
        for i in (0..256).step_by(17) {
            let expected = i as f32 / 255.0;
            assert!((lut[i] - expected).abs() < 0.01, "i={i} lut={} expected={expected}", lut[i]);
        }
    }

    #[test]
    fn luminance_histogram_counts_every_pixel_exactly_once() {
        let img = solid_image(10, 7, [42, 200, 5]);
        let hist = luminance_histogram(&img);
        let total: u64 = hist.iter().map(|&c| c as u64).sum();
        assert_eq!(total, 70);
    }
}
