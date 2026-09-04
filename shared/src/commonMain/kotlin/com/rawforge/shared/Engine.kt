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
     * "Incolla le impostazioni": prende il Look copiato dalla foto campione
     * (`sampleBytes`/`sampleFileName`) e lo applica alla foto da modificare
     * (`targetBytes`/`targetFileName`), adattandolo in modo intelligente alla
     * scena specifica di quest'ultima — Smart-Batch Contestuale
     * (docs/ARCHITECTURE.md, §4.2) — invece di applicarlo identico. Ritorna
     * l'anteprima già renderizzata insieme al Look completo effettivamente
     * applicato (`AdaptedPreview.appliedLook`): quest'ultimo è anche il punto
     * di partenza del pannello di editing manuale, che l'utente può poi
     * correggere a mano richiamando `renderLookOnTarget`. L'export come
     * preset `.xmp` (`extractLookAndExportXmp`, sulla sola foto campione)
     * resta un'azione separata e facoltativa.
     *
     * `overrideStrength` (0f..1f) è lo slider "Intensità adattamento": 0
     * applica il Look letterale (nessun adattamento), 1 applica il massimo
     * adattamento consentito dai guardrail del motore.
     */
    fun pasteLookOntoTarget(
        sampleBytes: ByteArray,
        sampleFileName: String,
        lookName: String,
        targetBytes: ByteArray,
        targetFileName: String,
        overrideStrength: Float,
    ): Result<AdaptedPreview>

    /**
     * Renderizza `look` (qualunque sia la sua origine: quello incollato da
     * Smart-Batch, o quello corrente del pannello di editing manuale dopo che
     * l'utente ha mosso uno slider) sulla foto `target` — senza rifare né
     * l'estrazione dalla foto campione né l'adattamento, è il passo veloce
     * richiamato a ogni modifica manuale. Non lancia mai eccezioni verso la
     * UI: un errore del motore diventa un `Result` fallito.
     */
    fun renderLookOnTarget(targetBytes: ByteArray, targetFileName: String, look: EditableLook): Result<ByteArray>
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
 * Un punto di controllo della tone curve (ingresso/uscita 0..255).
 * Controparte comune, solo tipi primitivi, di `TonePointFfi` (generato da
 * UniFFI) — vedi la nota su `EditableLook` per il perché di questa
 * duplicazione voluta.
 */
data class TonePoint(val x: Int, val y: Int)

/**
 * Controparte comune (solo tipi primitivi, niente tipi generati da UniFFI) di
 * `HarmonicLookFfi`. Esiste apposta perché un tipo generato da UniFFI vive
 * solo nelle copie platform-specific dei binding Kotlin (`androidMain`/
 * `desktopMain`): non può comparire nella firma di una funzione `expect` in
 * `commonMain` (vedi `pasteLookOntoTarget`/`renderLookOnTarget` sopra, e la
 * stessa nota già presente lato Rust su `paste_look_onto_target_photo`).
 * `EditableLook` è quindi lo stato che il pannello "Develop" della UI comune
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
)
