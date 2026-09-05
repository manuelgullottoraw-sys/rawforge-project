package com.rawforge.shared

import android.net.Uri
import android.provider.DocumentsContract

actual object BatchExport {
    actual fun writeBytes(folderId: String, fileName: String, bytes: ByteArray): Result<String> = runCatching {
        val treeUri = Uri.parse(folderId)
        val parentDocUri = DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )
        // MIME esplicito per nome — creare un documento SAF richiede un MIME
        // valido, e il nome file da solo (via `DocumentsContract`) non lo
        // deduce automaticamente come farebbe `java.io.File` su Desktop. Tre
        // tipi scritti dal batch: preset .xmp, foto renderizzata (JPEG, non
        // più PNG) e — aggiunto in questo giro — il master TIFF a 16 bit
        // senza perdita (vedi `PhotoEditSession.renderFullResolutionExport`
        // lato Rust).
        val mimeType = when {
            fileName.endsWith(".xmp", ignoreCase = true) -> "application/xml"
            fileName.endsWith(".tiff", ignoreCase = true) || fileName.endsWith(".tif", ignoreCase = true) -> "image/tiff"
            else -> "image/jpeg"
        }
        val resolver = AndroidAppContext.context.contentResolver
        val newDocUri = DocumentsContract.createDocument(resolver, parentDocUri, mimeType, fileName)
            ?: throw IllegalStateException("Impossibile creare il file nella cartella scelta: $fileName")
        resolver.openOutputStream(newDocUri)?.use { it.write(bytes) }
            ?: throw IllegalStateException("Impossibile scrivere il file: $fileName")
        newDocUri.toString()
    }
}
