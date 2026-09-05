package com.rawforge.shared

/**
 * Facciata comune verso il motore nativo (Rust) di RawForge.
 *
 * Le implementazioni Android e Desktop (vedi `Engine.android.kt` / `Engine.desktop.kt`)
 * chiamano davvero il motore Rust tramite i binding Kotlin generati da UniFFI
 * (`uniffi.rawforge_ffi.*`, prodotti dal crate `engine/ffi`), collegati alla libreria
 * nativa compilata per ciascuna piattaforma dalla pipeline CI. Vedi
 * `docs/ARCHITECTURE.md`, §1, §2 e §7.
 */
expect object Engine {
    /** Chiama `rawforge_ffi::engine_version()` — conferma che il collegamento nativo funziona. */
    fun versionInfo(): String

    /**
     * Chiama `rawforge_ffi::generate_lightroom_preset_xmp()` su un `HarmonicLook` di
     * esempio, dimostrando end-to-end la pipeline Sintesi Armonica -> export XMP
     * (docs/ARCHITECTURE.md, §4.1 e §5) attraverso il motore Rust reale.
     */
    fun generateSampleXmpPreset(): String

    /**
     * Importa una foto vera da bytes grezzi. Se `fileName` ha un'estensione RAW
     * nota (crate `raw-decode`, docs/ARCHITECTURE.md §2), decodifica l'anteprima
     * incorporata dalla fotocamera stessa; altrimenti tratta i byte come
     * un'immagine già sviluppata (JPEG/PNG). Non lancia mai eccezioni verso la UI:
     * un errore del motore (formato non riconosciuto, file corrotto) diventa un
     * `Result` fallito con il messaggio prodotto da Rust.
     */
    fun importPhoto(bytes: ByteArray, fileName: String): Result<ImportedPhoto>

    /**
     * Sintesi Armonica Automatica (docs/ARCHITECTURE.md, §4.1) sui bytes grezzi
     * originali di una foto (stesso rilevamento RAW-vs-già-sviluppata di
     * `importPhoto`), seguita subito dall'export come preset Lightroom `.xmp`
     * (§5) — dimostra la catena completa import -> analisi -> preset su un file
     * scelto dall'utente, non su un esempio fisso.
     */
    fun extractLookAndExportXmp(bytes: ByteArray, fileName: String, lookName: String): Result<String>

    /**
     * Apre la foto da modificare per l'editing: la decodifica una sola volta
     * (RAW-aware) e la mantiene cacheiata lato Rust (motore `PhotoEditSession`,
     * inclusa una copia ridotta apposta per il rendering interattivo) invece
     * di decodificarla di nuovo ad ogni singola modifica — è quello che rende
     * possibile un feedback dal vivo mentre si trascina uno slider del
     * pannello "Develop", non solo al rilascio. Va richiamata quando l'utente
     * importa/cambia la foto da modificare; la sessione risultante va chiusa
     * con `PhotoEditSession.close()` quando non serve più (una nuova foto
     * sostituisce la precedente, o l'app si chiude) per liberare la memoria
     * lato Rust.
     */
    fun openPhotoForEditing(bytes: ByteArray, fileName: String): Result<PhotoEditSession>

    /**
     * Anteprima ispezionabile del "probabile soggetto" (`compute_subject_saliency_preview`
     * lato Rust, `engine/README.md` per la spiegazione completa e i limiti
     * dichiarati dell'euristica): una mappa in scala di grigi (bianco = alta
     * salienza) alla stessa risoluzione di analisi usata dal motore, PENSATA
     * come mostra-e-basta — la sezione "Maschera Soggetto/Sfondo" del
     * pannello Develop (`EditableLook.subjectMask*`) è quella che usa
     * davvero questa stessa mappa lato motore per applicare regolazioni
     * locali, non questa funzione. `imageBytes` deve essere già
     * un'immagine decodificabile (JPEG/PNG) — la stessa `previewImageBytes`
     * di `ImportedPhoto`, mai i byte grezzi di un file RAW originale.
     */
    fun computeSubjectSaliencyPreview(imageBytes: ByteArray): Result<ByteArray>

    /**
     * Genera il testo di un preset Lightroom `.xmp` per un `EditableLook` già
     * pronto (`rawforge_ffi::generate_lightroom_preset_xmp`, la stessa
     * funzione dietro `generateSampleXmpPreset`/`extractLookAndExportXmp`),
     * SENZA ripartire dall'estrazione automatica su una foto — a differenza
     * di `extractLookAndExportXmp`, che estrae un Look da bytes grezzi,
     * questa riusa un Look già calcolato altrove (tipicamente
     * `AdaptedPreview.appliedLook` restituito da
     * `PhotoEditSession.pasteLookFromSample`). È la funzione dietro
     * l'elaborazione in batch (vedi `BatchExport`/`App.kt`): per ciascuna
     * foto target produce sia il rendering sia il preset, dallo stesso Look
     * adattato — mai due calcoli separati che potrebbero divergere.
     */
    fun generateXmpForLook(look: EditableLook): Result<String>

    /**
     * Curva (256 pesi, 0f..1f, uno per bin di luma — stessa convenzione di
     * bin di `RenderedPreview.luminanceHistogram`) di quanto lo slider tonale
     * `kind` modifica ciascuna fascia dell'istogramma
     * (`look_render::tonal_mask_curve` lato Rust: la STESSA funzione di
     * maschera usata dal rendering reale, non una sua riscrittura qui).
     * Indipendente dalla foto aperta — la UI la richiede una volta per
     * ciascuno dei quattro valori e la tiene in cache (vedi `tonalMaskCurves`
     * in `DevelopPanel`, `App.kt`), per disegnare sopra l'istogramma a
     * schermo quale parte lo slider attualmente trascinato sta modificando
     * (richiesta esplicita dell'utente: "aggiungi anche un istogramma a
     * schermo che evidenzia le parti che stai modificando mentre muovi lo
     * slider").
     */
    fun tonalMaskCurve(kind: TonalMaskKind): List<Float>
}

