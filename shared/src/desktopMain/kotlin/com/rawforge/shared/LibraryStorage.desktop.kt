package com.rawforge.shared

import java.io.File
import java.util.prefs.Preferences

/**
 * Persistenza della cartella Libreria su Desktop: `java.util.prefs.Preferences`
 * (equivalente JDK standard di `SharedPreferences` su Android — libreria
 * standard, nessuna dipendenza aggiuntiva, sopravvive al riavvio del
 * processo perché scritta dal JDK nel registro/nei file di preferenze
 * dell'utente del sistema operativo, non nella memoria del processo).
 */
private val libraryPrefs = Preferences.userRoot().node("com.rawforge.shared.library")
private const val FOLDER_KEY = "folder_path"

/**
 * Stesse estensioni RAW riconosciute da `raw_decode::KNOWN_RAW_EXTENSIONS`
 * lato Rust, più i formati già sviluppati che `Engine.importPhoto` accetta
 * direttamente — duplicato qui perché elencare i file di una cartella è
 * un'operazione lato Kotlin, prima ancora di toccare il motore per ciascun
 * file. Se l'elenco lato Rust cambia, va aggiornato anche qui: un
 * disallineamento non romperebbe nulla (il motore resta l'autorità finale
 * su cosa sa davvero decodificare) ma potrebbe nascondere o mostrare file
 * nella griglia della Libreria in modo inconsistente con l'import singolo.
 */
private val KNOWN_PHOTO_EXTENSIONS = setOf(
    "jpg", "jpeg", "png",
    "cr2", "cr3", "nef", "arw", "raf", "rw2", "dng", "pef", "orf", "srw", "raw", "3fr", "mrw",
)

actual object LibraryStorage {
    actual fun rememberedFolder(): String? = libraryPrefs.get(FOLDER_KEY, null)

    actual fun rememberFolder(folderId: String) {
        libraryPrefs.put(FOLDER_KEY, folderId)
    }

    actual fun listPhotos(folderId: String): Result<List<LibraryPhotoEntry>> = runCatching {
        val dir = File(folderId)
        val files = dir.listFiles() ?: throw IllegalStateException("Impossibile leggere la cartella: $folderId")
        files
            .filter { it.isFile && it.extension.lowercase() in KNOWN_PHOTO_EXTENSIONS }
            .sortedBy { it.name.lowercase() }
            .map { file -> LibraryPhotoEntry(id = file.absolutePath, displayName = file.name, sizeBytes = file.length()) }
    }

    actual fun readPhotoBytes(id: String): Result<ByteArray> = runCatching {
        File(id).readBytes()
    }
}
