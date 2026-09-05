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

/// A quale regione applicare la maschera "Soggetto"/"Sfondo" (vedi
/// `SubjectMask`): `Subject` = dove la mappa di salienza è ALTA (il probabile
/// soggetto principale), `Background` = il complementare (dove la salienza è
/// BASSA). La mappa stessa (`harmonic::compute_saliency_map`) è la stessa
/// euristica esposta all'utente da `compute_subject_saliency_preview` — qui
/// diventa per la prima volta un vero input per il rendering, non solo
/// un'anteprima ispezionabile (vedi `look-render::apply_subject_mask` per
/// come viene sogliata/sfumata in un peso 0..1).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaskTarget {
    Subject,
    Background,
}

impl Default for MaskTarget {
    fn default() -> Self {
        MaskTarget::Subject
    }
}

/// Regolazione locale, ristretta a una maschera automatica derivata dalla
/// salienza (vedi `MaskTarget`) invece che all'intera foto. Deliberatamente
/// solo tre controlli (non l'intero set di un `HarmonicLook`): esposizione,
/// contrasto e saturazione sono i tre che più spesso servono per "staccare"
/// il soggetto dallo sfondo (es. scurire leggermente lo sfondo, o
/// desaturarlo) senza la complessità di un secondo Look completo — un set
/// più ampio è un'estensione naturale futura, non preclusa da questa scelta
/// (vedi `look-render::apply_subject_mask` per i limiti onesti dell'approccio
/// attuale: una sola maschera, nessun pennello manuale, nessun raffinamento
/// dei bordi oltre la sfumatura di soglia della salienza).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct SubjectMask {
    pub enabled: bool,
    pub target: MaskTarget,
    /// EV, come `HarmonicLook::exposure_ev` ma ristretto alla maschera.
    pub exposure_ev: f32,
    /// -100..100, come `HarmonicLook::contrast` ma ristretto alla maschera.
    pub contrast: i32,
    /// -100..100, come `HarmonicLook::saturation` ma ristretto alla maschera.
    pub saturation: i32,
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
    /// Maschera automatica "Soggetto"/"Sfondo" derivata dalla salienza
    /// (vedi `SubjectMask`): quando `enabled` è vero, `look-render` applica
    /// esposizione/contrasto/saturazione locali SOLO sulla regione scelta,
    /// oltre alle stesse regolazioni globali sopra.
    pub subject_mask: SubjectMask,
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
            subject_mask: SubjectMask::default(),
        }
    }
}