/**
 * Controparte comune di `uniffi.rawforge_ffi.TonalMaskKindFfi` — stessa
 * ragione di `MaskTarget` rispetto a `MaskTargetFfi`: quale dei quattro
 * slider tonali mascherati per zona (Ombre/Luci/Neri/Bianchi) la UI vuole
 * disegnare sopra l'istogramma a schermo.
 */
enum class TonalMaskKind { SHADOWS, HIGHLIGHTS, BLACKS, WHITES }

/**
 * Una foto aperta per l'editing (vedi `Engine.openPhotoForEditing`). Incapsula
 * la sessione nativa `PhotoEditSession` generata da UniFFI — che, come
 * `HarmonicLookFfi`, esiste solo nelle copie platform-specific dei binding e
 * non può quindi comparire direttamente in `commonMain` — dietro tipi comuni
 * (`EditableLook`, `ByteArray`). `close()` va chiamata esplicitamente quando
 * la sessione non serve più: non c'è un finalizer automatico lato Kotlin, la
 * memoria della foto decodificata resterebbe altrimenti allocata lato Rust.
 */
expect class PhotoEditSession {
    /**
     * "Incolla le impostazioni": prende il Look copiato dalla foto campione
     * (`sampleBytes`/`sampleFileName`) e lo applica alla scena già cacheiata
     * in questa sessione, adattandolo in modo intelligente — Smart-Batch
     * Contestuale (docs/ARCHITECTURE.md, §4.2) — invece di applicarlo
     * identico. Ritorna l'anteprima già renderizzata insieme al Look completo
     * effettivamente applicato (`AdaptedPreview.appliedLook`): quest'ultimo è
     * anche il punto di partenza del pannello di editing manuale, che
     * l'utente può poi correggere a mano (`renderPreview`). L'export come
     * preset `.xmp` (`Engine.extractLookAndExportXmp`, sulla sola foto
     * campione) resta un'azione separata e facoltativa.
     *
     * `overrideStrength` (0f..1f) è lo slider "Intensità adattamento": 0
     * applica il Look letterale (nessun adattamento), 1 applica il massimo
     * adattamento consentito dai guardrail del motore.
     */
    fun pasteLookFromSample(
        sampleBytes: ByteArray,
        sampleFileName: String,
        lookName: String,
        overrideStrength: Float,
    ): Result<AdaptedPreview>

    /**
     * Renderizza `look` sulla copia RIDOTTA cacheiata da questa sessione —
     * veloce apposta (niente ri-decodifica, niente pixel a piena
     * risoluzione): è il passo richiamato ad ogni singola modifica di uno
     * slider del pannello "Develop", pensato per un feedback dal vivo mentre
     * si trascina, non solo al rilascio. Il risultato include anche le
     * frazioni di clipping ombre/luci di QUESTA anteprima (vedi
     * `RenderedPreview`), per il feedback "slider sicuri".
     */
    fun renderPreview(look: EditableLook): Result<RenderedPreview>

    /**
     * Renderizza `look` sulla foto a piena risoluzione — per un RAW vero, dal
     * demosaic COMPLETO del sensore (cambio architetturale di questo giro:
     * prima si limitava all'anteprima incorporata dalla fotocamera, limite
     * ora eliminato) — più lento, va usato solo per l'esportazione finale del
     * risultato, non ad ogni modifica. Restituisce DUE file dallo stesso
     * rendering: un JPEG ad alta qualità pronto per la consegna/condivisione,
     * e un master TIFF a 16 bit per canale senza perdita da conservare —
     * richiesta esplicita dell'utente per un uso editoriale ("non è ammessa
     * la minima imperfezione").
     */
    fun renderFullResolutionExport(look: EditableLook): Result<FullResolutionExport>

    /**
     * Restituisce la foto attualmente aperta in questa sessione, con `look`
     * applicato, codificata come PNG in memoria — pensata per essere passata
     * subito come `sampleBytes`/`sampleFileName` (con un nome qualunque che
     * finisca in ".png") a `pasteLookFromSample` di un'ALTRA sessione,
     * durante un batch. Permette di usare come "foto campione" per la
     * Sintesi Armonica/Smart-Batch uno scatto RAW già aperto e modificato a
     * mano in questa stessa sessione, invece di richiedere sempre un file
     * scelto da disco (richiesta esplicita dell'utente: "trova un modo per
     * creare la foto di riferimento da uno scatto raw editato direttamente
     * in app"). Economica quanto `renderPreview` (lavora sulla stessa copia
     * ridotta), non quanto un export a piena risoluzione.
     */
    fun exportCurrentEditAsSamplePng(look: EditableLook): Result<ByteArray>

    /** Libera la foto decodificata cacheiata lato Rust. Va chiamata quando
     * questa sessione non serve più (nuova foto importata, o chiusura app). */
    fun close()
}

