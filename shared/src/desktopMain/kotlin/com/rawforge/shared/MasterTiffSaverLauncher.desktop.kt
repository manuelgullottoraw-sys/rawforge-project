package com.rawforge.shared

import androidx.compose.runtime.Composable
import java.awt.FileDialog
import java.io.File

@Composable
actual fun rememberMasterTiffSaverLauncher(
    onSaved: (String) -> Unit,
    onError: (String) -> Unit,
): (ByteArray, String) -> Unit {
    return { bytes, suggestedFileName ->
        // Stessa finestra nativa AWT già usata per l'esportazione della foto
        // (`FileSaverLauncher.desktop.kt`) e del preset (`PresetSaverLauncher.desktop.kt`):
        // su Desktop il tipo di contenuto non condiziona il dialogo (nessun
        // MIME da dichiarare), solo il nome file suggerito cambia estensione.
        val dialog = FileDialog(null as java.awt.Frame?, "Esporta master TIFF", FileDialog.SAVE)
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
