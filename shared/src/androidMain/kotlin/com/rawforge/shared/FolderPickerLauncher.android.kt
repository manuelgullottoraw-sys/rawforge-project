package com.rawforge.shared

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

@Composable
actual fun rememberFolderPickerLauncher(onPicked: (folderId: String) -> Unit): () -> Unit {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri: Uri? ->
        if (uri != null) {
            // Fondamentale per una Libreria "persistente" (scelta dell'utente):
            // senza questa chiamata il permesso vale solo per la sessione
            // corrente e sparirebbe al riavvio dell'app, rendendo inutile
            // ricordare la cartella in SharedPreferences.
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
            onPicked(uri.toString())
        }
    }
    return { launcher.launch(null) }
}
