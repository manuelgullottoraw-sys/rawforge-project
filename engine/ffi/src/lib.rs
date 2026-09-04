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

/// Versione "piatta" (solo tipi primitivi, compatibile UniFFI) di `core_types::HarmonicLook`.
#[derive(uniffi::Record, Clone, Debug)]
pub struct HarmonicLookFfi {
    pub name: String,
    pub exposure_ev: f32,
    pub contrast: i32,
    pub vibrance: i32,
    pub white_balance_temp: u32,
    pub shadow_hue: i32,
    pub shadow_sat: i32,
    pub highlight_hue: i32,
    pub highlight_sat: i32,
}

impl From<core_types::HarmonicLook> for HarmonicLookFfi {
    fn from(look: core_types::HarmonicLook) -> Self {
        Self {
            name: look.name,
            exposure_ev: look.exposure_ev,
            contrast: look.contrast,
            vibrance: look.vibrance,
            white_balance_temp: look.white_balance.temp,
            shadow_hue: look.split_toning.shadow_hue,
            shadow_sat: look.split_toning.shadow_sat,
            highlight_hue: look.split_toning.highlight_hue,
            highlight_sat: look.split_toning.highlight_sat,
        }
    }
}

impl From<HarmonicLookFfi> for core_types::HarmonicLook {
    fn from(look: HarmonicLookFfi) -> Self {
        let mut full = core_types::HarmonicLook {
            name: look.name,
            exposure_ev: look.exposure_ev,
            contrast: look.contrast,
            vibrance: look.vibrance,
            ..core_types::HarmonicLook::default()
        };
        full.white_balance.temp = look.white_balance_temp;
        full.split_toning.shadow_hue = look.shadow_hue;
        full.split_toning.shadow_sat = look.shadow_sat;
        full.split_toning.highlight_hue = look.highlight_hue;
        full.split_toning.highlight_sat = look.highlight_sat;
        full
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
}
