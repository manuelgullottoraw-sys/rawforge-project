package com.rawforge.shared

import uniffi.rawforge_ffi.HarmonicLookFfi
import uniffi.rawforge_ffi.engineVersion
import uniffi.rawforge_ffi.generateLightroomPresetXmp

actual object Engine {
    actual fun versionInfo(): String = engineVersion()

    actual fun generateSampleXmpPreset(): String {
        val look = HarmonicLookFfi(
            name = "RawForge Sample Look",
            exposureEv = 0.35f,
            contrast = 12,
            vibrance = 8,
            whiteBalanceTemp = 5600u,
            shadowHue = 210,
            shadowSat = 15,
            highlightHue = 45,
            highlightSat = 10,
        )
        return generateLightroomPresetXmp(look)
    }
}
