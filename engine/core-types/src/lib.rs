//! Strutture dati condivise da più crate del motore RawForge.
//! Vedi `docs/ARCHITECTURE.md`, sezione 5.1, per il contesto.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct WhiteBalance {
    pub temp: u32,
    pub tint: i32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct HslAdjustments {
    /// Ordine bande: Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta
    pub hue: [i32; 8],
    pub sat: [i32; 8],
    pub lum: [i32; 8],
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct SplitToning {
    pub shadow_hue: i32,
    pub shadow_sat: i32,
    pub highlight_hue: i32,
    pub highlight_sat: i32,
    pub balance: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarmonicLook {
    pub name: String,
    pub process_version: String,
    pub white_balance: WhiteBalance,
    pub exposure_ev: f32,
    pub contrast: i32,
    pub highlights: i32,
    pub shadows: i32,
    pub whites: i32,
    pub blacks: i32,
    pub vibrance: i32,
    pub saturation: i32,
    pub tone_curve: Vec<(u8, u8)>,
    pub hsl: HslAdjustments,
    pub split_toning: SplitToning,
}

impl Default for HarmonicLook {
    fn default() -> Self {
        Self {
            name: "RawForge Look".to_string(),
            process_version: "15.4".to_string(),
            white_balance: WhiteBalance { temp: 5500, tint: 0 },
            exposure_ev: 0.0,
            contrast: 0,
            highlights: 0,
            shadows: 0,
            whites: 0,
            blacks: 0,
            vibrance: 0,
            saturation: 0,
            tone_curve: vec![(0, 0), (64, 64), (128, 128), (192, 192), (255, 255)],
            hsl: HslAdjustments::default(),
            split_toning: SplitToning::default(),
        }
    }
}
