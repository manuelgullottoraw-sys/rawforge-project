package com.rawforge.shared

import uniffi.rawforge_ffi.HarmonicLookFfi
import uniffi.rawforge_ffi.TonePointFfi
import uniffi.rawforge_ffi.decodeRawFilePreview
import uniffi.rawforge_ffi.engineVersion
import uniffi.rawforge_ffi.extractLookFromRawReference
import uniffi.rawforge_ffi.extractLookFromReferenceImage
import uniffi.rawforge_ffi.generateLightroomPresetXmp
import uniffi.rawforge_ffi.isKnownRawFileName
import uniffi.rawforge_ffi.PhotoEditSession as NativePhotoEditSession

/**
 * Converte da/verso `HarmonicLookFfi` (generato da UniFFI, esiste solo qui
 * dentro `desktopMain`) e `EditableLook` (comune, solo tipi primitivi — vedi
 * la nota su `EditableLook` in `Engine.kt`). Nessuna delle due funzioni
 * attraversa mai il confine `expect`/`actual` con un tipo UniFFI: `toFfi()` è
 * chiamata solo qui, subito prima di una chiamata al motore; `toEditable()`
 * solo qui, subito dopo.
 */
private fun EditableLook.toFfi(): HarmonicLookFfi = HarmonicLookFfi(
    name = name,
    whiteBalanceTemp = whiteBalanceTemp.toUInt(),
    whiteBalanceTint = whiteBalanceTint,
    exposureEv = exposureEv,
    contrast = contrast,
    highlights = highlights,
    shadows = shadows,
    whites = whites,
    blacks = blacks,
    vibrance = vibrance,
    saturation = saturation,
    toneCurve = toneCurve.map { TonePointFfi(it.x.toUByte(), it.y.toUByte()) },
    hslHue = hslHue,
    hslSat = hslSat,
    hslLum = hslLum,
    shadowHue = shadowHue,
    shadowSat = shadowSat,
    highlightHue = highlightHue,
    highlightSat = highlightSat,
    splitToningBalance = splitToningBalance,
    textureFine = textureFine,
    textureMedium = textureMedium,
    textureCoarse = textureCoarse,
    whiteBalanceBTemp = whiteBalanceBTemp.toUInt(),
    whiteBalanceBTint = whiteBalanceBTint,
    wbGradientEnabled = wbGradientEnabled,
    wbGradientVertical = wbGradientVertical,
    wbGradientPosition = wbGradientPosition,
    wbGradientSpread = wbGradientSpread,
    noiseReductionLuma = noiseReductionLuma,
    noiseReductionColor = noiseReductionColor,
)

private fun HarmonicLookFfi.toEditable(): EditableLook = EditableLook(
    name = name,
    whiteBalanceTemp = whiteBalanceTemp.toInt(),
    whiteBalanceTint = whiteBalanceTint,
    exposureEv = exposureEv,
    contrast = contrast,
    highlights = highlights,
    shadows = shadows,
    whites = whites,
    blacks = blacks,
    vibrance = vibrance,
    saturation = saturation,
    toneCurve = toneCurve.map { TonePoint(it.x.toInt(), it.y.toInt()) },
    hslHue = hslHue,
    hslSat = hslSat,
    hslLum = hslLum,
    shadowHue = shadowHue,
    shadowSat = shadowSat,
    highlightHue = highlightHue,
    highlightSat = highlightSat,
    splitToningBalance = splitToningBalance,
    textureFine = textureFine,
    textureMedium = textureMedium,
    textureCoarse = textureCoarse,
    whiteBalanceBTemp = whiteBalanceBTemp.toInt(),
    whiteBalanceBTint = whiteBalanceBTint,
    wbGradientEnabled = wbGradientEnabled,
    wbGradientVertical = wbGradientVertical,
    wbGradientPosition = wbGradientPosition,
    wbGradientSpread = wbGradientSpread,
    noiseReductionLuma = noiseReductionLuma,
    noiseReductionColor = noiseReductionColor,
)

actual class PhotoEditSession(private val inner: NativePhotoEditSession) {
    actual fun pasteLookFromSample(
        sampleBytes: ByteArray,
        sampleFileName: String,
        lookName: String,
        overrideStrength: Float,
    ): Result<AdaptedPreview> = runCatching {
        val result = inner.pasteLookFromSample(sampleBytes, sampleFileName, lookName, overrideStrength)
        AdaptedPreview(
            renderedImageBytes = result.renderedPreviewPngBytes,
            appliedLook = result.appliedLook.toEditable(),
        )
    }

    actual fun renderPreview(look: EditableLook): Result<RenderedPreview> = runCatching {
        val result = inner.renderPreview(look.toFfi())
        RenderedPreview(
            imageBytes = result.previewPngBytes,
            shadowClipFraction = result.shadowClipFraction,
            highlightClipFraction = result.highlightClipFraction,
        )
    }

    actual fun renderFullResolution(look: EditableLook): Result<ByteArray> = runCatching {
        inner.renderFullResolution(look.toFfi())
    }

    actual fun close() = inner.close()
}

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
            textureFine = 0,
            textureMedium = 0,
            textureCoarse = 0,
            whiteBalanceBTemp = 5600u,
            whiteBalanceBTint = 0,
            wbGradientEnabled = false,
            wbGradientVertical = true,
            wbGradientPosition = 50,
            wbGradientSpread = 50,
            noiseReductionLuma = 0,
            noiseReductionColor = 0,
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

    actual fun openPhotoForEditing(bytes: ByteArray, fileName: String): Result<PhotoEditSession> = runCatching {
        PhotoEditSession(NativePhotoEditSession(bytes, fileName))
    }
}
