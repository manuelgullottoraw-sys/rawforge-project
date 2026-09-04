//! Superficie pubblica del motore RawForge, esposta a Kotlin (Android + Windows
//! Desktop) tramite UniFFI. Vedi `docs/ARCHITECTURE.md`, §1 e §7: questo è il
//! crate che chiude il cerchio tra il motore Rust (già reale e testato negli
//! altri crate del workspace) e la UI Kotlin Multiplatform.
//!
//! Scelta deliberata: la superficie esposta oggi è minima (versione del motore,
//! Sintesi Armonica da una foto di riferimento, export XMP) — quanto basta per
//! dimostrare l'intera catena Rust -> UniFFI -> Kotlin -> APK/EXE end-to-end.
//! Il resto della pipeline (decodifica RAW via LibRaw, GPU pipe, Smart-Batch
//! collegato alla UI) si aggiunge alla stessa superficie senza cambiarne
//! l'architettura.

uniffi::setup_scaffolding!();

/// Un punto di controllo della tone curve (0..255 in entrambi gli assi).
/// UniFFI non supporta tuple anonime nei tipi esposti: questo record sostituisce
/// la tupla `(u8, u8)` usata internamente da `core_types::HarmonicLook`.
#[derive(uniffi::Record, Clone, Debug)]
pub struct TonePointFfi {
    pub x: u8,
    pub y: u8,
}

/// Versione "piatta" (solo tipi primitivi/record, compatibile UniFFI) di
/// `core_types::HarmonicLook`.
///
/// NB (fedeltà dei dati): questo struct portava originariamente solo 9 dei
/// campi di `HarmonicLook` — un sottoinsieme scelto per la prima demo minima.
/// Da questo incremento in poi porta *tutti* i campi, perché sia l'export XMP
/// sia il nuovo rendering "incolla impostazioni" (vedi
/// `paste_look_onto_target_photo` più sotto) dipendono da highlights/shadows/
/// tone_curve/HSL per essere fedeli a quanto estratto dalla Sintesi Armonica —
/// prima di questo cambio venivano silenziosamente azzerati nel passaggio
/// Kotlin -> Rust -> Kotlin (bug di fedeltà pre-esistente, corretto qui).
#[derive(uniffi::Record, Clone, Debug)]
pub struct HarmonicLookFfi {
    pub name: String,
    pub white_balance_temp: u32,
    pub white_balance_tint: i32,
    pub exposure_ev: f32,
    pub contrast: i32,
    pub highlights: i32,
    pub shadows: i32,
    pub whites: i32,
    pub blacks: i32,
    pub vibrance: i32,
    pub saturation: i32,
    pub tone_curve: Vec<TonePointFfi>,
    /// Ordine bande, per tutti e tre i seguenti: Red, Orange, Yellow, Green,
    /// Aqua, Blue, Purple, Magenta (8 elementi).
    pub hsl_hue: Vec<i32>,
    pub hsl_sat: Vec<i32>,
    pub hsl_lum: Vec<i32>,
    pub shadow_hue: i32,
    pub shadow_sat: i32,
    pub highlight_hue: i32,
    pub highlight_sat: i32,
    pub split_toning_balance: i32,
}

/// Adatta un `Vec<i32>` di lunghezza arbitraria a un array fisso di 8 elementi
/// (bande HSL), riempiendo con 0 se più corto — protegge da un input Kotlin
/// malformato senza dover restituire un errore per un dettaglio così minore.
fn hsl_band_array(values: &[i32]) -> [i32; 8] {
    let mut out = [0i32; 8];
    for (slot, value) in out.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    out
}

impl From<core_types::HarmonicLook> for HarmonicLookFfi {
    fn from(look: core_types::HarmonicLook) -> Self {
        Self {
            name: look.name,
            white_balance_temp: look.white_balance.temp,
            white_balance_tint: look.white_balance.tint,
            exposure_ev: look.exposure_ev,
            contrast: look.contrast,
            highlights: look.highlights,
            shadows: look.shadows,
            whites: look.whites,
            blacks: look.blacks,
            vibrance: look.vibrance,
            saturation: look.saturation,
            tone_curve: look
                .tone_curve
                .into_iter()
                .map(|(x, y)| TonePointFfi { x, y })
                .collect(),
            hsl_hue: look.hsl.hue.to_vec(),
            hsl_sat: look.hsl.sat.to_vec(),
            hsl_lum: look.hsl.lum.to_vec(),
            shadow_hue: look.split_toning.shadow_hue,
            shadow_sat: look.split_toning.shadow_sat,
            highlight_hue: look.split_toning.highlight_hue,
            highlight_sat: look.split_toning.highlight_sat,
            split_toning_balance: look.split_toning.balance,
        }
    }
}

