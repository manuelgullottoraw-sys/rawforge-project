package com.rawforge.shared

import java.io.File

actual object BatchExport {
    actual fun writeBytes(folderId: String, fileName: String, bytes: ByteArray): Result<String> = runCatching {
        val dir = File(folderId)
        if (!dir.exists() && !dir.mkdirs()) {
            throw IllegalStateException("Impossibile creare la cartella di destinazione: $folderId")
        }
        val file = File(dir, fileName)
        file.writeBytes(bytes)
        file.absolutePath
    }
}
