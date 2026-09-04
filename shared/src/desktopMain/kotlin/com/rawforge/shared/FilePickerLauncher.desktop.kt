package com.rawforge.shared

import androidx.compose.runtime.Composable
import java.awt.FileDialog
import java.io.File

@Composable
actual fun rememberFilePickerLauncher(onPicked: (bytes: ByteArray, fileName: String) -> Unit): () -> Unit {
    return {
        // AWT FileDialog: finestra di dialogo nativa Windows, semplice e
        // robusta (nessuna dipendenza aggiuntiva) — pompa il proprio event
        // loop finché l'utente non sceglie un file o annulla.
        val dialog = FileDialog(null as java.awt.Frame?, "Seleziona una foto (RAW, JPEG o PNG)", FileDialog.LOAD)
        dialog.isVisible = true
        val directory = dialog.directory
        val fileName = dialog.file
        if (directory != null && fileName != null) {
            val file = File(directory, fileName)
            onPicked(file.readBytes(), file.name)
        }
    }
}
