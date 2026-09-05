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
//! **Due percorsi di decodifica, per due scopi diversi**: `decode_raw_preview`
//! estrae solo l'anteprima JPEG incorporata dalla fotocamera — pressoché
//! istantanea (nessun demosaic da calcolare), adatta a miniature di Libreria
//! e all'analisi della foto campione per la Sintesi Armonica, dove la
//! velocità conta più della precisione assoluta. `decode_raw_full` (aggiunto
//! in un giro successivo al primo incremento, su richiesta esplicita
//! dell'utente per un uso editoriale) esegue invece il demosaic VERO del
//! sensore — via la pipeline di sviluppo di `rawler` stessa (algoritmo PPG
//! per il Bayer RGB), non la pipeline `gpu-pipe`/WGSL di questo workspace
//! (quella resta pensata per il color grading interattivo lato Compose, non
//! ancora collegata) — usato SOLO per il file che verrà consegnato
//! all'utente (l'esportazione finale, singola o batch), mai per un'anteprima
//! o un'analisi. Limitato deliberatamente al Bayer RGB (le uniche fotocamere
//! in uso in questo progetto, Sony A7 IV e Canon EOS 77D): vedi il commento
//! esteso su `decode_raw_full` per il perché.

use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::imgop::develop::{Intermediate, RawDevelop};
use rawler::imgop::sensor::SensorType;
use rawler::rawimage::{CFAConfig, RawPhotometricInterpretation};
use rawler::rawsource::RawSource;

#[derive(thiserror::Error, Debug)]
pub enum RawDecodeError {
    #[error("formato RAW non riconosciuto o file corrotto: {reason}")]
    UnrecognizedFormat { reason: String },
    #[error("il file RAW non contiene un'anteprima incorporata utilizzabile")]
    NoEmbeddedPreview,
    /// Il file è un RAW riconosciuto, ma il suo layout sensore non è uno dei
    /// due effettivamente in uso in questo progetto (Bayer RGB — Sony A7 IV,
    /// Canon EOS 77D): vedi il commento esteso su `decode_raw_full` per il
    /// perché di questo limite deliberato, invece di tentare comunque un
    /// demosaic non verificato su un formato mai testato.
    #[error("layout sensore non supportato per il demosaic a piena risoluzione: {reason}")]
    UnsupportedSensorLayout { reason: String },
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

/// Estrae l'anteprima incorporata (o, in assenza, la thumbnail) e i metadati
/// base da un file RAW passato come byte in memoria — mai scritto su disco,
/// adatto sia al percorso desktop sia a quello Android (dove i file si
/// leggono spesso da content:// URI, non da path di filesystem).
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

    // Prima si tentano ENTRAMBE preview e thumbnail (quando il decoder le
    // implementa) e si tiene quella con più pixel, invece di "preferire
    // sempre la preview per nome e ripiegare sulla thumbnail solo se la
    // preview manca del tutto": alcuni file hanno una preview presente ma più
    // piccola della thumbnail (nomenclatura non standardizzata fra formati/
    // fotocamere), e prendere la preview "perché si chiama preview" darebbe
    // silenziosamente un'anteprima più piccola del massimo disponibile —
    // esattamente il tipo di regressione di qualità/risoluzione segnalata
    // dall'utente con foto vere ("l'esportazione deve avvenire alla massima
    // risoluzione possibile").
    let preview = decoder.preview_image(&source, &params).ok().flatten();
    let thumbnail = decoder.thumbnail_image(&source, &params).ok().flatten();
    let image = match (preview, thumbnail) {
        (Some(p), Some(t)) => {
            if pixel_count(&p) >= pixel_count(&t) {
                p
            } else {
                t
            }
        }
        (Some(p), None) => p,
        (None, Some(t)) => t,
        (None, None) => return Err(RawDecodeError::NoEmbeddedPreview),
    };

    // Né `rawler` né la libreria `image` ruotano/specchiano l'anteprima
    // secondo il tag EXIF Orientation da sole (entrambe restituiscono i
    // pixel esattamente come sono memorizzati nel file) — senza questa
    // correzione, qualunque foto scattata con la fotocamera ruotata (la
    // stragrande maggioranza delle foto verticali) uscirebbe storta. Bug
    // reale segnalato dall'utente con foto vere ("l'orientamento è
    // completamente sballato"). Vedi `apply_exif_orientation` sotto.
    let image = apply_exif_orientation(image, metadata.exif.orientation);

    Ok(RawPreview {
        info: RawFileInfo {
            camera_make: metadata.make,
            camera_model: metadata.model,
        },
        image,
    })
}

