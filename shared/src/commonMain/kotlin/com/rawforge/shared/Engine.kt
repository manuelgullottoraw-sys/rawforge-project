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
     * l'anteprima già renderizzata: tutto resta nell'app, l'export come
     * preset `.xmp` (`extractLookAndExportXmp`, sulla sola foto campione)
     * resta un'azione separata e facoltativa.
     *
     * `overrideStrength` (0f..1f) è lo slider "Override Strength": 0 applica
     * il Look letterale (nessun adattamento), 1 applica il massimo
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
 * renderizzata con il Look adattato, più i valori di esposizione/highlights/
 * shadows effettivamente applicati dopo l'adattamento — utile per mostrare in
 * UI cosa ha deciso lo Smart-Batch, non solo il risultato finale.
 */
data class AdaptedPreview(
    val renderedImageBytes: ByteArray,
    val appliedExposureEv: Float,
    val appliedHighlights: Int,
    val appliedShadows: Int,
)