impl From<HarmonicLookFfi> for core_types::HarmonicLook {
    fn from(look: HarmonicLookFfi) -> Self {
        core_types::HarmonicLook {
            name: look.name,
            process_version: core_types::HarmonicLook::default().process_version,
            white_balance: core_types::WhiteBalance {
                temp: look.white_balance_temp,
                tint: look.white_balance_tint,
            },
            exposure_ev: look.exposure_ev,
            contrast: look.contrast,
            highlights: look.highlights,
            shadows: look.shadows,
            whites: look.whites,
            blacks: look.blacks,
            vibrance: look.vibrance,
            saturation: look.saturation,
            tone_curve: look.tone_curve.into_iter().map(|p| (p.x, p.y)).collect(),
            hsl: core_types::HslAdjustments {
                hue: hsl_band_array(&look.hsl_hue),
                sat: hsl_band_array(&look.hsl_sat),
                lum: hsl_band_array(&look.hsl_lum),
            },
            split_toning: core_types::SplitToning {
                shadow_hue: look.shadow_hue,
                shadow_sat: look.shadow_sat,
                highlight_hue: look.highlight_hue,
                highlight_sat: look.highlight_sat,
                balance: look.split_toning_balance,
            },
        }
    }
}

#[derive(uniffi::Error, thiserror::Error, Debug)]
pub enum EngineError {
    // NB: il campo non può chiamarsi "message" — UniFFI genera già una proprietà
    // `message` sulla sottoclasse Kotlin di Exception (presa dalla stringa di
    // Display di thiserror), e un campo dati con lo stesso nome produce due
    // dichiarazioni in conflitto nel Kotlin generato (errore reale osservato in
    // CI: "Conflicting declarations: public open val message / public final val
    // message"). Soluzione: rinominare il campo, qui "reason".
    #[error("impossibile decodificare l'immagine di riferimento: {reason}")]
    DecodeError { reason: String },

    /// Errore nella decodifica di un file RAW vero (crate `raw-decode`, che
    /// avvolge `rawler`) — formato non riconosciuto, file corrotto, o nessuna
    /// anteprima incorporata disponibile.
    #[error("impossibile leggere il file RAW: {reason}")]
    RawFileError { reason: String },
}

/// Stringa di stato del motore — usata dalla UI (pulsante "Stato motore") per
/// confermare che il collegamento Rust -> Kotlin funziona davvero.
#[uniffi::export]
pub fn engine_version() -> String {
    format!(
        "RawForge Core v{} — motore Rust collegato via UniFFI",
        env!("CARGO_PKG_VERSION")
    )
}

/// Sintesi Armonica Automatica (docs/ARCHITECTURE.md, §4.1): analizza i byte di
/// un'immagine di riferimento (JPEG o PNG, come già decodificata dalla UI o
/// selezionata dall'utente) e restituisce il Look estratto.
#[uniffi::export]
pub fn extract_look_from_reference_image(
    reference_image_bytes: Vec<u8>,
    look_name: String,
) -> Result<HarmonicLookFfi, EngineError> {
    let img = image::load_from_memory(&reference_image_bytes)
        .map_err(|e| EngineError::DecodeError { reason: e.to_string() })?;
    let look = harmonic::extract_look_from_reference(&img, &look_name);
    Ok(look.into())
}

/// Esporta un `HarmonicLookFfi` come preset Lightroom `.xmp` (docs/ARCHITECTURE.md, §5).
#[uniffi::export]
pub fn generate_lightroom_preset_xmp(look: HarmonicLookFfi) -> String {
    let full_look: core_types::HarmonicLook = look.into();
    xmp::generate_lightroom_xmp(&full_look)
}

