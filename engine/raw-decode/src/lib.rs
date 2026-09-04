//! Wraps `rawler` per estrarre da un file RAW vero (CR2/CR3/NEF/ARW/RAF/RW2/
//! DNG/...) l'anteprima incorporata dalla fotocamera stessa e i metadati
//! base (marca/modello). Vedi `docs/ARCHITECTURE.md` §2.1 (livello di cache
//! L0: "JPEG embedded EXIF, decodificato lazy, istantaneo") e §9 (rischio
//! licenza LGPL — nota sotto).
//!
//! **Scelta deliberata rispetto a LibRaw (C++)** descritto nell'architettura
//! originale: `rawler` è puro Rust, quindi cross-compila per Android tramite
//! `cargo-ndk` esattamente come ogni altro crate di questo workspace — nessun
//! toolchain NDK C++/CMake da configurare per la libreria di decodifica
//! stessa. È lo stesso "motore alternativo" che l'architettura menzionava
//! come piano B (§1.1, `rawler` come fallback a LibRaw); qui diventa il
//! piano A per la prima demo funzionante, perché elimina il blocco tecnico
//! più difficile rimasto aperto in questo repository (cross-compilare codice
//! C++ per Android).
//!
//! **NB legale** (LGPL-2.1, come da §9 del documento di architettura):
//! `rawler` è distribuito sotto LGPL-2.1, esattamente come LibRaw. In Rust il
//! link è tipicamente statico — la piena conformità LGPL per una
//! distribuzione commerciale (specie su Android, dove il "re-linking"
//! dinamico è scomodo) va verificata con un legale prima di un lancio
//! pubblico. Qui restiamo nell'ambito di un prototipo tecnico, non di una
//! valutazione legale.
//!
//! **Cosa NON fa (ancora) questo crate**: demosaic + color-correction
//! completi, cioè l'immagine RAW "sviluppata" a piena risoluzione secondo la
//! pipeline di `gpu-pipe` (§3.2). Estrarre solo l'anteprima incorporata è la
//! scelta corretta per il primo incremento: è comunque un'anteprima fedele
//! generata dalla fotocamera stessa al momento dello scatto (non un
//! placeholder), è pressoché istantanea (nessun demosaic da calcolare), e
//! sblocca subito un flusso reale "importa un vero file RAW -> vedi la foto
//! -> applica Sintesi Armonica/export XMP" end-to-end con codice genuino, non
//! finto. Il demosaic completo (per l'export a piena risoluzione) resta il
//! prossimo incremento naturale, una volta che questo è verde in CI.

use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;

#[derive(thiserror::Error, Debug)]
pub enum RawDecodeError {
    #[error("formato RAW non riconosciuto o file corrotto: {reason}")]
    UnrecognizedFormat { reason: String },
    #[error("il file RAW non contiene un'anteprima incorporata utilizzabile")]
    NoEmbeddedPreview,
}

/// Metadati minimi di un file RAW, estratti senza demosaic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFileInfo {
    pub camera_make: String,
    pub camera_model: String,
}

/// Risultato della decodifica "veloce" di un file RAW: l'anteprima incorporata
/// dalla fotocamera (già un'immagine RGB pienamente sviluppata, tipicamente un
/// JPEG interno) più i metadati base.
pub struct RawPreview {
    pub info: RawFileInfo,
    pub image: DynamicImage,
}

/// Estrae l'anteprima incorporata (o, in assenza, la thumbnail più piccola) e
/// i metadati base da un file RAW passato come byte in memoria — mai scritto
/// su disco, adatto sia al percorso desktop sia a quello Android (dove i file
/// si leggono spesso da content:// URI, non da path di filesystem).
pub fn decode_raw_preview(raw_bytes: &[u8]) -> Result<RawPreview, RawDecodeError> {
    let source = RawSource::new_from_slice(raw_bytes);

    let decoder = rawler::get_decoder(&source).map_err(|e| RawDecodeError::UnrecognizedFormat {
        reason: e.to_string(),
    })?;
    let params = RawDecodeParams::default();

    let metadata = decoder
        .raw_metadata(&source, &params)
        .map_err(|e| RawDecodeError::UnrecognizedFormat {
            reason: e.to_string(),
        })?;

    // Preferiamo la preview (più grande, quando disponibile) e ripieghiamo
    // sulla thumbnail: molti decoder implementano solo una delle due.
    let image = decoder
        .preview_image(&source, &params)
        .ok()
        .flatten()
        .or_else(|| decoder.thumbnail_image(&source, &params).ok().flatten())
        .ok_or(RawDecodeError::NoEmbeddedPreview)?;

    Ok(RawPreview {
        info: RawFileInfo {
            camera_make: metadata.make,
            camera_model: metadata.model,
        },
        image,
    })
}

/// Estensioni di file RAW che `rawler` sa riconoscere, usata dalla UI per
/// filtrare la selezione file (import cartella) senza dover tentare la
/// decodifica di ogni singolo file solo per scoprire se è un RAW.
pub const KNOWN_RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "nef", "arw", "raf", "rw2", "dng", "pef", "orf", "srw", "raw", "3fr", "mrw",
];

pub fn has_known_raw_extension(file_name: &str) -> bool {
    file_name
        .rsplit('.')
        .next()
        .map(|ext| KNOWN_RAW_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_bytes_yield_unrecognized_format_error_not_a_panic() {
        let result = decode_raw_preview(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_bytes_do_not_panic() {
        let result = decode_raw_preview(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_tiff_like_header_does_not_panic() {
        // Un header che assomiglia all'inizio di un TIFF/RAW (magic number
        // "II*\0") ma è troncato subito dopo: deve fallire in modo pulito,
        // non fare panic durante il parsing dell'IFD.
        let fake = [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let result = decode_raw_preview(&fake);
        assert!(result.is_err());
    }

    #[test]
    fn known_raw_extensions_are_recognized_case_insensitively() {
        assert!(has_known_raw_extension("IMG_0421.CR3"));
        assert!(has_known_raw_extension("foto.nef"));
        assert!(has_known_raw_extension("scatto.DNG"));
        assert!(!has_known_raw_extension("foto.jpg"));
        assert!(!has_known_raw_extension("senza_estensione"));
    }
}
