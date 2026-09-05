package com.rawforge.shared

import android.content.SharedPreferences
import android.net.Uri
import android.provider.DocumentsContract

private const val PREFS_NAME = "com.rawforge.shared.library"
private const val FOLDER_KEY = "folder_uri"

/**
 * Stessa lista di `LibraryStorage.desktop.kt` (vedi lì per il perché è
 * duplicata) — tenerle allineate se cambia il riconoscimento lato Rust.
 */
private val KNOWN_PHOTO_EXTENSIONS = setOf(
    "jpg", "jpeg", "png",
    "cr2", "cr3", "nef", "arw", "raf", "rw2", "dng", "pef", "orf", "srw", "raw", "3fr", "mrw",
)

private fun prefs(): SharedPreferences =
    AndroidAppContext.context.getSharedPreferences(PREFS_NAME, android.content.Context.MODE_PRIVATE)

actual object LibraryStorage {
    actual fun rememberedFolder(): String? = prefs().getString(FOLDER_KEY, null)

    actual fun rememberFolder(folderId: String) {
        prefs().edit().putString(FOLDER_KEY, folderId).apply()
    }

    /**
     * Elenca i figli diretti (non ricorsivo, come da contratto comune)
     * dell'albero di documenti `folderId` (un `content://` tree URI
     * serializzato) usando `DocumentsContract` direttamente — niente
     * dipendenza `androidx.documentfile`, che non è fra quelle già
     * dichiarate in `shared/build.gradle.kts` (vedi la nota di policy nel
     * progetto: niente nuove dipendenze Gradle non verificabili qui).
     */
    actual fun listPhotos(folderId: String): Result<List<LibraryPhotoEntry>> = runCatching {
        val treeUri = Uri.parse(folderId)
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )
        val resolver = AndroidAppContext.context.contentResolver
        val entries = mutableListOf<LibraryPhotoEntry>()
        resolver.query(
            childrenUri,
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
            ),
            null,
            null,
            null,
        )?.use { cursor ->
            val idIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
            val nameIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
            val sizeIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_SIZE)
            val mimeIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE)
            while (cursor.moveToNext()) {
                val mime = if (mimeIdx >= 0) cursor.getString(mimeIdx) else null
                // Le sottocartelle hanno MIME_TYPE_DIR: le saltiamo, coerente
                // con "listPhotos non è ricorsiva" dichiarato in comune.
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) continue
                val docId = if (idIdx >= 0) cursor.getString(idIdx) else continue
                val name = if (nameIdx >= 0) cursor.getString(nameIdx) else docId
                if (name.substringAfterLast('.', "").lowercase() !in KNOWN_PHOTO_EXTENSIONS) continue
                val size = if (sizeIdx >= 0) cursor.getLong(sizeIdx) else 0L
                val docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
                entries.add(LibraryPhotoEntry(id = docUri.toString(), displayName = name, sizeBytes = size))
            }
        }
        entries.sortedBy { it.displayName.lowercase() }
    }

    actual fun readPhotoBytes(id: String): Result<ByteArray> = runCatching {
        val uri = Uri.parse(id)
        AndroidAppContext.context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
            ?: throw IllegalStateException("Impossibile leggere il file: $id")
    }
}
