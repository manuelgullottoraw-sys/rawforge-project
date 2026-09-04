package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Ritorna una funzione "salva questi bytes su file" da agganciare al
 * pulsante "Esporta foto": su Desktop apre una finestra di dialogo nativa di
 * salvataggio (AWT `FileDialog` in modalità `SAVE`), su Android lancia il
 * selettore di destinazione di sistema (Storage Access Framework,
 * `ACTION_CREATE_DOCUMENT` — nessun permesso runtime richiesto). A differenza
 * di `rememberFilePickerLauncher` (che non prende argomenti, perché produce
 * output solo tramite `onPicked`), qui i bytes da scrivere non sono noti in
 * anticipo — sono l'anteprima corrente al momento del click — quindi la
 * funzione ritornata li accetta come parametro insieme al nome file
 * suggerito.
 */
@Composable
expect fun rememberFileSaverLauncher(
    onSaved: (destination: String) -> Unit,
    onError: (String) -> Unit,
): (bytes: ByteArray, suggestedFileName: String) -> Unit
