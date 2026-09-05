package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Ritorna una funzione "scegli la cartella della Libreria" da agganciare a
 * un pulsante — vedi `LibraryStorage` per cosa succede dopo. Su Desktop
 * apre un selettore di cartelle nativo (`javax.swing.JFileChooser` in
 * modalità sole-cartelle: `java.awt.FileDialog`, usato per la selezione di
 * un singolo file altrove in questo progetto, non supporta la selezione di
 * cartelle in modo affidabile multipiattaforma). Su Android lancia il
 * selettore di albero di documenti di sistema (Storage Access Framework,
 * `ACTION_OPEN_DOCUMENT_TREE`) e richiede subito il permesso PERSISTENTE
 * sulla cartella scelta — necessario perché, a differenza della selezione
 * di un singolo file altrove in questo progetto, qui l'accesso deve
 * restare valido anche dopo aver riavviato l'app (`LibraryStorage` la
 * ricorda fra le sessioni). L'identificatore restituito tramite `onPicked`
 * è opaco e specifico di piattaforma — va passato solo a `LibraryStorage`,
 * mai interpretato direttamente dalla UI comune.
 */
@Composable
expect fun rememberFolderPickerLauncher(onPicked: (folderId: String) -> Unit): () -> Unit
