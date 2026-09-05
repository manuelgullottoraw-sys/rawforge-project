package com.rawforge.shared

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

/**
 * Contenitore statico per l'`applicationContext`, catturato una sola volta
 * da `InitializeLibraryPlatform()`. `LibraryStorage` è un `expect object`
 * (non un composable) e quindi non può chiamare `LocalContext.current`
 * direttamente — questo è l'unico modo per dargli accesso a
 * `SharedPreferences`/`ContentResolver` senza introdurre un `Application`
 * dedicato (vedi la nota in `PlatformContext.kt`).
 *
 * Usare `applicationContext` (non l'`Activity` stessa) evita di trattenere
 * un riferimento che sopravviverebbe alla distruzione dell'Activity — una
 * fonte comune di memory leak su Android.
 */
internal object AndroidAppContext {
    lateinit var context: Context
        private set

    fun initialize(context: Context) {
        this.context = context.applicationContext
    }

    val isInitialized: Boolean
        get() = ::context.isInitialized
}

@Composable
actual fun InitializeLibraryPlatform() {
    val context = LocalContext.current
    if (!AndroidAppContext.isInitialized) {
        AndroidAppContext.initialize(context)
    }
}