/// Esito della decodifica "veloce" (nessun demosaic) di un file RAW vero: i
/// metadati base della fotocamera più l'anteprima incorporata, già ri-codificata
/// come PNG in modo che la UI Kotlin possa decodificarla con qualunque
/// libreria immagine standard, senza legare la UI al tipo `DynamicImage` di Rust.
#[derive(uniffi::Record, Clone, Debug)]
pub struct RawPreviewFfi {
    pub camera_make: String,
    pub camera_model: String,
    pub preview_png_bytes: Vec<u8>,
}

fn encode_preview_as_png(image: &image::DynamicImage) -> Result<Vec<u8>, EngineError> {
    let mut png_bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| EngineError::RawFileError {
            reason: e.to_string(),
        })?;
    Ok(png_bytes)
}

/// Decodifica un file RAW vero (bytes in memoria — niente path di filesystem,
/// per funzionare sia da file picker Desktop sia da content:// URI Android) ed
/// estrae l'anteprima incorporata dalla fotocamera più i metadati base.
/// Vedi `raw-decode/src/lib.rs` per cosa fa e cosa NON fa ancora (demosaic
/// completo escluso, primo incremento deliberatamente limitato all'anteprima).
#[uniffi::export]
pub fn decode_raw_file_preview(raw_bytes: Vec<u8>) -> Result<RawPreviewFfi, EngineError> {
    let preview = raw_decode::decode_raw_preview(&raw_bytes).map_err(|e| EngineError::RawFileError {
        reason: e.to_string(),
    })?;
    let preview_png_bytes = encode_preview_as_png(&preview.image)?;
    Ok(RawPreviewFfi {
        camera_make: preview.info.camera_make,
        camera_model: preview.info.camera_model,
        preview_png_bytes,
    })
}

/// Come `extract_look_from_reference_image`, ma partendo direttamente da un
/// file RAW vero invece che da un JPEG/PNG già sviluppato: usa l'anteprima
/// incorporata (via `raw-decode`) come immagine di riferimento per la Sintesi
/// Armonica, senza un giro a vuoto di ri-codifica.
#[uniffi::export]
pub fn extract_look_from_raw_reference(
    raw_bytes: Vec<u8>,
    look_name: String,
) -> Result<HarmonicLookFfi, EngineError> {
    let preview = raw_decode::decode_raw_preview(&raw_bytes).map_err(|e| EngineError::RawFileError {
        reason: e.to_string(),
    })?;
    let look = harmonic::extract_look_from_reference(&preview.image, &look_name);
    Ok(look.into())
}

/// Filtro rapido "è un file RAW noto?" da un nome file, usato dalla UI per
/// popolare la grid d'importazione senza tentare di decodificare ogni file.
#[uniffi::export]
pub fn is_known_raw_file_name(file_name: String) -> bool {
    raw_decode::has_known_raw_extension(&file_name)
}

/// Decodifica una foto qualunque (RAW vera o già sviluppata) in una
/// `DynamicImage` pronta per l'analisi/rendering — stessa logica di
/// riconoscimento RAW-vs-sviluppata usata da `Engine.importPhoto` lato Kotlin,
/// centralizzata qui perché sia l'estrazione del Look sia il nuovo rendering
/// ne hanno bisogno.
fn decode_any_photo(bytes: &[u8], file_name: &str) -> Result<image::DynamicImage, EngineError> {
    if raw_decode::has_known_raw_extension(file_name) {
        let preview = raw_decode::decode_raw_preview(bytes).map_err(|e| EngineError::RawFileError {
            reason: e.to_string(),
        })?;
        Ok(preview.image)
    } else {
        image::load_from_memory(bytes).map_err(|e| EngineError::DecodeError {
            reason: e.to_string(),
        })
    }
}

/// Esito di "incolla impostazioni": l'anteprima della foto target già
/// renderizzata con il Look adattato, più i valori di esposizione/highlights/
/// shadows effettivamente applicati (dopo l'adattamento intelligente) — utili
/// alla UI per mostrare "cosa ha deciso" lo Smart-Batch, non solo il risultato.
#[derive(uniffi::Record, Clone, Debug)]
pub struct AdaptedRenderFfi {
    pub rendered_preview_png_bytes: Vec<u8>,
    pub applied_exposure_ev: f32,
    pub applied_highlights: i32,
    pub applied_shadows: i32,
}

