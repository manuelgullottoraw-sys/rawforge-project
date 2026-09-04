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
}

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
     * Renderizza `look` sulla foto a piena risoluzione (l'anteprima
     * incorporata originale, non ancora un demosaic RAW completo — limite
     * già noto) — più lento, va usato solo per l'esportazione finale del
     * risultato, non ad ogni modifica.
     */
    fun renderFullResolution(look: EditableLook): Result<ByteArray>

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
)
