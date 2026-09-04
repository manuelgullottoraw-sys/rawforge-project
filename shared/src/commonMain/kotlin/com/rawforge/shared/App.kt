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

/**
 * Stato di una foto importata dall'utente: tiene anche i bytes originali
 * (servono per rilanciare la Sintesi Armonica sui dati grezzi, non sulla
 * sola anteprima) insieme a ciò che la UI mostra.
 */
private data class ImportState(
    val fileName: String,
    val rawBytes: ByteArray,
    val cameraLabel: String?,
    val bitmap: ImageBitmap?,
)

/**
 * UI condivisa (identica su Android e Windows). Le prime due sezioni (stato
 * motore, preset XMP di esempio) sono la demo minimale già verificata in CI;
 * la terza sezione è il flusso reale: importare una foto qualunque (anche un
 * vero file RAW di una fotocamera), vederne l'anteprima decodificata dal
 * motore Rust, e applicarci la Sintesi Armonica Automatica esportando subito
 * un preset Lightroom `.xmp` (docs/ARCHITECTURE.md, §2, §4.1, §5). Il modulo
 * Develop, la libreria a griglia e il batch multi-foto (Smart-Batch
 * Contestuale, §4.2) restano da costruire sopra questa base.
 */
@Composable
fun RawForgeApp() {
    var engineInfo by remember { mutableStateOf<String?>(null) }
    var xmpPreview by remember { mutableStateOf<String?>(null) }

    var importState by remember { mutableStateOf<ImportState?>(null) }
    var importError by remember { mutableStateOf<String?>(null) }
    var harmonicXmp by remember { mutableStateOf<String?>(null) }
    var harmonicError by remember { mutableStateOf<String?>(null) }

    val launchFilePicker = rememberFilePickerLauncher { bytes, fileName ->
        importError = null
        harmonicXmp = null
        harmonicError = null
        Engine.importPhoto(bytes, fileName).fold(
            onSuccess = { photo ->
                importState = ImportState(
                    fileName = photo.fileName,
                    rawBytes = bytes,
                    cameraLabel = listOfNotNull(photo.cameraMake, photo.cameraModel)
                        .joinToString(" ")
                        .ifBlank { null },
                    bitmap = decodeImageBitmapOrNull(photo.previewImageBytes),
                )
            },
            onFailure = { error ->
                importState = null
                importError = error.message ?: "Errore sconosciuto durante l'importazione"
            }
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
                Button(onClick = { launchFilePicker() }) {
                    Text("Importa foto campione…")
                }

                importError?.let {
                    Spacer(Modifier.height(8.dp))
                    Text("Errore: $it", color = MaterialTheme.colors.error)
                }

                importState?.let { state ->
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
            }
        }
    }
}