/// Incolla sulla foto da modificare (`target`) le impostazioni copiate dalla
/// foto campione, adattandole in modo intelligente alla scena specifica del
/// target invece di applicarle identiche — è lo Smart-Batch Contestuale
/// (docs/ARCHITECTURE.md, §4.2). In un solo passaggio: estrae il Look dalla
/// foto campione (Sintesi Armonica, §4.1), calcola i descrittori di scena di
/// campione e target dai rispettivi istogrammi di luminanza, i delta adattivi
/// (esposizione, recupero luci/ombre, con i guardrail dell'architettura), li
/// applica al Look di base e renderizza subito l'anteprima risultante — tutto
/// resta nell'app. Prende solo bytes/stringhe primitive (non un
/// `HarmonicLookFfi`) apposta: così la UI Kotlin comune (`commonMain`) può
/// richiamarlo senza dover far attraversare il confine `expect`/`actual` a un
/// tipo generato da UniFFI, che esiste solo nelle copie platform-specific dei
/// binding (vedi `shared/src/commonMain/kotlin/com/rawforge/shared/Engine.kt`).
///
/// Per esportare lo stesso Look anche come preset `.xmp`, la UI richiama
/// separatamente `extract_look_from_reference_image`/`extract_look_from_raw_
/// reference` + `generate_lightroom_preset_xmp` sulla foto campione: è una
/// piccola ri-analisi aggiuntiva (la Sintesi Armonica su una preview costa
/// <50ms, §4.1 punto 1), non un giro a vuoto.
///
/// `override_strength` (0.0..1.0) è lo slider "Override Strength" della UI:
/// 0.0 = applica il Look letterale (nessun adattamento), 1.0 = applica il
/// massimo adattamento consentito dai guardrail. Vedi `smartbatch` per i
/// dettagli dell'algoritmo, già testato indipendentemente da questo crate.
#[uniffi::export]
pub fn paste_look_onto_target_photo(
    sample_bytes: Vec<u8>,
    sample_file_name: String,
    look_name: String,
    target_bytes: Vec<u8>,
    target_file_name: String,
    override_strength: f32,
) -> Result<AdaptedRenderFfi, EngineError> {
    let sample_image = decode_any_photo(&sample_bytes, &sample_file_name)?;
    let target_image = decode_any_photo(&target_bytes, &target_file_name)?;

    let base_look = harmonic::extract_look_from_reference(&sample_image, &look_name);

    let sample_descriptors = smartbatch::compute_scene_descriptors(&look_render::luminance_histogram(&sample_image));
    let target_descriptors = smartbatch::compute_scene_descriptors(&look_render::luminance_histogram(&target_image));

    let params = smartbatch::AdaptationParams {
        override_strength: override_strength.clamp(0.0, 1.0),
        ..smartbatch::AdaptationParams::default()
    };
    let deltas = smartbatch::compute_adaptive_deltas(&sample_descriptors, &target_descriptors, &params);
    let adapted_look = smartbatch::apply_deltas(&base_look, &deltas);

    let rendered = look_render::render_preview_with_look(&target_image, &adapted_look);
    let rendered_preview_png_bytes = encode_preview_as_png(&rendered)?;

    Ok(AdaptedRenderFfi {
        rendered_preview_png_bytes,
        applied_exposure_ev: adapted_look.exposure_ev,
        applied_highlights: adapted_look.highlights,
        applied_shadows: adapted_look.shadows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_version_reports_something_sensible() {
        let v = engine_version();
        assert!(v.contains("RawForge"));
    }

    #[test]
    fn extract_and_export_round_trip_does_not_panic() {
        // 4x4 immagine PNG generata al volo, per non dipendere da fixture esterne.
        let mut buf = Vec::new();
        {
            use image::{ImageBuffer, Rgba};
            let img = ImageBuffer::from_fn(4, 4, |_, _| Rgba([200u8, 120, 60, 255]));
            let dyn_img = image::DynamicImage::ImageRgba8(img);
            dyn_img
                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .unwrap();
        }

        let look = extract_look_from_reference_image(buf, "Test Look".to_string()).unwrap();
        assert_eq!(look.name, "Test Look");

        let xmp_out = generate_lightroom_preset_xmp(look);
        assert!(xmp_out.contains("Test Look"));
        assert!(xmp_out.contains("crs:ProcessVersion"));
    }

    #[test]
    fn invalid_bytes_yield_decode_error() {
        let result = extract_look_from_reference_image(vec![0, 1, 2, 3], "Bad".to_string());
        assert!(result.is_err());
    }

    // NB: nessun file RAW reale è disponibile in questo ambiente (nessuna
    // fotocamera, nessun campione scaricabile qui) — questi test coprono solo
    // i percorsi di errore su input non validi, non un vero round-trip di
    // decodifica. La decodifica di un file RAW reale va verificata a mano una
    // volta disponibile un file di esempio (es. caricandolo dalla UI).

    #[test]
    fn garbage_bytes_yield_raw_file_error_not_panic() {
        let result = decode_raw_file_preview(vec![9, 9, 9, 9, 9, 9, 9, 9]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_raw_bytes_yield_raw_file_error() {
        let result = decode_raw_file_preview(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn extract_look_from_raw_reference_reports_error_on_bad_input() {
        let result = extract_look_from_raw_reference(vec![1, 2, 3], "Test".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn known_raw_file_names_are_recognized() {
        assert!(is_known_raw_file_name("IMG_1234.CR3".to_string()));
        assert!(!is_known_raw_file_name("foto.jpg".to_string()));
    }

    #[test]
    fn harmonic_look_ffi_round_trip_preserves_all_fields() {
        // Copre la regressione di fedeltà corretta in questo incremento: prima
        // highlights/shadows/whites/blacks/saturation/hsl/tone_curve/
        // split_toning.balance/white_balance.tint venivano silenziosamente
        // azzerati nel giro Kotlin -> Rust -> Kotlin.
        let mut original = core_types::HarmonicLook::default();
        original.highlights = -30;
        original.shadows = 45;
        original.whites = 10;
        original.blacks = -5;
        original.saturation = 12;
        original.hsl.hue[3] = 7;
        original.hsl.sat[3] = -9;
        original.hsl.lum[3] = 2;
        original.split_toning.balance = 15;
        original.white_balance.tint = -8;

        let ffi: HarmonicLookFfi = original.clone().into();
        let round_tripped: core_types::HarmonicLook = ffi.into();

        assert_eq!(round_tripped.highlights, original.highlights);
        assert_eq!(round_tripped.shadows, original.shadows);
        assert_eq!(round_tripped.whites, original.whites);
        assert_eq!(round_tripped.blacks, original.blacks);
        assert_eq!(round_tripped.saturation, original.saturation);
        assert_eq!(round_tripped.hsl.hue[3], original.hsl.hue[3]);
        assert_eq!(round_tripped.hsl.sat[3], original.hsl.sat[3]);
        assert_eq!(round_tripped.hsl.lum[3], original.hsl.lum[3]);
        assert_eq!(round_tripped.split_toning.balance, original.split_toning.balance);
        assert_eq!(round_tripped.white_balance.tint, original.white_balance.tint);
        assert_eq!(round_tripped.tone_curve, original.tone_curve);
    }

    fn png_bytes_of_solid_color(size: u32, rgb: [u8; 3]) -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let mut buf = Vec::new();
        let img = ImageBuffer::from_fn(size, size, |_, _| Rgba([rgb[0], rgb[1], rgb[2], 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn paste_look_onto_target_photo_renders_and_reports_positive_recovery_on_darker_target() {
        let sample_bytes = png_bytes_of_solid_color(6, [200, 150, 100]);
        let target_bytes = png_bytes_of_solid_color(6, [20, 20, 20]);

        let result = paste_look_onto_target_photo(
            sample_bytes,
            "campione.png".to_string(),
            "Campione".to_string(),
            target_bytes,
            "target.png".to_string(),
            1.0,
        )
        .unwrap();

        assert!(!result.rendered_preview_png_bytes.is_empty());
        // Il target è molto più scuro del campione: ci aspettiamo un recupero
        // di esposizione positivo, entro i guardrail testati in `smartbatch`.
        assert!(
            result.applied_exposure_ev > 0.0,
            "atteso recupero positivo, got {}",
            result.applied_exposure_ev
        );
    }

    #[test]
    fn paste_look_onto_target_photo_reports_error_on_bad_sample_bytes() {
        let result = paste_look_onto_target_photo(
            vec![1, 2, 3],
            "x.jpg".to_string(),
            "Look".to_string(),
            vec![9, 9, 9],
            "y.jpg".to_string(),
            1.0,
        );
        assert!(result.is_err());
    }
}
