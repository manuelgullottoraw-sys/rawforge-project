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

/// Controparte UniFFI di `core_types::MaskTarget` — vedi lì per la spiegazione.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskTargetFfi {
    Subject,
    Background,
}

impl From<core_types::MaskTarget> for MaskTargetFfi {
    fn from(target: core_types::MaskTarget) -> Self {
        match target {
            core_types::MaskTarget::Subject => MaskTargetFfi::Subject,
            core_types::MaskTarget::Background => MaskTargetFfi::Background,
        }
    }
}

impl From<MaskTargetFfi> for core_types::MaskTarget {
    fn from(target: MaskTargetFfi) -> Self {
        match target {
            MaskTargetFfi::Subject => core_types::MaskTarget::Subject,
            MaskTargetFfi::Background => core_types::MaskTarget::Background,
        }
    }
}

/// Controparte UniFFI di `look_render::TonalMaskKind` — quale dei quattro
/// slider tonali mascherati per zona (Ombre/Luci/Neri/Bianchi) la UI vuole
/// disegnare sopra l'istogramma a schermo (vedi [`tonal_mask_curve`]).
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TonalMaskKindFfi {
    Shadows,
    Highlights,
    Blacks,
    Whites,
}

impl From<TonalMaskKindFfi> for look_render::TonalMaskKind {
    fn from(kind: TonalMaskKindFfi) -> Self {
        match kind {
            TonalMaskKindFfi::Shadows => look_render::TonalMaskKind::Shadows,
            TonalMaskKindFfi::Highlights => look_render::TonalMaskKind::Highlights,
            TonalMaskKindFfi::Blacks => look_render::TonalMaskKind::Blacks,
            TonalMaskKindFfi::Whites => look_render::TonalMaskKind::Whites,
        }
    }
}

/// Espone `look_render::tonal_mask_curve` alla UI: 256 pesi (0.0..1.0, uno
/// per bin di luma, stessa convenzione di `RenderedPreviewFfi::
/// luminance_histogram`) che dicono quanto lo slider `kind` sta modificando
/// ciascuna fascia tonale — indipendente dalla foto aperta (è una proprietà
/// della sola formula di maschera, non dei suoi pixel), quindi la UI può
/// chiamarlo una volta sola per ciascuno dei quattro valori e tenere il
/// risultato in cache, invece di richiederlo ad ogni tick di trascinamento.
#[uniffi::export]
pub fn tonal_mask_curve(kind: TonalMaskKindFfi) -> Vec<f32> {
    look_render::tonal_mask_curve(kind.into()).to_vec()
}

/// Controparte UniFFI di `core_types::SubjectMask` — vedi lì per la
/// spiegazione completa dei campi.
#[derive(uniffi::Record, Clone, Debug)]
pub struct SubjectMaskFfi {
    pub enabled: bool,
    pub target: MaskTargetFfi,
    pub exposure_ev: f32,
    pub contrast: i32,
    pub saturation: i32,
}

impl From<core_types::SubjectMask> for SubjectMaskFfi {
    fn from(mask: core_types::SubjectMask) -> Self {
        Self {
            enabled: mask.enabled,
            target: mask.target.into(),
            exposure_ev: mask.exposure_ev,
            contrast: mask.contrast,
            saturation: mask.saturation,
        }
    }
}

