package com.rawforge.shared

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.Button
import androidx.compose.material.Divider
import androidx.compose.material.MaterialTheme
import androidx.compose.material.Slider
import androidx.compose.material.Surface
import androidx.compose.material.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/**
 * Stato di una foto importata dall'utente: tiene anche i bytes originali
 * (servono a rilanciare l'analisi sui dati grezzi, non sulla sola anteprima)
 * insieme a ciò che la UI mostra. Usato sia per la foto campione sia per la
 * foto da modificare.
 */
private data class ImportState(
    val fileName: String,
    val rawBytes: ByteArray,
    val cameraLabel: String?,
    val bitmap: ImageBitmap?,
)

/**
 * UI condivisa (identica su Android e Windows). Tre sezioni:
 * 1. la demo minimale originale (stato motore, preset XMP di esempio), già
 *    verificata in CI su entrambe le piattaforme;
 * 2. importa la foto campione (quella con il "look" da copiare) e copiane le
 *    impostazioni come preset Lightroom `.xmp`;
 * 3. apri la foto da modificare e incollaci le impostazioni copiate — non
 *    identiche, ma adattate in modo intelligente alla scena specifica di
 *    quella foto (Smart-Batch Contestuale, docs/ARCHITECTURE.md §4.2).
 *    L'anteprima risultante resta e si vede subito nell'app, l'export `.xmp`
 *    (sezione 2) resta un'azione separata e facoltativa.
 * Il modulo Develop con slider in tempo reale, la libreria a griglia e il
 * batch su centinaia di foto restano da costruire sopra questa base.
 */
