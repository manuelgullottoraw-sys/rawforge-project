package com.rawforge.shared

/**
 * Scrive i file prodotti dall'elaborazione in batch (per ciascuna foto
 * target: la foto renderizzata in PNG E il preset Lightroom `.xmp` — scelta
 * esplicita dell'utente, "Entrambi") in una cartella di destinazione.
 * `folderId` è lo stesso tipo di identificatore opaco di `LibraryStorage`
 * (un path assoluto su Desktop, un `content://` tree URI serializzato su
 * Android) — tipicamente una cartella scelta con
 * `rememberFolderPickerLauncher` appena prima di avviare il batch.
 *
 * **Onestà sui limiti**: se un file con lo stesso nome esiste già nella
 * cartella di destinazione, il comportamento dipende dalla piattaforma — su
 * Desktop viene sovrascritto silenziosamente, su Android il sistema crea un
 * documento con un nome alternativo invece di sovrascrivere (comportamento
 * di `DocumentsContract.createDocument`, non scelto da questo codice). Va
 * chiamata da un thread in background (`Dispatchers.Default`, come il resto
 * dell'elaborazione in batch): scrive su disco/storage, non è un'operazione
 * pensata per il thread della UI.
 */
expect object BatchExport {
    /** Scrive `bytes` come `fileName` dentro `folderId`. Ritorna una descrizione
     * (path o URI) del file scritto, utile solo per messaggi diagnostici. */
    fun writeBytes(folderId: String, fileName: String, bytes: ByteArray): Result<String>
}
