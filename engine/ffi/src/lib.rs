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
}
