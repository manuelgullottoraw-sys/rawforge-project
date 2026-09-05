package com.rawforge.shared

import androidx.compose.runtime.Composable
import java.awt.FileDialog
import java.io.File

@Composable
actual fun rememberPresetSaverLauncher(
    onSaved: (String) -> Unit,
    onError: (String) -> Unit,
): (String, String) -> Unit {
    return { xmpText, suggestedFileName ->
        // Stessa finestra nativa AWT già usata per l'esportazione della foto
        // (`FileSaverLauncher.desktop.kt`): SAVE lascia scegliere sia la
        // cartella di destinazione sia il nome file, precompilato con quello
        // suggerito ma modificabile.
        val dialog = FileDialog(null as java.awt.Frame?, "Esporta preset .xmp", FileDialog.SAVE)
        dialog.file = suggestedFileName
        dialog.isVisible = true
        val directory = dialog.directory
        val fileName = dialog.file
        if (directory != null && fileName != null) {
            try {
                val file = File(directory, fileName)
                file.writeText(xmpText, Charsets.UTF_8)
                onSaved(file.absolutePath)
            } catch (e: Exception) {
                onError(e.message ?: "Errore sconosciuto durante il salvataggio")
            }
        }
    }
}
