package com.rawforge.shared

/**
 * Una foto trovata nella cartella Libreria — solo metadati leggeri, MAI i
 * bytes del file (la Libreria può contenere centinaia di file: leggerli
 * tutti solo per elencarli sarebbe inutilmente lento). `id` è un
 * identificatore opaco specifico di piattaforma (un path assoluto su
 * Desktop, un `content://` URI serializzato su Android) — la UI comune lo
 * tratta come una chiave, non come un percorso da interpretare, e lo passa
 * a `LibraryStorage.readPhotoBytes` solo quando l'utente sceglie davvero
 * quella foto.
 */
data class LibraryPhotoEntry(
    val id: String,
    val displayName: String,
    val sizeBytes: Long,
)

/**
 * Accesso alla cartella Libreria (docs/ARCHITECTURE.md: "la libreria a
 * griglia" prevista fra gli incrementi futuri, qui la prima versione).
 * Persistente fra riavvii dell'app — ogni implementazione di piattaforma
 * ricorda l'ULTIMA cartella scelta (`rememberFolder`/`rememberedFolder`) in
 * un posto che sopravvive alla chiusura del processo (preferenze JDK su
 * Desktop, `SharedPreferences` su Android), così la Libreria è già
 * popolata al prossimo avvio invece di richiedere la cartella ogni volta.
 *
 * **Onestà sui limiti di questa prima versione** (dichiarati, non
 * nascosti — stesso principio seguito in tutto questo progetto): (1)
 * nessuna cache miniature su disco — `LibraryScreen` ridecodifica
 * l'anteprima di ogni foto ad ogni apertura della schermata (accettabile
 * per l'uso interattivo su una cartella di dimensioni normali; da
 * rivedere se una cartella con migliaia di file risultasse lenta); (2)
 * una sola cartella alla volta, non una "collezione" di più cartelle o
 * sottocartelle annidate (`listPhotos` non è ricorsiva); (3) nessun
 * indicizzatore in background, nessun watch delle modifiche al
 * filesystem — la lista si aggiorna solo quando l'utente riapre la
 * schermata Libreria.
 */
expect object LibraryStorage {
    /** L'ultima cartella Libreria scelta (persistita fra riavvii), o `null` se nessuna. */
    fun rememberedFolder(): String?

    /** Ricorda `folderId` come cartella Libreria per le prossime aperture dell'app. */
    fun rememberFolder(folderId: String)

    /**
     * Elenca le foto (RAW o già sviluppate — stesso riconoscimento per
     * estensione di `raw_decode::has_known_raw_extension` lato Rust, più
     * JPEG/PNG) direttamente dentro `folderId`, senza scendere in
     * eventuali sottocartelle.
     */
    fun listPhotos(folderId: String): Result<List<LibraryPhotoEntry>>

    /** Legge i bytes grezzi di una foto della Libreria, dato il suo `id`. */
    fun readPhotoBytes(id: String): Result<ByteArray>
}