impl From<SubjectMaskFfi> for core_types::SubjectMask {
    fn from(mask: SubjectMaskFfi) -> Self {
        Self {
            enabled: mask.enabled,
            target: mask.target.into(),
            exposure_ev: mask.exposure_ev,
            contrast: mask.contrast,
            saturation: mask.saturation,
        }
    }
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
    /// Texture per banda di frequenza (-100..100) — vedi
    /// `core_types::HarmonicLook` per la spiegazione completa.
    pub texture_fine: i32,
    pub texture_medium: i32,
    pub texture_coarse: i32,
    /// Bilanciamento del bianco a gradiente — zona B più i quattro parametri
    /// del gradiente stesso, vedi `core_types::HarmonicLook`.
    pub white_balance_b_temp: u32,
    pub white_balance_b_tint: i32,
    pub wb_gradient_enabled: bool,
    pub wb_gradient_vertical: bool,
    pub wb_gradient_position: i32,
    pub wb_gradient_spread: i32,
    /// Riduzione rumore (0..100 ciascuno) — vedi `core_types::HarmonicLook`.
    pub noise_reduction_luma: i32,
    pub noise_reduction_color: i32,
    /// Maschera automatica Soggetto/Sfondo — vedi `core_types::SubjectMask`.
    pub subject_mask: SubjectMaskFfi,
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
            texture_fine: look.texture_fine,
            texture_medium: look.texture_medium,
            texture_coarse: look.texture_coarse,
            white_balance_b_temp: look.white_balance_b.temp,
            white_balance_b_tint: look.white_balance_b.tint,
            wb_gradient_enabled: look.wb_gradient_enabled,
            wb_gradient_vertical: look.wb_gradient_vertical,
            wb_gradient_position: look.wb_gradient_position,
            wb_gradient_spread: look.wb_gradient_spread,
            noise_reduction_luma: look.noise_reduction_luma,
            noise_reduction_color: look.noise_reduction_color,
            subject_mask: look.subject_mask.into(),
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
            texture_fine: look.texture_fine,
            texture_medium: look.texture_medium,
            texture_coarse: look.texture_coarse,
            white_balance_b: core_types::WhiteBalance {
                temp: look.white_balance_b_temp,
                tint: look.white_balance_b_tint,
            },
            wb_gradient_enabled: look.wb_gradient_enabled,
            wb_gradient_vertical: look.wb_gradient_vertical,
            wb_gradient_position: look.wb_gradient_position,
            wb_gradient_spread: look.wb_gradient_spread,
            noise_reduction_luma: look.noise_reduction_luma,
            noise_reduction_color: look.noise_reduction_color,
            subject_mask: look.subject_mask.into(),
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

/// Anteprima ispezionabile del "probabile soggetto" di un'immagine —
/// `harmonic::compute_saliency_map` reso come immagine in scala di grigi
/// (bianco = alta salienza, nero = bassa) alla stessa risoluzione di analisi
/// usata internamente (max 512px sul lato lungo, non piena risoluzione: è
/// solo un'anteprima diagnostica, non un dato da usare per un crop o una
/// maschera di precisione).
///
/// **Perché questa funzione esiste ma non è collegata a "Incolla
/// impostazioni"**: `compute_saliency_map` è un'euristica di contrasto
/// globale di colore + prior di centratura (vedi il commento esteso su
/// quella funzione in `harmonic` per il funzionamento e i limiti dichiarati)
/// — NON un riconoscimento semantico del soggetto (non sa cosa sia un'auto o
/// un volto). Un primo tentativo di usarla per pesare l'estrazione HSL e
/// l'hue-matching è stato scartato dopo averlo misurato su una foto vera:
/// peggiorava la convergenza di tonalità appena corretta (da un divario di
/// 1.3° a 9.5° dal campione), perché la salienza di ciascuna foto è calcolata
/// in modo indipendente dall'altra e può enfatizzare porzioni diverse dello
/// stesso soggetto reale. Esposta qui invece come funzione autonoma e
/// ispezionabile: la UI può mostrarla come overlay ("ecco cosa il motore
/// considera il soggetto") per un futuro strumento di selezione guidata, senza
/// che influenzi silenziosamente il color matching automatico.
#[uniffi::export]
pub fn compute_subject_saliency_preview(image_bytes: Vec<u8>) -> Result<Vec<u8>, EngineError> {
    let img = image::load_from_memory(&image_bytes)
        .map_err(|e| EngineError::DecodeError { reason: e.to_string() })?;
    let analysis = img.resize(512, 512, image::imageops::FilterType::Triangle);
    let rgba = analysis.to_rgba8();
    let (width, height) = rgba.dimensions();
    let saliency = harmonic::compute_saliency_map(&rgba);

    let mut out = image::GrayImage::new(width, height);
    for (i, px) in out.pixels_mut().enumerate() {
        px.0[0] = (saliency[i].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    encode_preview_as_png(&image::DynamicImage::ImageLuma8(out))
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

/// Qualità JPEG (0..100) usata per l'esportazione a piena risoluzione
/// (`PhotoEditSession::render_full_resolution`, quindi sia il pulsante
/// "Esporta" sia l'elaborazione in batch). Scelta esplicita dell'utente:
/// prima l'esportazione produceva PNG (senza perdita, ma file enormi e non
/// direttamente utilizzabili in molti flussi fotografici che si aspettano
/// JPEG). 92 è il valore comunemente raccomandato come "visivamente senza
/// perdita" per una JPEG a 3 canali (fonte: la stessa soglia usata da
/// `libjpeg`/Lightroom per l'esportazione "Qualità: 100" percepita — oltre
/// 90-92 la dimensione del file cresce molto più della qualità percepita) —
/// qui deliberatamente ALTA (non il default 75 di molte librerie, pensato per
/// il web, non per una consegna fotografica) proprio perché l'utente ha
/// segnalato una qualità insoddisfacente sull'esportazione.
const FULL_RESOLUTION_JPEG_QUALITY: u8 = 92;

fn encode_full_resolution_as_jpeg(image: &image::DynamicImage) -> Result<Vec<u8>, EngineError> {
    let mut jpeg_bytes = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, FULL_RESOLUTION_JPEG_QUALITY);
    // Il JPEG non supporta un canale alpha: `write_image` con `ExtendedColorType::Rgb8`
    // su un buffer RGBA scarterebbe silenziosamente il canale alpha da solo se gli
    // passassimo l'immagine RGBA con quel color type dichiarato male — qui invece
    // convertiamo esplicitamente a RGB8 PRIMA di incodificare, così il colore
    // dichiarato all'encoder corrisponde davvero ai bytes forniti.
    //
    // Questa è l'UNICA quantizzazione a 8 bit di tutta la catena (demosaic ->
    // `look_render::render_full_resolution_with_look`, sempre `f32` fino a
    // qui): `image.to_rgb8()` su un `ImageRgb32F` converte scalando 0.0..1.0
    // -> 0..255 con arrotondamento, esattamente una volta, qui, non prima.
    let rgb = image.to_rgb8();
    encoder
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| EngineError::RawFileError {
            reason: e.to_string(),
        })?;
    Ok(jpeg_bytes)
}

/// Master senza perdita a 16 bit per canale (TIFF), accanto al JPEG di
/// consegna — richiesta esplicita dell'utente per un uso editoriale ("non è
/// ammessa la minima imperfezione"): il JPEG a qualunque qualità resta
/// comunque una compressione con perdita (anche se impercettibile a 92) e a
/// 8 bit per canale; questo file preserva la piena precisione del rendering
/// `f32` fino all'unico arrotondamento realmente inevitabile per un formato
/// su disco — 16 bit per canale (65536 livelli, contro i 256 dell'8 bit: lo
/// stesso arrotondamento qui è centinaia di volte più fine, praticamente
/// invisibile anche al gradiente più ampio di cielo o pelle).
///
/// Convertito sempre da `image.to_rgb32f()` (non presuppone che l'input sia
/// già `ImageRgb32F`): innocuo se lo è già (nessuna perdita aggiuntiva), ma
/// rende questa funzione sicura da chiamare anche se un domani un percorso
/// diverso da `render_full_resolution_with_look` la richiamasse con
/// un'immagine 8 bit.
fn encode_master_as_tiff16(image: &image::DynamicImage) -> Result<Vec<u8>, EngineError> {
    let rgb32f = image.to_rgb32f();
    let (width, height) = rgb32f.dimensions();
    let mut pixels_u16 = Vec::with_capacity(rgb32f.as_raw().len());
    for &v in rgb32f.as_raw() {
        pixels_u16.push((v.clamp(0.0, 1.0) * u16::MAX as f32).round() as u16);
    }
    let rgb16 = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(width, height, pixels_u16).ok_or_else(
        || EngineError::RawFileError {
            reason: "dimensioni non valide per il buffer del master TIFF".to_string(),
        },
    )?;

    let mut tiff_bytes = Vec::new();
    image::DynamicImage::ImageRgb16(rgb16)
        .write_to(&mut std::io::Cursor::new(&mut tiff_bytes), image::ImageFormat::Tiff)
        .map_err(|e| EngineError::RawFileError {
            reason: e.to_string(),
        })?;
    Ok(tiff_bytes)
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
///
/// Per i file RAW, `raw_decode::decode_raw_preview` applica già da sola la
/// correzione di orientamento EXIF (la legge dai metadati che `rawler` ha già
/// interpretato). Per una foto già sviluppata (JPEG/PNG), invece, la libreria
/// `image` decodifica i pixel così come sono memorizzati SENZA applicare
/// l'orientamento — bug reale segnalato dall'utente con foto vere
/// ("l'orientamento è completamente sballato"): qui lo leggiamo da soli con
/// `kamadak-exif` (sugli stessi bytes originali, non sull'immagine già
/// decodificata: il tag vive nei metadati del file, non nei pixel) e
/// applichiamo la stessa correzione di `raw_decode::apply_exif_orientation`.
fn decode_any_photo(bytes: &[u8], file_name: &str) -> Result<image::DynamicImage, EngineError> {
    if raw_decode::has_known_raw_extension(file_name) {
        let preview = raw_decode::decode_raw_preview(bytes).map_err(|e| EngineError::RawFileError {
            reason: e.to_string(),
        })?;
        Ok(preview.image)
    } else {
        let image = image::load_from_memory(bytes).map_err(|e| EngineError::DecodeError {
            reason: e.to_string(),
        })?;
        let orientation = read_exif_orientation(bytes);
        Ok(raw_decode::apply_exif_orientation(image, orientation))
    }
}

/// Come [`decode_any_photo`], ma per il file che verrà davvero consegnato
/// all'utente (JPEG di esportazione + master TIFF), non per un'analisi/
/// anteprima: per un file RAW vero esegue il demosaic COMPLETO
/// (`raw_decode::decode_raw_full`, algoritmo PPG sul sensore Bayer reale —
/// vedi il commento esteso lì per il perché) invece di limitarsi
/// all'anteprima incorporata dalla fotocamera. Per una foto già sviluppata
/// (JPEG/PNG) non cambia nulla rispetto a `decode_any_photo`: quei formati
/// non hanno un "sensore" da ri-demosaicizzare, i pixel del file SONO già la
/// piena risoluzione.
///
/// **Unico punto che la richiama**: `PhotoEditSession::new`, cioè quando
/// l'utente apre una foto per modificarla — non la foto campione di "Incolla
/// impostazioni" (`paste_look_from_sample` continua a usare
/// `decode_any_photo`, l'anteprima veloce: è sufficiente per estrarre
/// statistiche di tono/colore, e demosaicizzarla per intero raddoppierebbe
/// il costo di ogni singola foto del batch senza migliorare la qualità del
/// file consegnato, che dipende solo dal demosaic del TARGET).
fn decode_any_photo_full(bytes: &[u8], file_name: &str) -> Result<image::DynamicImage, EngineError> {
    if raw_decode::has_known_raw_extension(file_name) {
        let full = raw_decode::decode_raw_full(bytes).map_err(|e| EngineError::RawFileError {
            reason: e.to_string(),
        })?;
        Ok(full.image)
    } else {
        let image = image::load_from_memory(bytes).map_err(|e| EngineError::DecodeError {
            reason: e.to_string(),
        })?;
        let orientation = read_exif_orientation(bytes);
        Ok(raw_decode::apply_exif_orientation(image, orientation))
    }
}

/// Legge il tag EXIF Orientation (0x0112) direttamente dai bytes originali di
/// una foto già sviluppata (JPEG, e PNG dove presente — `kamadak-exif`
/// supporta entrambi i contenitori). Nessun tag presente, un file che non lo
/// supporta affatto, o bytes non validi: tutti gli stessi caso, `None` — mai
/// un errore né un panic, per non far fallire l'intera importazione solo
/// perché manca (o è illeggibile) un singolo metadato opzionale.
fn read_exif_orientation(bytes: &[u8]) -> Option<u16> {
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    field.value.get_uint(0).map(|v| v as u16)
}

/// Esito di "incolla impostazioni": l'anteprima della foto target già
/// renderizzata con il Look adattato, più il Look completo così come è stato
/// applicato (dopo l'adattamento intelligente) — non solo esposizione/
/// highlights/shadows: la UI lo usa anche come punto di partenza per il
/// pannello di editing manuale (§ "Develop"), che deve poter correggere
/// qualunque campo, non solo i tre toccati da Smart-Batch.
#[derive(uniffi::Record, Clone, Debug)]
pub struct AdaptedRenderFfi {
    pub rendered_preview_png_bytes: Vec<u8>,
    pub applied_look: HarmonicLookFfi,
}

/// Esito dell'esportazione a piena risoluzione: lo STESSO rendering `f32`
/// (`look_render::render_full_resolution_with_look`, calcolato una sola
/// volta) codificato in due formati — JPEG ad alta qualità per la consegna
/// pratica, e un master TIFF a 16 bit senza perdita da conservare. Vedi
/// `PhotoEditSession::render_full_resolution_export`.
#[derive(uniffi::Record, Clone, Debug)]
pub struct FullResolutionExportFfi {
    pub jpeg_bytes: Vec<u8>,
    pub master_tiff_bytes: Vec<u8>,
}

/// Dimensione massima (lato lungo) della copia ridotta che [`PhotoEditSession`]
/// tiene in cache per il rendering interattivo. Ogni modifica di uno slider
/// del pannello "Develop" richiama il rendering su QUESTA copia, non sulla
/// foto a piena risoluzione: a questa dimensione l'intera pipeline per-pixel
/// (`rayon`, CPU) gira in pochi millisecondi anche su foto da 24+ megapixel,
/// il che è ciò che rende possibile un feedback dal vivo mentre si trascina
/// uno slider invece di un rendering completo ad ogni rilascio.
const INTERACTIVE_PREVIEW_MAX_DIM: u32 = 1024;

fn downscale_for_interactive_preview(image: &image::DynamicImage) -> image::DynamicImage {
    if image.width() <= INTERACTIVE_PREVIEW_MAX_DIM && image.height() <= INTERACTIVE_PREVIEW_MAX_DIM {
        image.clone()
    } else {
        image.resize(
            INTERACTIVE_PREVIEW_MAX_DIM,
            INTERACTIVE_PREVIEW_MAX_DIM,
            image::imageops::FilterType::Triangle,
        )
    }
}

/// Esito del rendering interattivo: l'anteprima PNG più due frazioni (0.0..1.0)
/// di pixel ai limiti dinamici — "slider sicuri", pensata perché la UI possa
/// colorare lo slider corrente (esposizione/alte luci/bianchi per
/// `highlight_clip_fraction`, ombre/neri per `shadow_clip_fraction`) quando il
/// valore ATTUALE sta bruciando le luci o schiacciando le ombre. Calcolato
/// solo sul rendering appena prodotto, non su ogni possibile valore dello
/// slider (vedi `look_render::clipping_fractions`).
///
/// `luminance_histogram` (aggiunto in questo giro, richiesta esplicita
/// dell'utente: "aggiungi anche un istogramma a schermo") — 256 bin,
/// calcolato sullo STESSO rendering appena prodotto (`look_render::
/// luminance_histogram`), non ricalcolato lato Kotlin da un giro separato di
/// decodifica del PNG appena ricevuto: arriva già pronto ad ogni tick di
/// trascinamento di uno slider, sincronizzato per costruzione con quello che
/// l'utente vede a schermo.
#[derive(uniffi::Record, Clone, Debug)]
pub struct RenderedPreviewFfi {
    pub preview_png_bytes: Vec<u8>,
    pub shadow_clip_fraction: f32,
    pub highlight_clip_fraction: f32,
    pub luminance_histogram: Vec<u32>,
}

/// **Bug reale scoperto e corretto in questo giro**: segnalato dall'utente
/// come "rettangoli grigi" (i lastroni rettangolari della pavimentazione di
/// una foto vera, appiattiti senza più la loro texture/variazione tonale) e
/// "mancanza totale di contrasto" — misurato: il contrasto locale della
/// pavimentazione crollava di circa il 45% dopo "Incolla impostazioni", A
/// QUALUNQUE valore dello slider "Intensità adattamento". La causa:
/// `contrast` e `tone_curve` (a differenza di `exposure_ev`, `highlights` e
/// `shadows`, tutti e tre già tarati da chi chiama questa funzione in base
/// allo stesso slider) venivano presi sempre e solo dal valore LETTERALE
/// estratto dalla foto campione, per intero, quale che fosse la posizione
/// dello slider — che quindi, per questi due campi, non faceva assolutamente
/// nulla, in contraddizione con quanto promette la UI stessa ("0% =
/// impostazioni identiche alla foto campione, 100% = massimo adattamento
/// intelligente alla scena"): a 100% l'utente si aspetta MENO copiatura
/// letterale del campione, non la stessa identica copia di uno slider a 0%.
/// Se la foto campione ha una grana/palette scelta per un mood volutamente
/// piatto (tone curve che alza le ombre e abbassa le luci, contrasto
/// negativo), quella piattezza veniva trasferita in blocco sul target senza
/// alcun modo per l'utente di attenuarla — l'unico slider pensato apposta
/// per farlo (Intensità adattamento) non aveva alcun effetto su questi due
/// campi.
///
/// Corretto sfumando ANCHE `contrast` e `tone_curve` verso il loro valore
/// neutro (0 e curva identità, cioè "nessuna correzione") in proporzione a
/// `strength` — stesso principio, stessa direzione di come `exposure_ev`
/// viene già sfumato da chi chiama: più il cursore si sposta verso "massimo
/// adattamento", meno letteralmente viene copiata la grana del campione. A
/// `strength=0.0` il comportamento resta identico a prima (copia letterale
/// del campione, come promesso); a `strength=1.0` contrasto e tone curve
/// tornano completamente neutri, lasciando il target con la propria
/// tonalità originale invece di quella (potenzialmente molto piatta) del
/// campione. Modifica `adapted_base` sul posto; `original` è il Look
/// letterale da cui sfumare (di solito lo stato di `adapted_base` prima di
/// qualunque altra modifica, passato separatamente perché il chiamante può
/// già aver cambiato `adapted_base.exposure_ev` nel frattempo).
fn taper_contrast_and_tone_curve_toward_neutral(
    adapted_base: &mut core_types::HarmonicLook,
    original: &core_types::HarmonicLook,
    strength: f32,
) {
    let strength = strength.clamp(0.0, 1.0);
    adapted_base.contrast = (original.contrast as f32 * (1.0 - strength)).round() as i32;
    adapted_base.tone_curve = original
        .tone_curve
        .iter()
        .map(|&(x, y)| {
            let blended = x as f32 + (y as f32 - x as f32) * (1.0 - strength);
            (x, blended.round().clamp(0.0, 255.0) as u8)
        })
        .collect();
}

/// **Sesto bug reale, segnalato dall'utente dopo la correzione del
/// contrasto/tone-curve**: "la foto target ha una saturazione meno viva
/// rispetto alla foto sorgente... la tinta anche è parecchio diversa". Misura
/// su una foto vera: la chroma della zona sedili era già ben recuperata (94%
/// dell'originale, vicina a quella del campione), ma l'hue restava quasi
/// invariato rispetto al target di partenza (11.5° contro gli 11.0°
/// originali) e lontanissimo da quello del campione (25.3°) — vedi il
/// commento esteso su `harmonic::hue_matching_deltas` per la causa esatta
/// (`hsl_hue` estratto dal campione è uno scarto relativo al proprio centro
/// di banda, non un valore assoluto verso cui il target deve convergere).
///
/// Chiamata DOPO `smartbatch::apply_deltas` (che non tocca `hsl` — solo
/// esposizione/luci/ombre) per aggiungere, banda per banda, lo scostamento
/// di tonalità MISURATO fra campione e target, pesato da `strength` come
/// ogni altro delta adattivo di questa funzione: a 0.0 non cambia nulla (Look
/// letterale invariato, coerente con "0.0 = applica il Look letterale"), a
/// 1.0 applica per intero il delta (già clampato dal guardrail
/// `MAX_HUE_MATCH_DELTA` dentro `hue_matching_deltas`). Il clamp finale
/// (-100..100) è lo stesso range del campo `hsl.hue` esposto allo slider
/// manuale in `look-render` — mai superarlo, qualunque sia la somma fra il
/// bias di stile già presente e questo nuovo delta di matching.
fn apply_hue_matching(
    look: &mut core_types::HarmonicLook,
    sample_image: &image::DynamicImage,
    target_image: &image::DynamicImage,
    strength: f32,
) {
    let strength = strength.clamp(0.0, 1.0);
    let sample_bands = harmonic::analyze_hue_bands(sample_image);
    let target_bands = harmonic::analyze_hue_bands(target_image);
    let deltas = harmonic::hue_matching_deltas(&sample_bands, &target_bands);
    for band in 0..deltas.len() {
        let weighted = (deltas[band] as f32 * strength).round() as i32;
        look.hsl.hue[band] = (look.hsl.hue[band] + weighted).clamp(-100, 100);
    }
}

/// Una foto "da modificare" aperta per l'editing, con la sua decodifica già
/// fatta e cacheiata in memoria (RAW-aware, via [`decode_any_photo`]) —
/// un oggetto UniFFI vero e proprio (non solo funzioni), perché a differenza
/// di `extract_look_from_reference_image`/`paste_look_onto_target_photo`
/// (chiamate una tantum, su un click) questa sessione viene interrogata
/// decine di volte al secondo mentre l'utente trascina uno slider del
/// pannello "Develop": decodificare di nuovo il file ad ogni chiamata (come
/// faceva la precedente `render_look_on_photo`, rimossa) sarebbe stato il
/// collo di bottiglia principale, oltre a dover ritrasmettere i bytes
/// dell'intera foto attraverso il confine Kotlin/JNI ad ogni tick di
/// trascinamento invece che una volta sola all'apertura.
///
/// Tiene DUE copie decodificate: `full_res` — per un RAW vero, il demosaic
/// COMPLETO del sensore (`decode_any_photo_full`, non più solo l'anteprima
/// incorporata dalla fotocamera: cambio architetturale di questo giro,
/// richiesto esplicitamente dall'utente per un uso editoriale) — e
/// `interactive_preview` (ridotta a [`INTERACTIVE_PREVIEW_MAX_DIM`] a
/// partire dalla STESSA immagine demosaicizzata, non da un'anteprima
/// potenzialmente diversa: quello che l'utente vede mentre modifica è così
/// garantito coerente con quello che riceverà) per il rendering dal vivo
/// mentre si modifica.
#[derive(uniffi::Object)]
pub struct PhotoEditSession {
    full_res: image::DynamicImage,
    interactive_preview: image::DynamicImage,
}

#[uniffi::export]
impl PhotoEditSession {
    /// Apre `target_bytes` per l'editing: decodifica una sola volta (RAW-aware,
    /// demosaic completo per un RAW vero) e prepara la copia ridotta per il
    /// rendering interattivo. Va chiamata quando l'utente importa/cambia la
    /// foto da modificare, non ad ogni modifica di uno slider.
    ///
    /// **Nota sulle prestazioni**: per un RAW vero questa chiamata ora
    /// esegue un demosaic completo (algoritmo PPG su tutta la risoluzione
    /// del sensore) invece di leggere solo l'anteprima JPEG incorporata —
    /// più lenta della versione precedente, ma UNA sola volta per foto
    /// (qui, non ad ogni tick di uno slider): il costo che serve pagare per
    /// smettere di consegnare all'utente un JPEG di seconda generazione
    /// generato dalla fotocamera stessa.
    #[uniffi::constructor]
    pub fn new(target_bytes: Vec<u8>, target_file_name: String) -> Result<Self, EngineError> {
        let full_res = decode_any_photo_full(&target_bytes, &target_file_name)?;
        let interactive_preview = downscale_for_interactive_preview(&full_res);
        Ok(Self { full_res, interactive_preview })
    }

    /// Rendering veloce per l'editing interattivo: lavora sulla copia ridotta
    /// cacheiata all'apertura, non ri-decodifica nulla. È il metodo chiamato
    /// ad ogni singolo tick di trascinamento di uno slider del pannello
    /// "Develop" — deve restare economico.
    pub fn render_preview(&self, look: HarmonicLookFfi) -> Result<RenderedPreviewFfi, EngineError> {
        let core_look: core_types::HarmonicLook = look.into();
        let rendered = look_render::render_preview_with_look(&self.interactive_preview, &core_look);
        let (shadow_clip_fraction, highlight_clip_fraction) = look_render::clipping_fractions(&rendered);
        let luminance_histogram = look_render::luminance_histogram(&rendered).to_vec();
        let preview_png_bytes = encode_preview_as_png(&rendered)?;
        Ok(RenderedPreviewFfi {
            preview_png_bytes,
            shadow_clip_fraction,
            highlight_clip_fraction,
            luminance_histogram,
        })
    }

    /// Restituisce la foto attualmente aperta in editing, con il Look
    /// CORRENTE (passato da chi chiama — di solito lo stato attuale dei
    /// controlli del pannello "Develop") già applicato, codificata come PNG
    /// in memoria — pensata per essere passata come `sample_bytes` (con
    /// `sample_file_name` una stringa qualunque terminante in `.png`, così
    /// `decode_any_photo` non la scambia per un file RAW) a
    /// `paste_look_from_sample`, chiamato su ALTRE sessioni di editing
    /// durante un batch.
    ///
    /// **Perché serve** (richiesta esplicita dell'utente: "trova un modo per
    /// creare la foto di riferimento da uno scatto raw editato direttamente
    /// in app"): prima di questo metodo, la foto "campione" per la Sintesi
    /// Armonica/Smart-Batch poteva venire SOLO da un file scelto da disco —
    /// non c'era modo di usare come riferimento uno scatto RAW che l'utente
    /// aveva già aperto e modificato manualmente in questa stessa sessione.
    /// Questo metodo chiude quel buco: renderizza lo stato attuale (Look
    /// applicato) e restituisce bytes pronti per rientrare, invariati, nello
    /// stesso punto d'ingresso già usato per un file esterno — nessuna nuova
    /// via di estrazione del Look da mantenere in parallelo.
    ///
    /// PNG, non JPEG: l'estrazione del Look legge istogrammi e bande di
    /// tonalità (`harmonic::extract_look_from_reference`,
    /// `analyze_hue_bands`) — una compressione con perdita introdurrebbe
    /// proprio negli istogrammi/bande che quell'analisi misura artefatti di
    /// blocco/colore che non esistono nel rendering originale, per un
    /// beneficio (dimensione del file) che qui non serve: questi bytes non
    /// vengono mai scritti su disco né mostrati, solo ridecodificati subito
    /// dopo da `paste_look_from_sample`.
    ///
    /// Lavora sulla copia ridotta (`interactive_preview`), non su
    /// `full_res`: l'estrazione del Look è una statistica sull'INTERA
    /// immagine (istogrammi, bande di tonalità), non un dettaglio
    /// pixel-per-pixel — la stessa approssimazione già accettata per il
    /// rendering interattivo (`INTERACTIVE_PREVIEW_MAX_DIM`) vale anche qui,
    /// e mantiene questa operazione economica quanto un `render_preview`
    /// (chiamabile su un click, non solo in un contesto batch offline), non
    /// quanto un export a piena risoluzione.
    pub fn export_current_edit_as_sample_png(&self, look: HarmonicLookFfi) -> Result<Vec<u8>, EngineError> {
        let core_look: core_types::HarmonicLook = look.into();
        let rendered = look_render::render_preview_with_look(&self.interactive_preview, &core_look);
        encode_preview_as_png(&rendered)
    }

    /// Rendering a piena risoluzione — per un RAW vero, dal demosaic COMPLETO
    /// del sensore (`self.full_res`, non più solo l'anteprima incorporata),
    /// da usare solo per l'esportazione finale: più lento, non va richiamato
    /// ad ogni modifica. Renderizza UNA sola volta con
    /// `look_render::render_full_resolution_with_look` (pipeline `f32`
    /// esclusiva, nessuna quantizzazione a 8 bit fino a qui) e incodifica il
    /// risultato in DUE formati dallo stesso rendering: JPEG ad alta qualità
    /// per la consegna (`encode_full_resolution_as_jpeg`) e un master TIFF a
    /// 16 bit senza perdita (`encode_master_as_tiff16`) — richiesta esplicita
    /// dell'utente per un uso editoriale ("non è ammessa la minima
    /// imperfezione"), accanto al JPEG, non al suo posto: il JPEG resta
    /// comunque il file pratico da consegnare/condividere, il TIFF è
    /// l'originale sviluppato da conservare.
    pub fn render_full_resolution_export(&self, look: HarmonicLookFfi) -> Result<FullResolutionExportFfi, EngineError> {
        let core_look: core_types::HarmonicLook = look.into();
        let rendered = look_render::render_full_resolution_with_look(&self.full_res, &core_look);
        let jpeg_bytes = encode_full_resolution_as_jpeg(&rendered)?;
        let master_tiff_bytes = encode_master_as_tiff16(&rendered)?;
        Ok(FullResolutionExportFfi {
            jpeg_bytes,
            master_tiff_bytes,
        })
    }

    /// "Incolla le impostazioni" ma sulla scena già decodificata e cacheiata
    /// di questa sessione — stesso algoritmo di adattamento della precedente
    /// `paste_look_onto_target_photo` (rimossa, sostituita da questo metodo):
    /// estrae il Look dalla foto campione (Sintesi Armonica, §4.1), calcola i
    /// descrittori di scena di campione e target (quest'ultimo dalla copia
    /// ridotta, non dalla foto intera — un'approssimazione già accettata
    /// altrove nel motore, vedi `ANALYSIS_MAX_DIM` in `harmonic`), i delta
    /// adattivi con i guardrail dell'architettura (§4.2), li applica al Look
    /// di base e renderizza subito l'anteprima veloce.
    ///
    /// `override_strength` (0.0..1.0) è lo slider "Intensità adattamento"
    /// della UI: 0.0 = applica il Look letterale (nessun adattamento), 1.0 =
    /// applica il massimo adattamento consentito dai guardrail. L'esposizione
    /// assoluta del campione viene interpolata con `(1 - override_strength)`
    /// prima di sommare il delta di Smart-Batch — vedi la nota storica su
    /// questo stesso punto più sotto nei test, che riproducono il bug
    /// originale (-1.09 EV) e verificano che non si sia ripresentato.
    pub fn paste_look_from_sample(
        &self,
        sample_bytes: Vec<u8>,
        sample_file_name: String,
        look_name: String,
        override_strength: f32,
    ) -> Result<AdaptedRenderFfi, EngineError> {
        let sample_image = decode_any_photo(&sample_bytes, &sample_file_name)?;
        let base_look = harmonic::extract_look_from_reference(&sample_image, &look_name);

        let sample_descriptors =
            smartbatch::compute_scene_descriptors(&look_render::luminance_histogram(&sample_image));
        let target_descriptors =
            smartbatch::compute_scene_descriptors(&look_render::luminance_histogram(&self.interactive_preview));

        let clamped_strength = override_strength.clamp(0.0, 1.0);
        let params = smartbatch::AdaptationParams {
            override_strength: clamped_strength,
            ..smartbatch::AdaptationParams::default()
        };
        let deltas = smartbatch::compute_adaptive_deltas(&sample_descriptors, &target_descriptors, &params);

        let mut adapted_base = base_look.clone();
        adapted_base.exposure_ev = base_look.exposure_ev * (1.0 - clamped_strength);
        taper_contrast_and_tone_curve_toward_neutral(&mut adapted_base, &base_look, clamped_strength);
        let mut adapted_look = smartbatch::apply_deltas(&adapted_base, &deltas);
        apply_hue_matching(&mut adapted_look, &sample_image, &self.interactive_preview, clamped_strength);

        let rendered = look_render::render_preview_with_look(&self.interactive_preview, &adapted_look);
        let rendered_preview_png_bytes = encode_preview_as_png(&rendered)?;

        Ok(AdaptedRenderFfi {
            rendered_preview_png_bytes,
            applied_look: adapted_look.into(),
        })
    }
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

    #[test]
    fn saliency_preview_reports_error_on_bad_bytes() {
        let result = compute_subject_saliency_preview(vec![0, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn saliency_preview_returns_a_valid_grayscale_png_at_analysis_resolution() {
        // `image::DynamicImage::resize` riscala per STARE DENTRO 512x512
        // preservando l'aspect ratio — anche ingrandendo se la sorgente è più
        // piccola (qui: un quadrato piccolo diventa 512x512, non resta 20x20).
        let bytes = png_bytes_of_solid_color(20, [180, 60, 60]);
        let result = compute_subject_saliency_preview(bytes).unwrap();
        assert!(!result.is_empty());
        let decoded = image::load_from_memory(&result).unwrap();
        assert_eq!(decoded.width(), 512);
        assert_eq!(decoded.height(), 512);
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
        original.texture_fine = 40;
        original.texture_medium = -25;
        original.texture_coarse = 10;
        original.white_balance_b = core_types::WhiteBalance { temp: 3200, tint: 12 };
        original.wb_gradient_enabled = true;
        original.wb_gradient_vertical = false;
        original.wb_gradient_position = 65;
        original.wb_gradient_spread = 20;
        original.noise_reduction_luma = 35;
        original.noise_reduction_color = 60;
        original.subject_mask = core_types::SubjectMask {
            enabled: true,
            target: core_types::MaskTarget::Background,
            exposure_ev: -0.8,
            contrast: 22,
            saturation: -30,
        };

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
        assert_eq!(round_tripped.texture_fine, original.texture_fine);
        assert_eq!(round_tripped.texture_medium, original.texture_medium);
        assert_eq!(round_tripped.texture_coarse, original.texture_coarse);
        assert_eq!(round_tripped.white_balance_b.temp, original.white_balance_b.temp);
        assert_eq!(round_tripped.white_balance_b.tint, original.white_balance_b.tint);
        assert_eq!(round_tripped.wb_gradient_enabled, original.wb_gradient_enabled);
        assert_eq!(round_tripped.wb_gradient_vertical, original.wb_gradient_vertical);
        assert_eq!(round_tripped.wb_gradient_position, original.wb_gradient_position);
        assert_eq!(round_tripped.wb_gradient_spread, original.wb_gradient_spread);
        assert_eq!(round_tripped.noise_reduction_luma, original.noise_reduction_luma);
        assert_eq!(round_tripped.noise_reduction_color, original.noise_reduction_color);
        assert_eq!(round_tripped.subject_mask.enabled, original.subject_mask.enabled);
        assert_eq!(round_tripped.subject_mask.target, original.subject_mask.target);
        assert_eq!(round_tripped.subject_mask.exposure_ev, original.subject_mask.exposure_ev);
        assert_eq!(round_tripped.subject_mask.contrast, original.subject_mask.contrast);
        assert_eq!(round_tripped.subject_mask.saturation, original.subject_mask.saturation);
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
    fn photo_edit_session_open_reports_error_on_bad_bytes() {
        let result = PhotoEditSession::new(vec![9, 9, 9], "x.png".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn paste_look_from_sample_renders_and_reports_positive_recovery_on_darker_target() {
        let sample_bytes = png_bytes_of_solid_color(6, [200, 150, 100]);
        let target_bytes = png_bytes_of_solid_color(6, [20, 20, 20]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();

        let result = session
            .paste_look_from_sample(sample_bytes, "campione.png".to_string(), "Campione".to_string(), 1.0)
            .unwrap();

        assert!(!result.rendered_preview_png_bytes.is_empty());
        // Il target è molto più scuro del campione: ci aspettiamo un recupero
        // di esposizione positivo, entro i guardrail testati in `smartbatch`.
        assert!(
            result.applied_look.exposure_ev > 0.0,
            "atteso recupero positivo, got {}",
            result.applied_look.exposure_ev
        );
        assert!(
            result.applied_look.exposure_ev <= 0.5 + f32::EPSILON,
            "esposizione applicata fuori dal guardrail Smart-Batch (max 0.5 EV): {}",
            result.applied_look.exposure_ev
        );
    }

    #[test]
    fn paste_look_from_sample_does_not_force_large_exposure_shift_when_target_matches_reference_scene() {
        // Riproduce il bug storico segnalato dall'utente: campione scattato ed
        // editato in basso-chiave (scuro), target la STESSA scena non editata
        // (qui approssimata da un colore identico, la stessa condizione che
        // rendeva l'istogramma di scena del target praticamente identico a
        // quello del campione). L'esposizione ASSOLUTA del campione (p50
        // basso -> exposure_ev fortemente negativo in `harmonic`) non deve
        // dominare il risultato quando la scena del target è già simile a
        // quella del campione: Smart-Batch calcola in quel caso un delta
        // vicino a zero, e l'esposizione finale deve restare dentro al
        // guardrail (default +-0.5 EV), non ereditare per intero la
        // luminosità assoluta -- spesso puramente stilistica -- della foto
        // campione (il caso reale riportato: "-1.09 EV", risultato scurito e
        // desaturato non voluto).
        let sample_bytes = png_bytes_of_solid_color(6, [20, 20, 20]);
        let target_bytes = png_bytes_of_solid_color(6, [20, 20, 20]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();

        let result = session
            .paste_look_from_sample(sample_bytes, "campione_scuro.png".to_string(), "Look Scuro".to_string(), 1.0)
            .unwrap();

        assert!(
            result.applied_look.exposure_ev.abs() <= 0.5 + f32::EPSILON,
            "esposizione applicata fuori dal guardrail Smart-Batch (max 0.5 EV): {}",
            result.applied_look.exposure_ev
        );
    }

    #[test]
    fn taper_contrast_and_tone_curve_leaves_look_literal_at_zero_strength() {
        let mut original = core_types::HarmonicLook::default();
        original.contrast = -60;
        original.tone_curve = vec![(0, 0), (64, 100), (128, 128), (192, 140), (255, 255)];
        let mut adapted = original.clone();

        taper_contrast_and_tone_curve_toward_neutral(&mut adapted, &original, 0.0);

        assert_eq!(adapted.contrast, original.contrast, "a intensità 0 il contrasto deve restare quello letterale del campione");
        assert_eq!(adapted.tone_curve, original.tone_curve, "a intensità 0 la tone curve deve restare quella letterale del campione");
    }

    #[test]
    fn taper_contrast_and_tone_curve_becomes_fully_neutral_at_max_strength() {
        // Riproduce il bug reale segnalato dall'utente ("rettangoli grigi",
        // "mancanza totale di contrasto"): a intensità massima, contrasto e
        // tone curve non devono più portare NULLA del valore piatto/letterale
        // estratto dal campione — deve restare solo la tonalità originale
        // del target (contrasto neutro, curva identità x == y per ogni punto).
        let mut original = core_types::HarmonicLook::default();
        original.contrast = -60;
        original.tone_curve = vec![(0, 0), (64, 100), (128, 128), (192, 140), (255, 255)];
        let mut adapted = original.clone();

        taper_contrast_and_tone_curve_toward_neutral(&mut adapted, &original, 1.0);

        assert_eq!(adapted.contrast, 0, "a intensità 1.0 il contrasto deve azzerarsi (neutro)");
        for &(x, y) in &adapted.tone_curve {
            assert_eq!(y, x, "a intensità 1.0 ogni punto della tone curve deve tornare all'identità (x=y): punto ({x},{y})");
        }
    }

    #[test]
    fn taper_contrast_and_tone_curve_is_a_partial_blend_at_half_strength() {
        let mut original = core_types::HarmonicLook::default();
        original.contrast = -60;
        original.tone_curve = vec![(0, 0), (128, 178), (255, 255)]; // punto 128 -> 178, deviazione di +50 dall'identità
        let mut adapted = original.clone();

        taper_contrast_and_tone_curve_toward_neutral(&mut adapted, &original, 0.5);

        assert_eq!(adapted.contrast, -30, "a metà intensità il contrasto deve dimezzarsi verso lo zero");
        let mid_point = adapted.tone_curve.iter().find(|&&(x, _)| x == 128).unwrap();
        assert_eq!(mid_point.1, 153, "a metà intensità la deviazione dall'identità (+50) deve dimezzarsi: atteso 128+25=153");
    }

    #[test]
    fn paste_look_from_sample_flattens_less_at_full_adaptation_strength_than_at_zero() {
        // End-to-end: una foto campione con un forte gradiente verticale (non
        // tinta unita) produce dalla Sintesi Armonica un contrasto/tone curve
        // non banali da estrarre. Il contrasto (in valore assoluto) applicato
        // al target con intensità di adattamento MASSIMA (1.0) non deve mai
        // superare quello applicato con intensità ZERO (copia letterale) —
        // altrimenti il fix sopra non sarebbe collegato a "Incolla
        // impostazioni".
        use image::{ImageBuffer, Rgba};
        let mut buf = Vec::new();
        let gradient = ImageBuffer::from_fn(32, 32, |_, y| {
            let v = (y * 255 / 31) as u8;
            Rgba([v, v, v, 255])
        });
        image::DynamicImage::ImageRgba8(gradient)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let sample_bytes = buf;
        let target_bytes = png_bytes_of_solid_color(6, [128, 128, 128]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();

        let at_zero = session
            .paste_look_from_sample(sample_bytes.clone(), "gradiente.png".to_string(), "L".to_string(), 0.0)
            .unwrap();
        let at_max = session
            .paste_look_from_sample(sample_bytes, "gradiente.png".to_string(), "L".to_string(), 1.0)
            .unwrap();

        assert!(
            at_max.applied_look.contrast.abs() <= at_zero.applied_look.contrast.abs(),
            "il contrasto a intensità massima ({}) non deve superare in valore assoluto quello a intensità zero ({})",
            at_max.applied_look.contrast,
            at_zero.applied_look.contrast
        );
    }

    fn png_bytes_of_solid_color_rect(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let mut buf = Vec::new();
        let img = ImageBuffer::from_fn(width, height, |_, _| Rgba([rgb[0], rgb[1], rgb[2], 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn apply_hue_matching_leaves_hue_untouched_at_zero_strength() {
        // Campione rosso-arancio (hue misurato più alto), target rosso puro
        // (hue misurato più basso): a intensità 0.0 nessun matching va
        // applicato, il Look resta letterale.
        let sample_img = image::load_from_memory(&png_bytes_of_solid_color_rect(8, 8, [230, 90, 20])).unwrap();
        let target_img = image::load_from_memory(&png_bytes_of_solid_color_rect(8, 8, [230, 20, 20])).unwrap();
        let mut look = core_types::HarmonicLook::default();
        let before = look.hsl.hue;

        apply_hue_matching(&mut look, &sample_img, &target_img, 0.0);

        assert_eq!(look.hsl.hue, before, "a intensità 0 l'hue non deve cambiare");
    }

    #[test]
    fn apply_hue_matching_shifts_hue_toward_the_sample_at_full_strength() {
        let sample_img = image::load_from_memory(&png_bytes_of_solid_color_rect(8, 8, [230, 90, 20])).unwrap();
        let target_img = image::load_from_memory(&png_bytes_of_solid_color_rect(8, 8, [230, 20, 20])).unwrap();
        let mut look = core_types::HarmonicLook::default();

        apply_hue_matching(&mut look, &sample_img, &target_img, 1.0);

        assert!(
            look.hsl.hue.iter().any(|&v| v != 0),
            "a intensità massima almeno una banda deve ricevere un delta di hue-matching, got {:?}",
            look.hsl.hue
        );
    }

    #[test]
    fn paste_look_from_sample_converges_target_hue_toward_sample_hue_at_full_strength() {
        // Riproduce (in miniatura) il bug reale segnalato dall'utente:
        // campione e target ritraggono lo "stesso soggetto" (stesso colore di
        // base, rosso) ma con una tinta diversa — il campione più arancio, il
        // target più puro. Prima di questo fix, `hsl.hue` restava lo scarto
        // RELATIVO del campione dal proprio centro banda, che non fa
        // convergere le due tinte. Dopo il fix, a intensità massima l'hue
        // effettivamente applicato al render deve avvicinare il target verso
        // il campione, non restare a zero.
        let sample_bytes = png_bytes_of_solid_color_rect(8, 8, [230, 90, 20]);
        let target_bytes = png_bytes_of_solid_color_rect(8, 8, [230, 20, 20]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();

        let result = session
            .paste_look_from_sample(sample_bytes, "campione.png".to_string(), "Look".to_string(), 1.0)
            .unwrap();

        assert!(
            result.applied_look.hsl_hue.iter().any(|&v| v != 0),
            "atteso un delta di hue-matching non nullo su almeno una banda, got {:?}",
            result.applied_look.hsl_hue
        );
    }

    #[test]
    fn paste_look_from_sample_reports_error_on_bad_sample_bytes() {
        let target_bytes = png_bytes_of_solid_color(6, [20, 20, 20]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let result = session.paste_look_from_sample(vec![1, 2, 3], "x.jpg".to_string(), "Look".to_string(), 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn render_preview_applies_manual_exposure_without_reextraction() {
        // Il pannello di editing manuale non ripassa mai dalla foto campione:
        // prende il Look corrente (qui costruito a mano, come farebbe uno
        // slider) e lo renderizza direttamente sulla sessione già aperta,
        // senza ri-decodificare il target.
        let target_bytes = png_bytes_of_solid_color(6, [100, 100, 100]);
        let session = PhotoEditSession::new(target_bytes.clone(), "target.png".to_string()).unwrap();
        let mut look = HarmonicLookFfi::from(core_types::HarmonicLook::default());
        look.exposure_ev = 1.0;

        let rendered = session.render_preview(look).unwrap();
        assert!(!rendered.preview_png_bytes.is_empty());

        let before = image::load_from_memory(&target_bytes).unwrap().to_rgba8();
        let after = image::load_from_memory(&rendered.preview_png_bytes).unwrap().to_rgba8();
        assert!(
            after.pixels().next().unwrap()[0] > before.pixels().next().unwrap()[0],
            "un'esposizione positiva manuale deve schiarire il pixel"
        );
    }

    #[test]
    fn render_full_resolution_uses_the_uncropped_original_size() {
        use image::GenericImageView;
        // La copia interattiva viene ridotta oltre INTERACTIVE_PREVIEW_MAX_DIM,
        // ma l'esportazione finale deve lavorare sulla foto originale intera.
        let big_size = INTERACTIVE_PREVIEW_MAX_DIM + 200;
        let target_bytes = png_bytes_of_solid_color(big_size, [80, 80, 80]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let look = HarmonicLookFfi::from(core_types::HarmonicLook::default());

        let full = session.render_full_resolution_export(look.clone()).unwrap();
        let full_dims = image::load_from_memory(&full.jpeg_bytes).unwrap().dimensions();
        assert_eq!(full_dims, (big_size, big_size), "il rendering a piena risoluzione deve preservare le dimensioni originali");
        let master_dims = image::load_from_memory(&full.master_tiff_bytes).unwrap().dimensions();
        assert_eq!(master_dims, (big_size, big_size), "anche il master TIFF deve preservare le dimensioni originali");

        let preview = session.render_preview(look).unwrap();
        let preview_dims = image::load_from_memory(&preview.preview_png_bytes).unwrap().dimensions();
        assert!(
            preview_dims.0 <= INTERACTIVE_PREVIEW_MAX_DIM && preview_dims.1 <= INTERACTIVE_PREVIEW_MAX_DIM,
            "l'anteprima interattiva deve restare entro il limite di downscale, got {:?}",
            preview_dims
        );
    }

    #[test]
    fn render_preview_reports_high_shadow_clip_fraction_for_a_crushed_black_image() {
        // "Slider sicuri": un'immagine quasi tutta nera, renderizzata con il
        // Look di default, deve riportare una shadow_clip_fraction alta e una
        // highlight_clip_fraction quasi nulla.
        let target_bytes = png_bytes_of_solid_color(6, [1, 1, 1]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let look = HarmonicLookFfi::from(core_types::HarmonicLook::default());

        let result = session.render_preview(look).unwrap();
        assert!(
            result.shadow_clip_fraction > 0.9,
            "atteso shadow_clip_fraction alto, got {}",
            result.shadow_clip_fraction
        );
        assert!(
            result.highlight_clip_fraction < 0.1,
            "atteso highlight_clip_fraction basso, got {}",
            result.highlight_clip_fraction
        );
    }

    #[test]
    fn render_preview_luminance_histogram_has_256_bins_summing_to_pixel_count() {
        // Nuovo in questo giro (richiesta esplicita dell'utente: "aggiungi
        // anche un istogramma a schermo"): l'istogramma restituito da
        // `render_preview` deve avere sempre 256 bin (una voce per livello di
        // luma 0..255) e la somma di tutti i bin deve corrispondere
        // esattamente al numero di pixel dell'anteprima renderizzata —
        // altrimenti la UI disegnerebbe un istogramma silenziosamente
        // incompleto o con pixel contati più volte.
        let target_bytes = png_bytes_of_solid_color(6, [128, 64, 32]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let look = HarmonicLookFfi::from(core_types::HarmonicLook::default());

        let result = session.render_preview(look).unwrap();
        assert_eq!(result.luminance_histogram.len(), 256);
        let total: u64 = result.luminance_histogram.iter().map(|&c| c as u64).sum();
        assert_eq!(total, 6 * 6, "la somma dei bin deve contare ogni pixel esattamente una volta");
    }

    // --- Orientamento EXIF (bug reale segnalato dall'utente con foto vere:
    // "l'orientamento è completamente sballato") + esportazione JPEG ad alta
    // qualità (bug reale segnalato dallo stesso utente: qualità pessima e
    // formato PNG invece di JPEG) ---

    /// Costruisce un JPEG valido (incodificato per davvero, non bytes finti)
    /// con un blocco APP1/EXIF minimale iniettato subito dopo il marcatore
    /// SOI — la posizione standard, la stessa in cui una vera fotocamera lo
    /// scrive — contenente UNA sola entry IFD: il tag Orientation (0x0112),
    /// tipo SHORT, valore `orientation`. Serve a testare `read_exif_orientation`
    /// e `decode_any_photo` contro un file realistico, non contro
    /// un'approssimazione della struttura EXIF.
    fn jpeg_bytes_with_exif_orientation(width: u32, height: u32, rgb: [u8; 3], orientation: u16) -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let mut base = Vec::new();
        let img = ImageBuffer::from_fn(width, height, |_, _| Rgba([rgb[0], rgb[1], rgb[2], 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut base), image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(&base[0..2], &[0xFF, 0xD8], "il JPEG di base deve iniziare con SOI");

        // Blocco TIFF minimale: header (byte order + magic + offset primo
        // IFD) + un IFD con una entry (Orientation) + offset "nessun altro
        // IFD" (0). Little-endian ("II"), per non dover gestire anche il
        // caso big-endian nel test.
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes()); // offset del primo (unico) IFD
        tiff.extend_from_slice(&1u16.to_le_bytes()); // numero di entry nell'IFD
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // tag: Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // tipo: SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count: 1
        let mut value_field = [0u8; 4];
        value_field[0..2].copy_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&value_field);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // offset del prossimo IFD: nessuno

        let mut app1_payload = Vec::new();
        app1_payload.extend_from_slice(b"Exif\0\0");
        app1_payload.extend_from_slice(&tiff);
        // La lunghezza del segmento (2 byte, big-endian: convenzione JPEG)
        // include se stessa, non il marcatore FF E1 che la precede.
        let segment_length = (app1_payload.len() + 2) as u16;

        let mut result = Vec::new();
        result.extend_from_slice(&[0xFF, 0xD8]); // SOI
        result.extend_from_slice(&[0xFF, 0xE1]); // APP1
        result.extend_from_slice(&segment_length.to_be_bytes());
        result.extend_from_slice(&app1_payload);
        result.extend_from_slice(&base[2..]); // resto del JPEG di base, SOI escluso
        result
    }

    #[test]
    fn read_exif_orientation_returns_none_when_no_exif_present() {
        let bytes = png_bytes_of_solid_color(4, [10, 20, 30]);
        assert_eq!(read_exif_orientation(&bytes), None);
    }

    #[test]
    fn read_exif_orientation_returns_none_on_garbage_bytes_not_panic() {
        assert_eq!(read_exif_orientation(&[1, 2, 3, 4, 5]), None);
        assert_eq!(read_exif_orientation(&[]), None);
    }

    #[test]
    fn read_exif_orientation_reads_the_real_tag_value_from_a_real_jpeg() {
        for value in [1u16, 2, 3, 4, 5, 6, 7, 8] {
            let bytes = jpeg_bytes_with_exif_orientation(4, 3, [128, 64, 32], value);
            assert_eq!(read_exif_orientation(&bytes), Some(value), "orientamento {value}");
        }
    }

    #[test]
    fn decode_any_photo_applies_exif_orientation_for_a_plain_jpeg() {
        // Bug reale: senza questa correzione, `decode_any_photo` per un
        // JPEG/PNG già sviluppato (a differenza del percorso RAW, che la
        // applica già) ignorava del tutto il tag Orientation. Qui si verifica
        // la catena INTERA (bytes -> lettura EXIF -> applicazione), non solo
        // la funzione geometrica isolata (già testata a parte in
        // `raw_decode`): un'immagine 3x2 con orientamento 6 deve arrivare
        // decodificata come 2x3 (dimensioni scambiate).
        use image::GenericImageView;
        let bytes = jpeg_bytes_with_exif_orientation(3, 2, [90, 140, 60], 6);
        let decoded = decode_any_photo(&bytes, "foto.jpg").unwrap();
        assert_eq!(decoded.dimensions(), (2, 3));
    }

    #[test]
    fn decode_any_photo_leaves_dimensions_untouched_without_an_orientation_tag() {
        use image::GenericImageView;
        // Nessuna regressione per il caso comune (foto senza tag Orientation,
        // o già orientamento 1): non deve succedere nulla.
        let bytes = png_bytes_of_solid_color_rect(5, 3, [200, 100, 50]);
        let decoded = decode_any_photo(&bytes, "foto.png").unwrap();
        assert_eq!(decoded.dimensions(), (5, 3));
    }

    #[test]
    fn encode_full_resolution_as_jpeg_produces_valid_decodable_bytes_preserving_dimensions() {
        use image::{GenericImageView, ImageBuffer, Rgba};
        let img = ImageBuffer::from_fn(10, 6, |_, _| Rgba([180u8, 90, 40, 255]));
        let dyn_img = image::DynamicImage::ImageRgba8(img);

        let jpeg_bytes = encode_full_resolution_as_jpeg(&dyn_img).unwrap();
        assert!(!jpeg_bytes.is_empty());
        assert_eq!(&jpeg_bytes[0..2], &[0xFF, 0xD8], "deve essere un JPEG vero (marcatore SOI)");

        let decoded = image::load_from_memory(&jpeg_bytes).unwrap();
        assert_eq!(decoded.dimensions(), (10, 6));
    }

    #[test]
    fn encode_master_as_tiff16_produces_valid_decodable_16bit_bytes_preserving_dimensions() {
        use image::{GenericImageView, ImageBuffer, Rgb};
        let img = ImageBuffer::from_fn(10, 6, |_, _| Rgb([0.7f32, 0.3, 0.1]));
        let dyn_img = image::DynamicImage::ImageRgb32F(img);

        let tiff_bytes = encode_master_as_tiff16(&dyn_img).unwrap();
        assert!(!tiff_bytes.is_empty());

        let decoded = image::load_from_memory_with_format(&tiff_bytes, image::ImageFormat::Tiff).unwrap();
        assert_eq!(decoded.dimensions(), (10, 6));
        assert!(matches!(decoded, image::DynamicImage::ImageRgb16(_)), "deve restare a 16 bit per canale");
    }

    #[test]
    fn decode_any_photo_full_applies_exif_orientation_for_a_plain_jpeg_just_like_decode_any_photo() {
        // Per un JPEG/PNG già sviluppato la logica è identica a
        // `decode_any_photo` (nessun sensore da ri-demosaicizzare) — questo
        // test lo verifica esplicitamente per la nuova funzione, invece di
        // presupporlo per somiglianza col nome.
        use image::GenericImageView;
        let bytes = jpeg_bytes_with_exif_orientation(3, 2, [90, 140, 60], 6);
        let decoded = decode_any_photo_full(&bytes, "foto.jpg").unwrap();
        assert_eq!(decoded.dimensions(), (2, 3));
    }

    #[test]
    fn decode_any_photo_full_leaves_dimensions_untouched_without_an_orientation_tag() {
        use image::GenericImageView;
        let bytes = png_bytes_of_solid_color_rect(5, 3, [200, 100, 50]);
        let decoded = decode_any_photo_full(&bytes, "foto.png").unwrap();
        assert_eq!(decoded.dimensions(), (5, 3));
    }

    #[test]
    fn render_full_resolution_export_returns_jpeg_bytes_not_png() {
        // L'esportazione a piena risoluzione (pulsante "Esporta" e batch)
        // deve produrre JPEG — richiesta esplicita dell'utente, prima
        // produceva PNG. `render_preview` (l'anteprima interattiva, mai
        // salvata su disco) resta invece PNG: non è cambiata e non deve
        // esserlo.
        let target_bytes = png_bytes_of_solid_color(6, [80, 80, 80]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let look = HarmonicLookFfi::from(core_types::HarmonicLook::default());

        let full = session.render_full_resolution_export(look).unwrap();
        assert_eq!(
            &full.jpeg_bytes[0..2],
            &[0xFF, 0xD8],
            "render_full_resolution_export deve produrre JPEG (SOI) in jpeg_bytes"
        );
    }

    #[test]
    fn render_full_resolution_export_master_tiff_is_a_valid_decodable_16bit_file() {
        // Il master TIFF va oltre "non è vuoto": deve essere un TIFF vero
        // (magic number "II*\0" o "MM\0*"), decodificabile, e a 16 bit per
        // canale — non 8, altrimenti non offrirebbe alcun vantaggio di
        // precisione sul JPEG accanto a cui viene consegnato.
        let target_bytes = png_bytes_of_solid_color(6, [80, 120, 200]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let look = HarmonicLookFfi::from(core_types::HarmonicLook::default());

        let full = session.render_full_resolution_export(look).unwrap();
        assert!(!full.master_tiff_bytes.is_empty());
        let is_little_endian_tiff = &full.master_tiff_bytes[0..4] == b"II*\0";
        let is_big_endian_tiff = &full.master_tiff_bytes[0..4] == [0x4D, 0x4D, 0x00, 0x2A];
        assert!(
            is_little_endian_tiff || is_big_endian_tiff,
            "il master deve avere l'intestazione TIFF standard"
        );

        let decoded = image::load_from_memory_with_format(&full.master_tiff_bytes, image::ImageFormat::Tiff).unwrap();
        assert!(
            matches!(decoded, image::DynamicImage::ImageRgb16(_)),
            "il master TIFF deve essere a 16 bit per canale, non 8"
        );
    }

    #[test]
    fn render_full_resolution_export_jpeg_and_tiff_agree_on_color_within_rounding() {
        // Entrambi i file derivano dallo STESSO rendering f32: a meno
        // dell'arrotondamento (8 bit per il JPEG, 16 bit per il TIFF), i
        // colori devono corrispondere — non essere due render indipendenti
        // che potrebbero scostarsi.
        let target_bytes = png_bytes_of_solid_color(6, [80, 120, 200]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let mut look = HarmonicLookFfi::from(core_types::HarmonicLook::default());
        look.exposure_ev = 0.4;

        let full = session.render_full_resolution_export(look).unwrap();
        let jpeg_px = image::load_from_memory(&full.jpeg_bytes).unwrap().to_rgb8().get_pixel(0, 0).0;
        let tiff_px = image::load_from_memory_with_format(&full.master_tiff_bytes, image::ImageFormat::Tiff)
            .unwrap()
            .to_rgb8()
            .get_pixel(0, 0)
            .0;
        for c in 0..3 {
            let diff = (jpeg_px[c] as i32 - tiff_px[c] as i32).abs();
            assert!(diff <= 2, "canale {c}: JPEG={} TIFF={} (troppo distanti per essere lo stesso rendering)", jpeg_px[c], tiff_px[c]);
        }
    }

    #[test]
    fn tonal_mask_curve_ffi_returns_256_weights_matching_look_render_for_every_kind() {
        // La funzione FFI non deve riscrivere la formula di maschera: deve
        // solo convertire l'enum ed esporre `look_render::tonal_mask_curve`
        // così com'è — nessuna logica duplicata che potrebbe scollegarsi dal
        // rendering reale in una modifica futura.
        for (kind, expected) in [
            (TonalMaskKindFfi::Shadows, look_render::TonalMaskKind::Shadows),
            (TonalMaskKindFfi::Highlights, look_render::TonalMaskKind::Highlights),
            (TonalMaskKindFfi::Blacks, look_render::TonalMaskKind::Blacks),
            (TonalMaskKindFfi::Whites, look_render::TonalMaskKind::Whites),
        ] {
            let via_ffi = tonal_mask_curve(kind);
            let direct = look_render::tonal_mask_curve(expected);
            assert_eq!(via_ffi.len(), 256, "kind={kind:?}");
            for i in 0..256 {
                assert!(
                    (via_ffi[i] - direct[i]).abs() < 1e-6,
                    "kind={kind:?} i={i} via_ffi={} direct={}",
                    via_ffi[i],
                    direct[i]
                );
            }
        }
    }

    #[test]
    fn export_current_edit_as_sample_png_produces_a_valid_decodable_png_at_preview_size() {
        // Nuovo in questo giro (richiesta esplicita dell'utente: "trova un
        // modo per creare la foto di riferimento da uno scatto raw editato
        // direttamente in app"): i bytes restituiti devono essere un PNG
        // vero, decodificabile, alla dimensione della copia RIDOTTA usata
        // per l'editing interattivo (non a piena risoluzione — vedi il
        // commento esteso sul metodo: un campione serve solo a estrarre
        // statistiche di tono/colore).
        let target_bytes = png_bytes_of_solid_color(6, [10, 200, 50]);
        let session = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let mut look = HarmonicLookFfi::from(core_types::HarmonicLook::default());
        look.exposure_ev = 0.5;

        let sample_png = session.export_current_edit_as_sample_png(look).unwrap();
        let decoded = image::load_from_memory(&sample_png).unwrap();
        use image::GenericImageView;
        assert_eq!(decoded.dimensions(), (6, 6));
    }

    #[test]
    fn export_current_edit_as_sample_png_output_is_usable_as_a_paste_look_from_sample_input() {
        // La ragion d'essere del metodo: i bytes prodotti devono poter
        // rientrare SUBITO come `sample_bytes` in `paste_look_from_sample`,
        // esattamente come un file scelto da disco — nessun percorso
        // speciale da aggiungere altrove per farli accettare.
        let target_bytes = png_bytes_of_solid_color(6, [200, 150, 100]);
        let session = PhotoEditSession::new(target_bytes.clone(), "target.png".to_string()).unwrap();
        let look = HarmonicLookFfi::from(core_types::HarmonicLook::default());
        let sample_png = session.export_current_edit_as_sample_png(look).unwrap();

        let another_target = PhotoEditSession::new(target_bytes, "target.png".to_string()).unwrap();
        let result = another_target.paste_look_from_sample(
            sample_png,
            "campione_dalla_modifica.png".to_string(),
            "Look dalla modifica".to_string(),
            1.0,
        );
        assert!(result.is_ok(), "atteso ok, got {:?}", result.err());
    }
}
