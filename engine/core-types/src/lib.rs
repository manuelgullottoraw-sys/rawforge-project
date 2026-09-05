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
    /// Texture per banda di frequenza (-100..100, 0 = nessun effetto):
    /// separazione di frequenza gaussiana in tre bande (fine/media/grossa),
    /// vedi `look-render` per l'implementazione. A differenza di "Chiarezza"
    /// (non ancora esposta) questi controlli non toccano la luminosità media
    /// dell'immagine, solo l'ampiezza del dettaglio locale a quella scala.
    pub texture_fine: i32,
    pub texture_medium: i32,
    pub texture_coarse: i32,
    /// Seconda zona di bilanciamento del bianco per il "bilanciamento del
    /// bianco a gradiente": quando `wb_gradient_enabled` è vero, il motore
    /// sfuma linearmente tra `white_balance` (zona A) e `white_balance_b`
    /// (zona B) lungo l'asse scelto — utile per cieli freddi/terreno caldo
    /// nello stesso scatto, un caso che un singolo WB globale non può
    /// risolvere.
    pub white_balance_b: WhiteBalance,
    pub wb_gradient_enabled: bool,
    /// true = il gradiente va dall'alto (zona A) al basso (zona B); false =
    /// da sinistra (zona A) a destra (zona B).
    pub wb_gradient_vertical: bool,
    /// Posizione del centro della transizione lungo l'asse, 0..100 (percentuale).
    pub wb_gradient_position: i32,
    /// Ampiezza della transizione, 0..100: 0 = bordo netto, 100 = sfumatura
    /// distribuita sull'intero fotogramma.
    pub wb_gradient_spread: i32,
    /// Riduzione del rumore di LUMINANZA (0..100, 0 = nessun effetto): sfoca
    /// il canale L (Lab) nelle zone piatte, protetta ai bordi (vedi
    /// `look-render::apply_noise_reduction`) — corrisponde al concetto di
    /// "Luminance" nel pannello Dettaglio di Lightroom/ACR.
    pub noise_reduction_luma: i32,
    /// Riduzione del rumore CROMATICO (0..100, 0 = nessun effetto): sfoca i
    /// canali a*/b* (Lab), anch'essa protetta ai bordi per evitare aloni di
    /// colore — corrisponde al concetto di "Color" nel pannello Dettaglio di
    /// Lightroom/ACR. Il rumore cromatico è percettivamente meno legato al
    /// dettaglio della scena rispetto a quello di luminanza, quindi in
    /// pratica tollera un raggio di sfocatura maggiore prima di risultare
    /// visibile come perdita di nitidezza.
    pub noise_reduction_color: i32,
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
            texture_fine: 0,
            texture_medium: 0,
            texture_coarse: 0,
            white_balance_b: WhiteBalance { temp: 5500, tint: 0 },
            wb_gradient_enabled: false,
            wb_gradient_vertical: true,
            wb_gradient_position: 50,
            wb_gradient_spread: 50,
            noise_reduction_luma: 0,
            noise_reduction_color: 0,
        }
    }
}
