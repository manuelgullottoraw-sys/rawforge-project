package com.rawforge.shared

import androidx.compose.runtime.Composable
import javax.swing.JFileChooser

@Composable
actual fun rememberFolderPickerLauncher(onPicked: (folderId: String) -> Unit): () -> Unit {
    return {
        // JFileChooser (Swing, libreria standard del JDK — nessuna dipendenza
        // aggiuntiva): a differenza di `FileDialog` (AWT, già usato per la
        // selezione di un singolo file) supporta davvero la modalità
        // "sole cartelle" su ogni piattaforma desktop.
        val chooser = JFileChooser()
        chooser.fileSelectionMode = JFileChooser.DIRECTORIES_ONLY
        chooser.dialogTitle = "Scegli la cartella della Libreria"
        if (chooser.showOpenDialog(null) == JFileChooser.APPROVE_OPTION) {
            chooser.selectedFile?.absolutePath?.let(onPicked)
        }
    }
}
