package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Come `rememberFileSaverLauncher`, ma per il testo di un preset `.xmp`
 * invece dei bytes di una foto: stesso principio (l'utente sceglie la
 * cartella di destinazione tramite il selettore nativo della piattaforma —
 * finestra di salvataggio AWT su Desktop, Storage Access Framework su
 * Android), ma con un tipo di contenuto testuale/XML invece che un'immagine
 * PNG. Prima di questa aggiunta, il pulsante "Esporta preset .xmp" calcolava
 * il preset e ne mostrava solo un'anteprima di testo troncata nella UI, senza
 * mai scriverlo su disco: nessun modo di scegliere dove salvarlo davvero.
 */
@Composable
expect fun rememberPresetSaverLauncher(
    onSaved: (destination: String) -> Unit,
    onError: (String) -> Unit,
): (xmpText: String, suggestedFileName: String) -> Unit