fn pixel_count(image: &DynamicImage) -> u64 {
    image.width() as u64 * image.height() as u64
}

/// Risultato del demosaic VERO (non l'anteprima incorporata) di un file RAW:
/// stessi metadati base di [`RawPreview`], ma `image` qui è un
/// `DynamicImage::ImageRgb32F` — pixel in virgola mobile a 32 bit,
/// direttamente dal sensore, non un JPEG che la fotocamere aveva già
/// generato al proprio interno.
pub struct RawFullImage {
    pub info: RawFileInfo,
    pub image: DynamicImage,
}

/// Sviluppa un file RAW a PIENA risoluzione sensore: demosaic vero (algoritmo
/// PPG di `rawler` per il Bayer RGB, la stessa famiglia di algoritmi usata da
/// darktable/RawTherapee, ben più accurata del bilineare su bordi/dettaglio
/// fine — proprio il tipo di errore visibile su fogliame e transizioni di
/// colore segnalato dall'utente con foto vere), calibrazione colore (matrice
/// XYZ->camera del profilo specifico dal database interno di `rawler`) e
/// bilanciamento del bianco "as shot" (`rawimage.wb_coeffs`, letto dal file
/// stesso), gamma sRGB applicata in fondo — la stessa sequenza di stage che
/// userebbe qualunque editor RAW "neutro" (Rescale -> Demosaic -> FujiRotate
/// -> CropActiveArea -> WhiteBalance -> Calibrate -> CropDefault -> SRgb, la
/// pipeline di default di `RawDevelop` in `rawler`).
///
/// **Perché questa funzione esiste accanto a `decode_raw_preview`, invece di
/// sostituirla**: quest'ultima resta la scelta giusta per tutto ciò che non
/// diventa il file consegnato all'utente — miniature di Libreria, analisi
/// della foto campione per la Sintesi Armonica/Smart-Batch, anteprima
/// interattiva mentre si trascina uno slider — dove la velocità conta più
/// della precisione assoluta e un demosaic completo ad ogni tick sarebbe
/// sprecato. Questa funzione è invece per l'UNICA cosa che deve davvero
/// essere alla massima qualità disponibile: il file finale (JPEG di consegna
/// + master TIFF 16 bit), sia per la foto singola sia per ciascuna foto del
/// batch — vedi `PhotoEditSession::new` in `rawforge-ffi`, l'unico punto che
/// la richiama.
///
/// **NON quantizza MAI a 8 bit**: l'immagine restituita resta `f32` per
/// canale dall'inizio alla fine di questa funzione (nessun arrotondamento
/// intermedio) — è la pipeline di editing (`look_render`) a continuare in
/// `f32` fino alla codifica JPEG/TIFF finale, per la stessa ragione:
/// arrotondare a 8 bit anche una sola volta lungo la catena introduce un
/// errore di quantizzazione che poi la stessa pipeline di color grading
/// amplifica (contrasto, HSL per banda, ecc.) — esattamente il meccanismo
/// dietro la "qualità pessima su vegetazione e colori" segnalata
/// dall'utente, anche se in quel caso la causa concreta era l'anteprima
/// incorporata (già JPEG-compressa dalla fotocamera), non un arrotondamento
/// nostro. Qui eliminiamo ENTRAMBE le fonti di perdita.
///
/// **Limite deliberato: solo Bayer RGB** (Sony A7 IV, Canon EOS 77D — le
/// uniche fotocamere effettivamente in uso in questo progetto, entrambe
/// Bayer classico). `rawler` sa demosaicizzare anche l'X-Trans Fujifilm
/// (Markesteijn) e altri layout CFA, ma supportarli qui senza un file vero
/// di quelle fotocamere per verificarlo sarebbe un rischio silenzioso non
/// giustificato — meglio `UnsupportedSensorLayout` (un errore esplicito, con
/// messaggio chiaro) che un'immagine sviluppata male senza che nessuno se ne
/// accorga. Ampliabile in futuro se servirà una fotocamera con sensore
/// diverso.
///
/// **Non testato end-to-end su un file RAW vero in questo ambiente**: qui
/// non è disponibile alcun file RAW reale (né Sony né Canon) su cui
/// verificare il demosaic effettivo — limite onestamente dichiarato, non
/// nascosto. Testati invece: la gestione pulita di bytes non validi (stesso
/// standard di `decode_raw_preview`) e la logica di riconoscimento del
/// layout sensore (`is_supported_bayer_rgb`) in isolamento. La verifica
/// completa su scatti reali resta da fare alla prossima build con file
/// dell'utente.
pub fn decode_raw_full(raw_bytes: &[u8]) -> Result<RawFullImage, RawDecodeError> {
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

    // `dummy: false` — un valore `true` farebbe restituire a `rawler` un
    // placeholder vuoto invece dei pixel veri (usato altrove per letture
    // "solo metadati" molto più veloci): qui servono i pixel veri.
    let rawimage = decoder
        .raw_image(&source, &params, false)
        .map_err(|e| RawDecodeError::UnrecognizedFormat {
            reason: e.to_string(),
        })?;

    match &rawimage.photometric {
        RawPhotometricInterpretation::Cfa(config) if is_supported_bayer_rgb(config) => {}
        _ => {
            return Err(RawDecodeError::UnsupportedSensorLayout {
                reason: format!(
                    "{} {}: sensore non Bayer RGB (X-Trans, monocromatico, o CFA a 4 canali) — supportato solo per Sony/Canon Bayer",
                    metadata.make, metadata.model
                ),
            });
        }
    }

    let intermediate = RawDevelop::default()
        .develop_intermediate(&rawimage)
        .map_err(|e| RawDecodeError::UnrecognizedFormat {
            reason: e.to_string(),
        })?;

    let (width, height, mut rgb) = match intermediate {
        Intermediate::ThreeColor(pixels) => {
            let dim = pixels.dim();
            (dim.w as u32, dim.h as u32, pixels.flatten())
        }
        Intermediate::Monochrome(_) => {
            return Err(RawDecodeError::UnsupportedSensorLayout {
                reason: "immagine monocromatica (1 canale) dopo il demosaic — non supportata".to_string(),
            });
        }
        Intermediate::FourColor(_) => {
            return Err(RawDecodeError::UnsupportedSensorLayout {
                reason: "layout CFA a 4 canali dopo il demosaic — non supportato".to_string(),
            });
        }
    };

    sanitize_float_buffer(&mut rgb);

    let buffer: image::ImageBuffer<image::Rgb<f32>, Vec<f32>> = image::ImageBuffer::from_raw(width, height, rgb)
        .ok_or_else(|| RawDecodeError::UnrecognizedFormat {
            reason: "dimensioni non valide per il buffer demosaicizzato".to_string(),
        })?;
    let image = apply_exif_orientation(DynamicImage::ImageRgb32F(buffer), metadata.exif.orientation);

    Ok(RawFullImage {
        info: RawFileInfo {
            camera_make: metadata.make,
            camera_model: metadata.model,
        },
        image,
    })
}

