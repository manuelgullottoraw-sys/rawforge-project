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
#[derive(uniffi::Record, Clone, Debug)]
pub struct RenderedPreviewFfi {
    pub preview_png_bytes: Vec<u8>,
    pub shadow_clip_fraction: f32,
    pub highlight_clip_fraction: f32,
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
/// Tiene DUE copie decodificate: `full_res` (l'anteprima incorporata dalla
/// fotocamera per un RAW, o l'immagine originale per un JPEG/PNG — non
/// ancora un demosaic RAW completo, limite già noto) per l'esportazione
/// finale, e `interactive_preview` (ridotta a
/// [`INTERACTIVE_PREVIEW_MAX_DIM`]) per il rendering dal vivo mentre si
/// modifica.
#[derive(uniffi::Object)]
pub struct PhotoEditSession {
    full_res: image::DynamicImage,
    interactive_preview: image::DynamicImage,
}

#[uniffi::export]
impl PhotoEditSession {
    /// Apre `target_bytes` per l'editing: decodifica una sola volta (RAW-aware)
    /// e prepara la copia ridotta per il rendering interattivo. Va chiamata
    /// quando l'utente importa/cambia la foto da modificare, non ad ogni
    /// modifica di uno slider.
    #[uniffi::constructor]
    pub fn new(target_bytes: Vec<u8>, target_file_name: String) -> Result<Self, EngineError> {
        let full_res = decode_any_photo(&target_bytes, &target_file_name)?;
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
        let preview_png_bytes = encode_preview_as_png(&rendered)?;
        Ok(RenderedPreviewFfi {
            preview_png_bytes,
            shadow_clip_fraction,
            highlight_clip_fraction,
        })
    }

    /// Rendering a piena risoluzione (dell'anteprima incorporata originale,
    /// non ancora del RAW pieno — limite già noto), da usare solo per
    /// l'esportazione finale: più lento, non va richiamato ad ogni modifica.
    pub fn render_full_resolution(&self, look: HarmonicLookFfi) -> Result<Vec<u8>, EngineError> {
        let core_look: core_types::HarmonicLook = look.into();
        let rendered = look_render::render_preview_with_look(&self.full_res, &core_look);
        encode_preview_as_png(&rendered)
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
        let adapted_look = smartbatch::apply_deltas(&adapted_base, &deltas);

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

        let full = session.render_full_resolution(look.clone()).unwrap();
        let full_dims = image::load_from_memory(&full).unwrap().dimensions();
        assert_eq!(full_dims, (big_size, big_size), "il rendering a piena risoluzione deve preservare le dimensioni originali");

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
}
