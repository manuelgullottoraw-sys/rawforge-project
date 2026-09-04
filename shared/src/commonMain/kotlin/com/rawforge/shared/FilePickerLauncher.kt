package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Ritorna una funzione "avvia selezione file" da agganciare a un pulsante:
 * su Desktop apre una finestra di dialogo file nativa (AWT `FileDialog`), su
 * Android lancia il selettore di documenti di sistema (Storage Access
 * Framework, nessun permesso runtime richiesto). Il risultato (bytes del
 * file più il suo nome, necessario per riconoscere l'estensione RAW) arriva
 * tramite `onPicked` sul thread della UI.
 */
@Composable
expect fun rememberFilePickerLauncher(onPicked: (bytes: ByteArray, fileName: String) -> Unit): () -> Unit
