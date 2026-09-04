//! Conversioni di spazio colore usate dalla pipeline (docs/ARCHITECTURE.md, §3.2)
//! e dal motore di Sintesi Armonica (§4.1). Implementazione scalare, corretta e
//! senza early-return nei loop interni: pensata per essere facilmente
//! auto-vettorizzata dal compilatore o riscritta con intrinsics SIMD espliciti
//! (`pulp`) senza cambiarne la logica.

/// sRGB (0..1) -> lineare (0..1), standard IEC 61966-2-1.
#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Lineare (0..1) -> sRGB (0..1).
#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// RGB lineare (D65) -> Lab. L in 0..100, a/b tipicamente in -128..127.
pub fn linear_rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    // Matrice sRGB lineare -> XYZ (D65)
    let x = 0.4124564 * rgb[0] + 0.3575761 * rgb[1] + 0.1804375 * rgb[2];
    let y = 0.2126729 * rgb[0] + 0.7151522 * rgb[1] + 0.0721750 * rgb[2];
    let z = 0.0193339 * rgb[0] + 0.1191920 * rgb[1] + 0.9503041 * rgb[2];

    // Whitepoint D65
    const XN: f32 = 0.95047;
    const YN: f32 = 1.0;
    const ZN: f32 = 1.08883;

    let f = |t: f32| -> f32 {
        const DELTA: f32 = 6.0 / 29.0;
        if t > DELTA.powi(3) {
            t.cbrt()
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };

    let fx = f(x / XN);
    let fy = f(y / YN);
    let fz = f(z / ZN);

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);

    [l, a, b]
}

/// Inversa di [`linear_rgb_to_lab`].
pub fn lab_to_linear_rgb(lab: [f32; 3]) -> [f32; 3] {
    const XN: f32 = 0.95047;
    const YN: f32 = 1.0;
    const ZN: f32 = 1.08883;

    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;

    let f_inv = |t: f32| -> f32 {
        const DELTA: f32 = 6.0 / 29.0;
        if t > DELTA {
            t.powi(3)
        } else {
            3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
        }
    };

    let x = XN * f_inv(fx);
    let y = YN * f_inv(fy);
    let z = ZN * f_inv(fz);

    let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let b = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;

    [r, g, b]
}

/// Hue (0..360) e chroma (distanza dall'asse acromatico) a partire da a*/b* Lab.
pub fn lab_ab_to_hue_chroma(a: f32, b: f32) -> (f32, f32) {
    let mut hue = b.atan2(a).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }
    let chroma = (a * a + b * b).sqrt();
    (hue, chroma)
}

/// RGB (0..1) -> HSL. H in 0..360, S/L in 0..1.
pub fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let l = (maxc + minc) * 0.5;
    let d = maxc - minc;

    if d <= 1e-6 {
        return [0.0, 0.0, l];
    }

    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let mut h = if maxc == r {
        ((g - b) / d) % 6.0
    } else if maxc == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    if h < 0.0 {
        h += 360.0;
    }

    [h, s, l]
}

/// Inversa di [`rgb_to_hsl`].
pub fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let (h, s, l) = (hsl[0], hsl[1], hsl[2]);
    if s <= 1e-6 {
        return [l, l, l];
    }
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c * 0.5;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    [r1 + m, g1 + m, b1 + m]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn srgb_linear_round_trip() {
        for i in 0..=10 {
            let c = i as f32 / 10.0;
            let round_tripped = linear_to_srgb(srgb_to_linear(c));
            assert!(approx_eq(c, round_tripped, 1e-4), "c={c} got={round_tripped}");
        }
    }

    #[test]
    fn white_is_l100_a0_b0() {
        let lab = linear_rgb_to_lab([1.0, 1.0, 1.0]);
        assert!(approx_eq(lab[0], 100.0, 0.1));
        assert!(approx_eq(lab[1], 0.0, 0.1));
        assert!(approx_eq(lab[2], 0.0, 0.1));
    }

    #[test]
    fn black_is_l0() {
        let lab = linear_rgb_to_lab([0.0, 0.0, 0.0]);
        assert!(approx_eq(lab[0], 0.0, 0.1));
    }

    #[test]
    fn lab_round_trip() {
        let original = [0.6, 0.2, 0.4];
        let lab = linear_rgb_to_lab(original);
        let back = lab_to_linear_rgb(lab);
        for i in 0..3 {
            assert!(approx_eq(original[i], back[i], 1e-3), "channel {i}: {} vs {}", original[i], back[i]);
        }
    }

    #[test]
    fn hsl_round_trip() {
        let original = [0.8, 0.3, 0.1];
        let hsl = rgb_to_hsl(original);
        let back = hsl_to_rgb(hsl);
        for i in 0..3 {
            assert!(approx_eq(original[i], back[i], 1e-3), "channel {i}: {} vs {}", original[i], back[i]);
        }
    }

    #[test]
    fn gray_has_zero_saturation() {
        let hsl = rgb_to_hsl([0.5, 0.5, 0.5]);
        assert!(approx_eq(hsl[1], 0.0, 1e-6));
    }
}
