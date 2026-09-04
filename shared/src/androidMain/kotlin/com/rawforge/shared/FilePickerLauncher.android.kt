package com.rawforge.shared

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

@Composable
actual fun rememberFilePickerLauncher(onPicked: (bytes: ByteArray, fileName: String) -> Unit): () -> Unit {
    val context = LocalContext.current
    // Storage Access Framework: nessun permesso runtime richiesto, funziona
    // sia su file locali sia su provider cloud (Google Drive, ecc.).
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri != null) {
            val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
            if (bytes != null) {
                onPicked(bytes, resolveDisplayName(context, uri))
            }
        }
    }
    return { launcher.launch(arrayOf("*/*")) }
}

/**
 * I content:// URI di Android non portano un nome file "vero" nel path —
 * serve interrogare il ContentResolver per il DISPLAY_NAME, altrimenti non
 * potremmo riconoscere l'estensione RAW (es. ".CR3") e sceglieremmo il
 * percorso di decodifica sbagliato in Engine.importPhoto.
 */
private fun resolveDisplayName(context: Context, uri: Uri): String {
    var name: String? = null
    context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
        val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
        if (nameIndex >= 0 && cursor.moveToFirst()) {
            name = cursor.getString(nameIndex)
        }
    }
    return name ?: uri.lastPathSegment ?: "foto_importata"
}