/// Vero solo per un CFA Bayer RGB standard (RGGB/BGGR/GBRG/GRGB) — non
/// X-Trans, non monocromatico, non un CFA a colori non-RGB (es. CMY). Estratta
/// come funzione pura separata da `decode_raw_full` proprio per poterla
/// testare da sola con un `CFAConfig` costruito a mano, senza bisogno di un
/// file RAW vero (non disponibile in questo ambiente di sviluppo).
fn is_supported_bayer_rgb(config: &CFAConfig) -> bool {
    config.cfa.is_rgb() && config.sensor == SensorType::Bayer
}

/// La calibrazione colore (matrice XYZ->camera) e il bilanciamento del
/// bianco possono, per pixel estremi fuori gamut, produrre valori negativi o
/// non finiti (NaN/inf) — la gamma sRGB di `rawler` applicherebbe `powf` su
/// un valore negativo producendo NaN, che poi si propagherebbe attraverso
/// tutta la pipeline di editing a valle (qualunque media/blend con un NaN
/// resta NaN). Puliamo qui, una sola volta, invece di far sì che ogni stage
/// successivo (di questo crate e di `look_render`) debba difendersi da un
/// input malformato: NaN/inf -> 0.0 (nero, il valore più sicuro), negativi ->
/// 0.0 (la luce non può essere negativa). I valori sopra 1.0 (luci
/// "sopraesposte" nei dati del sensore) restano invece intatti: non sono un
/// errore, sono margine di recupero luci che la pipeline di rendering può
/// scegliere di sfruttare in futuro — oggi viene comunque clampato a 0..1 al
/// primo stage di `look_render::render_look_core`, ma non è questo il posto
/// giusto per deciderlo.
fn sanitize_float_buffer(buffer: &mut [f32]) {
    for v in buffer.iter_mut() {
        if !v.is_finite() {
            *v = 0.0;
        } else if *v < 0.0 {
            *v = 0.0;
        }
    }
}