/**
 * Esito di un'importazione riuscita: bytes già pronti per essere decodificati
 * dalla UI come immagine (via `decodeImageBitmapOrNull`), più i metadati
 * camera quando disponibili (solo per i file RAW: un JPEG/PNG importato non
 * passa dal crate `raw-decode`, quindi qui restano `null`).
 */
data class ImportedPhoto(
    val fileName: String,
    val cameraMake: String?,
    val cameraModel: String?,
    val previewImageBytes: ByteArray,
)

/**
 * Esito di "incolla impostazioni": l'anteprima della foto target già
 * renderizzata con il Look adattato, più il Look completo effettivamente
 * applicato — punto di partenza del pannello di editing manuale.
 */
data class AdaptedPreview(
    val renderedImageBytes: ByteArray,
    val appliedLook: EditableLook,
)

/**
 * Esito di un rendering interattivo (`PhotoEditSession.renderPreview`):
 * l'anteprima renderizzata più due frazioni (0f..1f) di pixel ai limiti
 * dinamici di QUESTA anteprima — "slider sicuri" (una delle idee discusse e
 * approvate, vedi `README.md`): `highlightClipFraction` alta segnala luci
 * bruciate (rilevante per Esposizione/Alte luci/Bianchi),
 * `shadowClipFraction` alta segnala ombre schiacciate (Ombre/Neri). Calcolato
 * solo sul valore CORRENTE dello slider, non sull'intero range possibile —
 * ricalcolare per ogni valore richiederebbe ri-renderizzare l'immagine una
 * volta per posizione, troppo costoso per un feedback dal vivo.
 */
data class RenderedPreview(
    val imageBytes: ByteArray,
    val shadowClipFraction: Float,
    val highlightClipFraction: Float,
    /** Istogramma di luminanza a 256 bin di QUESTO rendering (vedi
     * `look_render::luminance_histogram` lato Rust) — aggiunto in questo
     * giro per l'istogramma a schermo del pannello Develop (richiesta
     * esplicita dell'utente), sincronizzato per costruzione con
     * `imageBytes` invece di essere ricalcolato lato Kotlin da un giro
     * separato di decodifica del PNG. */
    val luminanceHistogram: List<Int>,
)

/**
 * Esito di `PhotoEditSession.renderFullResolutionExport`: lo STESSO rendering
 * a piena risoluzione (pipeline `f32` esclusiva, nessuna quantizzazione a 8
 * bit prima di questo punto — vedi `look_render::render_full_resolution_with_look`
 * lato Rust) incodificato in due file — `jpegBytes` (JPEG ad alta qualità,
 * pronto per la consegna pratica) e `masterTiffBytes` (TIFF a 16 bit per
 * canale, senza perdita, da conservare come originale sviluppato).
 */
