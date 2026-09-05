package com.rawforge.shared

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext

@Composable
actual fun rememberPresetSaverLauncher(
    onSaved: (String) -> Unit,
    onError: (String) -> Unit,
): (String, String) -> Unit {
    val context = LocalContext.current
    // Come `FileSaverLauncher.android.kt`, ma con mime type XML (non
    // "image/png": un preset `.xmp` non è un'immagine) — altrimenti il
    // selettore di sistema (Storage Access Framework) proporrebbe
    // un'estensione/associazione sbagliata.
    var pendingText by remember { mutableStateOf<String?>(null) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/xml")) { uri: Uri? ->
        val text = pendingText
        pendingText = null
        if (uri == null) {
            return@rememberLauncherForActivityResult
        }
        if (text == null) {
            onError("Nessun dato da salvare")
            return@rememberLauncherForActivityResult
        }
        try {
            context.contentResolver.openOutputStream(uri)?.use { it.write(text.toByteArray(Charsets.UTF_8)) }
                ?: onError("Impossibile aprire la destinazione scelta")
            onSaved(uri.toString())
        } catch (e: Exception) {
            onError(e.message ?: "Errore sconosciuto durante il salvataggio")
        }
    }
    return { text, suggestedFileName ->
        pendingText = text
        launcher.launch(suggestedFileName)
    }
}
