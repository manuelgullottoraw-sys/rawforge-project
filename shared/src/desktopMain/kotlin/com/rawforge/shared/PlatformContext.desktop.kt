package com.rawforge.shared

import androidx.compose.runtime.Composable

/**
 * Su Desktop `LibraryStorage` usa solo API JDK standard (`java.io.File`,
 * `java.util.prefs.Preferences`) che non richiedono alcun contesto — quindi
 * qui non c'è nulla da inizializzare.
 */
@Composable
actual fun InitializeLibraryPlatform() {
    // Nessuna azione necessaria su Desktop.
}
