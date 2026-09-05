package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Punto di innesto per l'inizializzazione specifica di piattaforma che ha
 * bisogno di un contesto "vivo" della UI (su Android, un `android.content.Context`)
 * ma non è di per sé una schermata — oggi serve solo a `LibraryStorage` su
 * Android (per `SharedPreferences` e `ContentResolver`). Va chiamata UNA
 * SOLA VOLTA, il prima possibile dentro `RawForgeApp()`, prima di qualunque
 * uso di `LibraryStorage`/`rememberFolderPickerLauncher`.
 *
 * Perché non un `Application` dedicato: questo progetto non ne ha uno (le
 * activity Android esistenti sono minime), e introdurne uno solo per questo
 * avrebbe richiesto toccare il manifest/i file di avvio su entrambe le
 * piattaforme per un guadagno minimo. Catturare il contesto qui, dentro il
 * primo composable eseguito, è più semplice e altrettanto affidabile perché
 * `RawForgeApp()` viene sempre composta prima che la Libreria possa essere
 * aperta dall'utente.
 */
@Composable
expect fun InitializeLibraryPlatform()
