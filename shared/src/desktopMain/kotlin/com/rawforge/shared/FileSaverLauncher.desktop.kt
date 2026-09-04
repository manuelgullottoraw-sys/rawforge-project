package com.rawforge.shared

import androidx.compose.runtime.Composable
import java.awt.FileDialog
import java.io.File

@Composable
actual fun rememberFileSaverLauncher(
    onSaved: (String) -> Unit,
    onError: (String) -> Unit,
): (ByteArray, String) -> Unit {
    return { bytes, suggestedFileName ->
        // AWT FileDialog in modalità SAVE: stessa scelta già fatta per
        // l'importazione (nessuna dipendenza aggiuntiva, finestra nativa
        // Windows), qui con il nome file precompilato con quello suggerito.
        val dialog = FileDialog(null as java.awt.Frame?, "Esporta foto", FileDialog.SAVE)
        dialog.file = suggestedFileName
        dialog.isVisible = true
        val directory = dialog.directory
        val fileName = dialog.file
        if (directory != null && fileName != null) {
            try {
                val file = File(directory, fileName)
                file.writeBytes(bytes)
                onSaved(file.absolutePath)
            } catch (e: Exception) {
                onError(e.message ?: "Errore sconosciuto durante il salvataggio")
            }
        }
    }
}
