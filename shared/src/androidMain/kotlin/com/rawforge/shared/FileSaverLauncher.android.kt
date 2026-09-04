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
actual fun rememberFileSaverLauncher(
    onSaved: (String) -> Unit,
    onError: (String) -> Unit,
): (ByteArray, String) -> Unit {
    val context = LocalContext.current
    // `CreateDocument` chiede all'utente SOLO la destinazione: i bytes da
    // scrivere non passano dall'Intent (potrebbero essere grandi), li teniamo
    // in memoria qui e li scriviamo quando il callback torna con l'Uri scelto.
    var pendingBytes by remember { mutableStateOf<ByteArray?>(null) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/png")) { uri: Uri? ->
        val bytes = pendingBytes
        pendingBytes = null
        if (uri == null) {
            return@rememberLauncherForActivityResult
        }
        if (bytes == null) {
            onError("Nessun dato da salvare")
            return@rememberLauncherForActivityResult
        }
        try {
            context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                ?: onError("Impossibile aprire la destinazione scelta")
            onSaved(uri.toString())
        } catch (e: Exception) {
            onError(e.message ?: "Errore sconosciuto durante il salvataggio")
        }
    }
    return { bytes, suggestedFileName ->
        pendingBytes = bytes
        launcher.launch(suggestedFileName)
    }
}
