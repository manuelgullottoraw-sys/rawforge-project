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
