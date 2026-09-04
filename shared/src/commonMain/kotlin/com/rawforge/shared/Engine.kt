package com.rawforge.shared

/**
 * Facciata comune verso il motore nativo (Rust) di RawForge.
 *
 * Le implementazioni Android e Desktop (vedi `Engine.android.kt` / `Engine.desktop.kt`)
 * chiamano davvero il motore Rust tramite i binding Kotlin generati da UniFFI
 * (`uniffi.rawforge_ffi.*`, prodotti dal crate `engine/ffi`), collegati alla libreria
 * nativa compilata per ciascuna piattaforma dalla pipeline CI. Vedi
 * `docs/ARCHITECTURE.md`, §1 e §7.
 */
expect object Engine {
    /** Chiama `rawforge_ffi::engine_version()` — conferma che il collegamento nativo funziona. */
    fun versionInfo(): String

    /**
     * Chiama `rawforge_ffi::generate_lightroom_preset_xmp()` su un `HarmonicLook` di
     * esempio, dimostrando end-to-end la pipeline Sintesi Armonica -> export XMP
     * (docs/ARCHITECTURE.md, §4.1 e §5) attraverso il motore Rust reale.
     */
    fun generateSampleXmpPreset(): String
}
