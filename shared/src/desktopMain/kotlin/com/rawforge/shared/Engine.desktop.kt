package com.rawforge.shared

import uniffi.rawforge_ffi.HarmonicLookFfi
import uniffi.rawforge_ffi.decodeRawFilePreview
import uniffi.rawforge_ffi.engineVersion
import uniffi.rawforge_ffi.extractLookFromRawReference
import uniffi.rawforge_ffi.extractLookFromReferenceImage
import uniffi.rawforge_ffi.generateLightroomPresetXmp
import uniffi.rawforge_ffi.isKnownRawFileName

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

    actual fun importPhoto(bytes: ByteArray, fileName: String): Result<ImportedPhoto> = runCatching {
        if (isKnownRawFileName(fileName)) {
            // File RAW vero: il motore decodifica l'anteprima incorporata dalla
            // fotocamera stessa (crate raw-decode, nessun demosaic ancora).
            val preview = decodeRawFilePreview(bytes)
            ImportedPhoto(
                fileName = fileName,
                cameraMake = preview.cameraMake,
                cameraModel = preview.cameraModel,
                previewImageBytes = preview.previewPngBytes,
            )
        } else {
            // Già un'immagine sviluppata (JPEG/PNG): i bytes originali sono già
            // un'anteprima valida, nessuna decodifica RAW necessaria.
            ImportedPhoto(
                fileName = fileName,
                cameraMake = null,
                cameraModel = null,
                previewImageBytes = bytes,
            )
        }
    }

    actual fun extractLookAndExportXmp(bytes: ByteArray, fileName: String, lookName: String): Result<String> =
        runCatching {
            val look = if (isKnownRawFileName(fileName)) {
                extractLookFromRawReference(bytes, lookName)
            } else {
                extractLookFromReferenceImage(bytes, lookName)
            }
            generateLightroomPresetXmp(look)
        }
}
