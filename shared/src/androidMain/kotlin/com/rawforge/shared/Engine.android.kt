package com.rawforge.shared

import uniffi.rawforge_ffi.HarmonicLookFfi
import uniffi.rawforge_ffi.TonePointFfi
import uniffi.rawforge_ffi.decodeRawFilePreview
import uniffi.rawforge_ffi.engineVersion
import uniffi.rawforge_ffi.extractLookFromRawReference
import uniffi.rawforge_ffi.extractLookFromReferenceImage
import uniffi.rawforge_ffi.generateLightroomPresetXmp
import uniffi.rawforge_ffi.isKnownRawFileName
import uniffi.rawforge_ffi.pasteLookOntoTargetPhoto

actual object Engine {
    actual fun versionInfo(): String = engineVersion()

    actual fun generateSampleXmpPreset(): String {
        val look = HarmonicLookFfi(
            name = "RawForge Sample Look",
            whiteBalanceTemp = 5600u,
            whiteBalanceTint = 0,
            exposureEv = 0.35f,
            contrast = 12,
            highlights = 0,
            shadows = 0,
            whites = 0,
            blacks = 0,
            vibrance = 8,
            saturation = 0,
            toneCurve = listOf(
                TonePointFfi(0u, 0u),
                TonePointFfi(64u, 64u),
                TonePointFfi(128u, 128u),
                TonePointFfi(192u, 192u),
                TonePointFfi(255u, 255u),
            ),
            hslHue = List(8) { 0 },
            hslSat = List(8) { 0 },
            hslLum = List(8) { 0 },
            shadowHue = 210,
            shadowSat = 15,
            highlightHue = 45,
            highlightSat = 10,
            splitToningBalance = 0,
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

    actual fun pasteLookOntoTarget(
        sampleBytes: ByteArray,
        sampleFileName: String,
        lookName: String,
        targetBytes: ByteArray,
        targetFileName: String,
        overrideStrength: Float,
    ): Result<AdaptedPreview> = runCatching {
        val result = pasteLookOntoTargetPhoto(
            sampleBytes,
            sampleFileName,
            lookName,
            targetBytes,
            targetFileName,
            overrideStrength,
        )
        AdaptedPreview(
            renderedImageBytes = result.renderedPreviewPngBytes,
            appliedExposureEv = result.appliedExposureEv,
            appliedHighlights = result.appliedHighlights,
            appliedShadows = result.appliedShadows,
        )
    }
}