@Composable
fun RawForgeApp() {
    var engineInfo by remember { mutableStateOf<String?>(null) }
    var xmpPreview by remember { mutableStateOf<String?>(null) }

    var sampleState by remember { mutableStateOf<ImportState?>(null) }
    var sampleError by remember { mutableStateOf<String?>(null) }
    var harmonicXmp by remember { mutableStateOf<String?>(null) }
    var harmonicError by remember { mutableStateOf<String?>(null) }

    var targetState by remember { mutableStateOf<ImportState?>(null) }
    var targetError by remember { mutableStateOf<String?>(null) }
    var overrideStrength by remember { mutableStateOf(1f) }
    var adaptedBitmap by remember { mutableStateOf<ImageBitmap?>(null) }
    var adaptedInfo by remember { mutableStateOf<String?>(null) }
    var pasteError by remember { mutableStateOf<String?>(null) }

    fun importInto(bytes: ByteArray, fileName: String, onDone: (ImportState) -> Unit, onError: (String) -> Unit) {
        Engine.importPhoto(bytes, fileName).fold(
            onSuccess = { photo ->
                onDone(
                    ImportState(
                        fileName = photo.fileName,
                        rawBytes = bytes,
                        cameraLabel = listOfNotNull(photo.cameraMake, photo.cameraModel)
                            .joinToString(" ")
                            .ifBlank { null },
                        bitmap = decodeImageBitmapOrNull(photo.previewImageBytes),
                    )
                )
            },
            onFailure = { error -> onError(error.message ?: "Errore sconosciuto durante l'importazione") }
        )
    }

    val launchSamplePicker = rememberFilePickerLauncher { bytes, fileName ->
        sampleError = null
        harmonicXmp = null
        harmonicError = null
        // Cambiando la foto campione, un'eventuale anteprima già incollata
        // sulla foto target non rispecchia più le impostazioni correnti.
        adaptedBitmap = null
        adaptedInfo = null
        pasteError = null
        importInto(
            bytes,
            fileName,
            onDone = { sampleState = it },
            onError = { sampleError = it; sampleState = null }
        )
    }

    val launchTargetPicker = rememberFilePickerLauncher { bytes, fileName ->
        targetError = null
        adaptedBitmap = null
        adaptedInfo = null
        pasteError = null
        importInto(
            bytes,
            fileName,
            onDone = { targetState = it },
            onError = { targetError = it; targetState = null }
        )
    }

    MaterialTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                Text("RawForge", style = MaterialTheme.typography.h3)
                Spacer(Modifier.height(8.dp))
                Text("Motore RAW ultra-veloce — motore Rust collegato via UniFFI")
                Spacer(Modifier.height(24.dp))

                Button(onClick = { engineInfo = Engine.versionInfo() }) {
                    Text("Stato motore")
                }
                engineInfo?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(it)
                }

                Spacer(Modifier.height(24.dp))

                Button(onClick = { xmpPreview = Engine.generateSampleXmpPreset() }) {
                    Text("Genera preset XMP di esempio (Sintesi Armonica)")
                }
                xmpPreview?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        it.take(600) + if (it.length > 600) "\n… (troncato)" else "",
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
                        style = MaterialTheme.typography.caption
                    )
                }

                Spacer(Modifier.height(32.dp))
                Divider(modifier = Modifier.fillMaxWidth())
                Spacer(Modifier.height(24.dp))

                Text("Sintesi Armonica: importa la foto campione", style = MaterialTheme.typography.h5)
                Spacer(Modifier.height(8.dp))
                Text(
                    "Scegli la foto con il \"look\" che vuoi copiare — RAW (CR2/CR3/NEF/ARW/RAF/RW2/" +
                        "DNG/...), JPEG o PNG. Per un file RAW il motore decodifica l'anteprima " +
                        "incorporata dalla fotocamera stessa (crate raw-decode).",
                    style = MaterialTheme.typography.caption,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp)
                )
                Spacer(Modifier.height(8.dp))
                Button(onClick = { launchSamplePicker() }) {
                    Text("Importa foto campione…")
                }

                sampleError?.let {
                    Spacer(Modifier.height(8.dp))
                    Text("Errore: $it", color = MaterialTheme.colors.error)
                }

                sampleState?.let { state ->
                    Spacer(Modifier.height(16.dp))
                    Text(state.fileName, style = MaterialTheme.typography.subtitle1)
                    state.cameraLabel?.let {
                        Text(it, style = MaterialTheme.typography.caption)
                    }
                    Spacer(Modifier.height(8.dp))
                    val bitmap = state.bitmap
                    if (bitmap != null) {
                        Image(
                            bitmap = bitmap,
                            contentDescription = state.fileName,
                            modifier = Modifier.fillMaxWidth().padding(8.dp)
                        )
                    } else {
                        Text("(anteprima non decodificabile dalla UI, ma il motore ha letto i metadati)")
                    }

                    Spacer(Modifier.height(8.dp))
                    Button(onClick = {
                        harmonicError = null
                        Engine.extractLookAndExportXmp(
                            state.rawBytes,
                            state.fileName,
                            "Look da ${state.fileName}"
                        ).fold(
                            onSuccess = { xmp -> harmonicXmp = xmp },
                            onFailure = { error -> harmonicError = error.message ?: "Errore sconosciuto" }
                        )
                    }) {
                        Text("Copia le impostazioni da questa foto → genera preset .xmp")
                    }

                    harmonicError?.let {
                        Spacer(Modifier.height(8.dp))
                        Text("Errore: $it", color = MaterialTheme.colors.error)
                    }

                    harmonicXmp?.let {
                        Spacer(Modifier.height(8.dp))
                        Text(
                            it.take(600) + if (it.length > 600) "\n… (troncato)" else "",
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
                            style = MaterialTheme.typography.caption
                        )
                    }
                }

                if (sampleState != null) {
                    Spacer(Modifier.height(32.dp))
                    Divider(modifier = Modifier.fillMaxWidth())
                    Spacer(Modifier.height(24.dp))

                    Text("Apri la foto da modificare", style = MaterialTheme.typography.h5)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Le impostazioni copiate dalla foto campione verranno adattate in modo " +
                            "intelligente alla scena di questa foto (Smart-Batch Contestuale), non " +
                            "applicate identiche.",
                        style = MaterialTheme.typography.caption,
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp)
                    )
                    Spacer(Modifier.height(8.dp))
                    Button(onClick = { launchTargetPicker() }) {
                        Text("Apri foto da modificare…")
                    }

                    targetError?.let {
                        Spacer(Modifier.height(8.dp))
                        Text("Errore: $it", color = MaterialTheme.colors.error)
                    }

                    targetState?.let { target ->
                        Spacer(Modifier.height(16.dp))
                        Text(target.fileName, style = MaterialTheme.typography.subtitle1)
                        target.cameraLabel?.let {
                            Text(it, style = MaterialTheme.typography.caption)
                        }
                        Spacer(Modifier.height(8.dp))
                        val shownBitmap = adaptedBitmap ?: target.bitmap
                        if (shownBitmap != null) {
                            Image(
                                bitmap = shownBitmap,
                                contentDescription = target.fileName,
                                modifier = Modifier.fillMaxWidth().padding(8.dp)
                            )
                        } else {
                            Text("(anteprima non decodificabile dalla UI, ma il motore ha letto i metadati)")
                        }

                        Spacer(Modifier.height(8.dp))
                        Text(
                            "Intensità adattamento: ${(overrideStrength * 100).roundToInt()}% " +
                                "(0% = impostazioni identiche alla foto campione, " +
                                "100% = massimo adattamento intelligente alla scena)",
                            style = MaterialTheme.typography.caption,
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp)
                        )
                        Slider(
                            value = overrideStrength,
                            onValueChange = { overrideStrength = it },
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp)
                        )

                        Spacer(Modifier.height(8.dp))
                        Button(onClick = {
                            pasteError = null
                            val sample = sampleState
                            if (sample == null) {
                                pasteError = "Importa prima una foto campione"
                            } else {
                                Engine.pasteLookOntoTarget(
                                    sampleBytes = sample.rawBytes,
                                    sampleFileName = sample.fileName,
                                    lookName = "Look da ${sample.fileName}",
                                    targetBytes = target.rawBytes,
                                    targetFileName = target.fileName,
                                    overrideStrength = overrideStrength,
                                ).fold(
                                    onSuccess = { adapted ->
                                        adaptedBitmap = decodeImageBitmapOrNull(adapted.renderedImageBytes)
                                        adaptedInfo = "Applicato: esposizione ${"%.2f".format(adapted.appliedExposureEv)} EV, " +
                                            "highlights ${adapted.appliedHighlights}, shadows ${adapted.appliedShadows}"
                                    },
                                    onFailure = { error -> pasteError = error.message ?: "Errore sconosciuto" }
                                )
                            }
                        }) {
                            Text("Incolla impostazioni (adattamento intelligente)")
                        }

                        pasteError?.let {
                            Spacer(Modifier.height(8.dp))
                            Text("Errore: $it", color = MaterialTheme.colors.error)
                        }

                        adaptedInfo?.let {
                            Spacer(Modifier.height(8.dp))
                            Text(it, style = MaterialTheme.typography.caption)
                        }
                    }
                }
            }
        }
    }
}
