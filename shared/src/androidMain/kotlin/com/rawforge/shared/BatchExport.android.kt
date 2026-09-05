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
        // deduce automaticamente come farebbe `java.io.File` su Desktop.
        val mimeType = if (fileName.endsWith(".xmp", ignoreCase = true)) "application/xml" else "image/png"
        val resolver = AndroidAppContext.context.contentResolver
        val newDocUri = DocumentsContract.createDocument(resolver, parentDocUri, mimeType, fileName)
            ?: throw IllegalStateException("Impossibile creare il file nella cartella scelta: $fileName")
        resolver.openOutputStream(newDocUri)?.use { it.write(bytes) }
            ?: throw IllegalStateException("Impossibile scrivere il file: $fileName")
        newDocUri.toString()
    }
}
