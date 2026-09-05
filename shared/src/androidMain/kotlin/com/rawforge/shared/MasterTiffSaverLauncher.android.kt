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
actual fun rememberMasterTiffSaverLauncher(
    onSaved: (String) -> Unit,
    onError: (String) -> Unit,
): (ByteArray, String) -> Unit {
    val context = LocalContext.current
    // Come `FileSaverLauncher.android.kt`, ma con mime type "image/tiff" (non
    // "image/jpeg": il master non è un JPEG) — altrimenti il selettore di
    // sistema (Storage Access Framework) proporrebbe un'estensione/
    // associazione sbagliata per un file che in realtà è un TIFF a 16 bit.
    var pendingBytes by remember { mutableStateOf<ByteArray?>(null) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/tiff")) { uri: Uri? ->
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