data class FullResolutionExport(
    val jpegBytes: ByteArray,
    val masterTiffBytes: ByteArray,
)

/**
 * Un punto di controllo della tone curve (ingresso/uscita 0..255).
 * Controparte comune, solo tipi primitivi, di `TonePointFfi` (generato da
 * UniFFI) — vedi la nota su `EditableLook` per il perché di questa
 * duplicazione voluta.
 */
data class TonePoint(val x: Int, val y: Int)

/**
 * Controparte comune (plain Kotlin, nessun tipo generato da UniFFI) di
 * `uniffi.rawforge_ffi.MaskTargetFfi` — stessa ragione di `EditableLook`
 * rispetto a `HarmonicLookFfi`: un tipo UniFFI non può comparire in una
 * firma `expect`/in `EditableLook`. La conversione avviene solo dentro
 * `toFfi()`/`toEditable()` in `Engine.desktop.kt`/`Engine.android.kt`.
 */
enum class MaskTarget { SUBJECT, BACKGROUND }

/**
 * Controparte comune (solo tipi primitivi, niente tipi generati da UniFFI) di
 * `HarmonicLookFfi`. Esiste apposta perché un tipo generato da UniFFI vive
 * solo nelle copie platform-specific dei binding Kotlin (`androidMain`/
 * `desktopMain`): non può comparire nella firma di una funzione `expect` in
 * `commonMain` (vedi `PhotoEditSession` sopra, e la stessa nota già presente
 * lato Rust sui metodi di `PhotoEditSession`). `EditableLook` è quindi lo
 * stato che il pannello "Develop" della UI comune
 * tiene in memoria (uno per slider) e che le implementazioni Android/Desktop
 * convertono avanti e indietro da/verso `HarmonicLookFfi` solo al proprio
 * interno, mai attraverso il confine `expect`/`actual`.
 */
data class EditableLook(
    val name: String = "RawForge Look",
    val whiteBalanceTemp: Int = 5500,
    val whiteBalanceTint: Int = 0,
    val exposureEv: Float = 0f,
    val contrast: Int = 0,
    val highlights: Int = 0,
    val shadows: Int = 0,
    val whites: Int = 0,
    val blacks: Int = 0,
    val vibrance: Int = 0,
    val saturation: Int = 0,
    val toneCurve: List<TonePoint> = listOf(
        TonePoint(0, 0),
        TonePoint(64, 64),
        TonePoint(128, 128),
        TonePoint(192, 192),
        TonePoint(255, 255),
    ),
    val hslHue: List<Int> = List(8) { 0 },
    val hslSat: List<Int> = List(8) { 0 },
    val hslLum: List<Int> = List(8) { 0 },
    val shadowHue: Int = 210,
    val shadowSat: Int = 0,
    val highlightHue: Int = 45,
    val highlightSat: Int = 0,
    val splitToningBalance: Int = 0,
    /** Texture per banda di frequenza (-100..100, 0 = nessun effetto). */
    val textureFine: Int = 0,
    val textureMedium: Int = 0,
    val textureCoarse: Int = 0,
    /** Zona B e parametri del bilanciamento del bianco a gradiente. */
    val whiteBalanceBTemp: Int = 5500,
    val whiteBalanceBTint: Int = 0,
    val wbGradientEnabled: Boolean = false,
    val wbGradientVertical: Boolean = true,
    val wbGradientPosition: Int = 50,
    val wbGradientSpread: Int = 50,
    /** Riduzione del rumore (0..100, 0 = nessun effetto): luminanza e colore,
     * sfocati separatamente in Lab con protezione ai bordi — vedi
     * `look-render::apply_noise_reduction` lato Rust. */
    val noiseReductionLuma: Int = 0,
    val noiseReductionColor: Int = 0,
    /**
     * Maschera automatica "Soggetto"/"Sfondo" (`look-render::apply_subject_mask`
     * lato Rust, derivata dalla stessa mappa di salienza di
     * `computeSubjectSaliencyPreview`): quando `subjectMaskEnabled` è vero,
     * esposizione/contrasto/saturazione locali si applicano SOLO sulla
     * regione scelta da `subjectMaskTarget`, in aggiunta alle stesse
     * regolazioni globali sopra. Disattivata di default (`false`): nessun
     * comportamento esistente cambia finché l'utente non la attiva.
     */
    val subjectMaskEnabled: Boolean = false,
    val subjectMaskTarget: MaskTarget = MaskTarget.SUBJECT,
    val subjectMaskExposureEv: Float = 0f,
    val subjectMaskContrast: Int = 0,
    val subjectMaskSaturation: Int = 0,
)