/// Applica la correzione di orientamento EXIF (valore standard 1..8 del tag
/// IFD Orientation, 0x0112 — la stessa tabella usata da qualunque editor
/// fotografico, es. https://exiftool.org/TagNames/EXIF.html) a un'immagine
/// già decodificata:
///
/// - 1 (o assente/sconosciuto): normale, nessuna trasformazione.
/// - 2: specchio orizzontale.
/// - 3: rotazione 180°.
/// - 4: specchio verticale.
/// - 5: specchio orizzontale + rotazione 270° oraria (equivalente a una
///   trasposizione pura).
/// - 6: rotazione 90° oraria.
/// - 7: specchio orizzontale + rotazione 90° oraria (equivalente a una
///   trasposizione trasversale).
/// - 8: rotazione 270° oraria (cioè 90° antioraria).
///
/// Pubblica (non solo uso interno di `decode_raw_preview`) perché lo stesso
/// problema si presenta anche per le foto già sviluppate (JPEG) importate
/// direttamente, non solo per i RAW: il chiamante (`rawforge-ffi`) legge
/// l'orientamento da sé con `kamadak-exif` sui bytes originali e applica
/// questa stessa funzione.
pub fn apply_exif_orientation(image: DynamicImage, orientation: Option<u16>) -> DynamicImage {
    match orientation {
        Some(2) => image.fliph(),
        Some(3) => image.rotate180(),
        Some(4) => image.flipv(),
        Some(5) => image.fliph().rotate270(),
        Some(6) => image.rotate90(),
        Some(7) => image.fliph().rotate90(),
        Some(8) => image.rotate270(),
        _ => image,
    }
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
    use image::GenericImageView;

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

    /// Immagine 3x2 asimmetrica (larghezza ≠ altezza, così una trasposizione
    /// e una semplice rotazione non sono confondibili) con i quattro angoli
    /// marcati da colori distinti — permette di verificare dove finisce
    /// ciascun angolo dopo una correzione di orientamento, non solo che le
    /// dimensioni cambino.
    fn marked_test_image() -> DynamicImage {
        let mut img = image::RgbaImage::new(3, 2);
        for px in img.pixels_mut() {
            *px = image::Rgba([10, 10, 10, 255]); // sfondo, per distinguerlo dagli angoli
        }
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255])); // TL rosso
        img.put_pixel(2, 0, image::Rgba([0, 255, 0, 255])); // TR verde
        img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255])); // BL blu
        img.put_pixel(2, 1, image::Rgba([255, 255, 0, 255])); // BR giallo
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn orientation_none_or_value_1_is_a_no_op() {
        let original = marked_test_image();
        for value in [None, Some(1)] {
            let corrected = apply_exif_orientation(original.clone(), value);
            assert_eq!(corrected.as_bytes(), original.as_bytes());
            assert_eq!(corrected.dimensions(), original.dimensions());
        }
    }

    #[test]
    fn orientation_unknown_value_is_treated_as_a_no_op() {
        // Un valore fuori dall'intervallo standard 1..8 (tag EXIF corrotto o
        // non conforme) non deve far panic né applicare una trasformazione
        // arbitraria: meglio un'anteprima non ruotata che uno schianto.
        let original = marked_test_image();
        let corrected = apply_exif_orientation(original.clone(), Some(200));
        assert_eq!(corrected.as_bytes(), original.as_bytes());
    }

    #[test]
    fn orientation_2_mirrors_horizontally_without_changing_dimensions() {
        let original = marked_test_image();
        let corrected = apply_exif_orientation(original.clone(), Some(2));
        assert_eq!(corrected.dimensions(), original.dimensions());
        // Specchio orizzontale: TL <-> TR, BL <-> BR.
        assert_eq!(corrected.get_pixel(2, 0), original.get_pixel(0, 0));
        assert_eq!(corrected.get_pixel(0, 0), original.get_pixel(2, 0));
    }

    #[test]
    fn orientation_4_mirrors_vertically_without_changing_dimensions() {
        let original = marked_test_image();
        let corrected = apply_exif_orientation(original.clone(), Some(4));
        assert_eq!(corrected.dimensions(), original.dimensions());
        // Specchio verticale: TL <-> BL, TR <-> BR.
        assert_eq!(corrected.get_pixel(0, 1), original.get_pixel(0, 0));
        assert_eq!(corrected.get_pixel(0, 0), original.get_pixel(0, 1));
    }

    #[test]
    fn orientation_3_rotates_180_moving_each_corner_to_the_opposite_one() {
        let original = marked_test_image();
        let corrected = apply_exif_orientation(original.clone(), Some(3));
        assert_eq!(corrected.dimensions(), original.dimensions());
        assert_eq!(corrected.get_pixel(2, 1), original.get_pixel(0, 0)); // TL -> BR
        assert_eq!(corrected.get_pixel(0, 0), original.get_pixel(2, 1)); // BR -> TL
        assert_eq!(corrected.get_pixel(2, 0), original.get_pixel(0, 1)); // BL -> TR
        assert_eq!(corrected.get_pixel(0, 1), original.get_pixel(2, 0)); // TR -> BL
    }

    #[test]
    fn orientations_2_3_4_are_involutions_applying_twice_restores_the_original() {
        let original = marked_test_image();
        for value in [2u16, 3, 4] {
            let twice = apply_exif_orientation(apply_exif_orientation(original.clone(), Some(value)), Some(value));
            assert_eq!(twice.as_bytes(), original.as_bytes(), "orientamento {value} applicato due volte deve restituire l'originale");
        }
    }

    #[test]
    fn orientations_5_6_7_8_swap_width_and_height() {
        let original = marked_test_image();
        let (w, h) = original.dimensions();
        for value in [5u16, 6, 7, 8] {
            let corrected = apply_exif_orientation(original.clone(), Some(value));
            assert_eq!(
                corrected.dimensions(),
                (h, w),
                "orientamento {value} deve scambiare larghezza e altezza"
            );
        }
    }

    #[test]
    fn orientations_5_and_7_are_involutions_despite_swapping_dimensions() {
        // 5 e 7 sono trasposizioni (riflessioni lungo una diagonale): come
        // ogni riflessione, applicarle due volte restituisce l'originale,
        // anche se ciascuna applicazione singola scambia le dimensioni.
        let original = marked_test_image();
        for value in [5u16, 7] {
            let twice = apply_exif_orientation(apply_exif_orientation(original.clone(), Some(value)), Some(value));
            assert_eq!(twice.as_bytes(), original.as_bytes(), "orientamento {value} applicato due volte deve restituire l'originale");
            assert_eq!(twice.dimensions(), original.dimensions());
        }
    }

    #[test]
    fn orientations_6_and_8_are_mutual_inverses() {
        // 6 (rotazione 90°) e 8 (rotazione 270°, l'opposta) applicate in
        // sequenza sommano a una rotazione di 360°: devono restituire
        // esattamente l'originale, qualunque sia la convenzione oraria/
        // antioraria usata internamente — una proprietà indipendente da
        // quella convenzione, quindi robusta anche a un'inversione di segno
        // non colta dai singoli test sopra.
        let original = marked_test_image();
        let round_trip = apply_exif_orientation(apply_exif_orientation(original.clone(), Some(6)), Some(8));
        assert_eq!(round_trip.as_bytes(), original.as_bytes());
        assert_eq!(round_trip.dimensions(), original.dimensions());
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
    fn decode_raw_full_invalid_bytes_yield_an_error_not_a_panic() {
        let result = decode_raw_full(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_raw_full_empty_bytes_do_not_panic() {
        let result = decode_raw_full(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_raw_full_truncated_tiff_like_header_does_not_panic() {
        let fake = [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let result = decode_raw_full(&fake);
        assert!(result.is_err());
    }

    #[test]
    fn is_supported_bayer_rgb_accepts_a_standard_bayer_rgb_cfa() {
        let config = CFAConfig::new(&rawler::cfa::CFA::new("RGGB"), &rawler::cfa::PlaneColor::new("RGGB"));
        assert!(is_supported_bayer_rgb(&config));
    }

    #[test]
    fn is_supported_bayer_rgb_rejects_xtrans_sensor_type() {
        let mut config = CFAConfig::new(&rawler::cfa::CFA::new("RGGB"), &rawler::cfa::PlaneColor::new("RGGB"));
        config.sensor = SensorType::Xtrans;
        assert!(!is_supported_bayer_rgb(&config));
    }

    #[test]
    fn sanitize_float_buffer_replaces_nan_and_infinity_with_zero() {
        let mut buf = [1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.5];
        sanitize_float_buffer(&mut buf);
        assert_eq!(buf, [1.0, 0.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn sanitize_float_buffer_clamps_negative_values_to_zero_but_preserves_headroom_above_one() {
        let mut buf = [-0.5, 1.8, -0.001, 0.0];
        sanitize_float_buffer(&mut buf);
        assert_eq!(buf, [0.0, 1.8, 0.0, 0.0]);
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
