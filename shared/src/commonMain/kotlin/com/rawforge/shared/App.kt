package com.rawforge.shared

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.withContext

/**
 * Stato di una foto importata dall'utente. Tiene sia i bytes ORIGINALI del
 * file (`rawBytes`, servono a rilanciare l'analisi Rust sui dati grezzi — per
 * un file RAW è il file RAW vero, non l'anteprima) sia i bytes già
 * DECODIFICATI/sviluppati pronti da mostrare o esportare (`previewImageBytes`
 * — per un RAW è l'anteprima JPEG incorporata dalla fotocamera, per un
 * JPEG/PNG è semplicemente lo stesso file). Usato sia per la foto campione
 * sia per la foto target.
 */
private data class ImportState(
    val fileName: String,
    val rawBytes: ByteArray,
    val previewImageBytes: ByteArray,
    val cameraLabel: String?,
    val bitmap: ImageBitmap?,
)

/** Cosa mostra in un dato momento il riquadro della foto target: i bytes PNG
 * correnti (originale, incollato da Smart-Batch, o ri-renderizzato dopo una
 * modifica manuale — sempre la copia RIDOTTA per l'editing interattivo, non
 * quella a piena risoluzione, per restare veloce) e il relativo bitmap già
 * decodificato. */
private data class PreviewState(val bytes: ByteArray, val bitmap: ImageBitmap?)

// Palette scura in stile "camera oscura" da software di editing fotografico
// professionale (pannelli grigio molto scuro, testo quasi bianco, un solo
// accento blu per i controlli attivi) — non i colori Material di default.
private val PanelBackground = Color(0xFF1B1B1B)
private val PanelSurface = Color(0xFF262626)
private val PanelSurfaceRaised = Color(0xFF2F2F2F)
private val PanelDivider = Color(0xFF3A3A3A)
private val AccentBlue = Color(0xFF4FA8FF)
private val TextPrimary = Color(0xFFE6E6E6)
private val TextMuted = Color(0xFFA0A0A0)

private val RawForgeDarkColors = darkColors(
    primary = AccentBlue,
    primaryVariant = Color(0xFF2E6DA4),
    secondary = AccentBlue,
    background = PanelBackground,
    surface = PanelSurface,
    error = Color(0xFFEF5350),
    onPrimary = Color.White,
    onSecondary = Color.White,
    onBackground = TextPrimary,
    onSurface = TextPrimary,
    onError = Color.White,
)

/**
 * UI condivisa (identica su Android e Windows), in stile "Develop module" di
 * Lightroom: tema scuro, le due foto (campione/target) affiancate in modo che
 * si vedano entrambe senza dover scorrere, un pannello di editing manuale a
 * destra con gli slider che ri-renderizzano la foto DAL VIVO mentre si
 * trascina (non solo al rilascio), e un pulsante per esportare il risultato
 * a piena risoluzione. La libreria a griglia, le maschere locali e il batch
 * su centinaia di foto restano da costruire sopra questa base (vedi
 * `docs/ARCHITECTURE.md`).
 *
 * Il feedback dal vivo è possibile perché la foto da modificare viene aperta
 * UNA VOLTA (`Engine.openPhotoForEditing`) in una `PhotoEditSession` che la
 * tiene decodificata e già ridotta in memoria lato Rust: ogni modifica di
 * uno slider aggiorna solo `currentLook` (uno stato leggero), e un
 * `LaunchedEffect` osserva quello stato e richiama il rendering veloce
 * (`renderPreview`) in background, scartando automaticamente i risultati
 * di rendering ormai superati da una modifica più recente
 * (`collectLatest`) — così il rendering insegue sempre l'ultima posizione
 * dello slider invece di accodarsi in ritardo dietro ogni tick di
 * trascinamento.
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
    var pasteError by remember { mutableStateOf<String?>(null) }
    var renderError by remember { mutableStateOf<String?>(null) }

    // La sessione di editing per la foto target corrente (vedi il commento
    // sopra `RawForgeApp`): apre/decodifica una sola volta, non ad ogni
    // modifica. `null` finché nessuna foto target è aperta, o se l'apertura
    // è fallita (`sessionError`).
    var session by remember { mutableStateOf<PhotoEditSession?>(null) }
    var sessionError by remember { mutableStateOf<String?>(null) }

    var preview by remember { mutableStateOf<PreviewState?>(null) }
    var currentLook by remember { mutableStateOf(EditableLook()) }

    var exportBusy by remember { mutableStateOf(false) }
    var exportMessage by remember { mutableStateOf<String?>(null) }
    var exportError by remember { mutableStateOf<String?>(null) }

    // Chiude sempre la sessione precedente prima di sostituirla: la foto
    // decodificata che tiene in memoria lato Rust va liberata esplicitamente
    // (non c'è un finalizer automatico), altrimenti ogni cambio di foto
    // target perderebbe quella memoria per tutta la durata dell'app.
    fun resetEditingStateFor(target: ImportState?) {
        session?.close()
        session = null
        sessionError = null
        preview = target?.let { PreviewState(it.previewImageBytes, it.bitmap) }
        currentLook = EditableLook()
        pasteError = null
        renderError = null
        exportMessage = null
        exportError = null
        if (target != null) {
            Engine.openPhotoForEditing(target.rawBytes, target.fileName).fold(
                onSuccess = { opened -> session = opened },
                onFailure = { error -> sessionError = error.message ?: "Errore sconosciuto durante l'apertura della foto" }
            )
        }
    }

    // Rendering dal vivo: ad ogni modifica di `currentLook` (uno slider
    // spostato) richiama `renderPreview` sulla sessione corrente, in
    // background (`Dispatchers.Default`, il rendering CPU non deve bloccare
    // il thread della UI) e sempre sull'ULTIMO valore (`collectLatest`
    // annulla il rendering di un valore superato non appena ne arriva uno
    // più recente, invece di accodarli).
    LaunchedEffect(session) {
        val activeSession = session ?: return@LaunchedEffect
        snapshotFlow { currentLook }.collectLatest { look ->
            val result = withContext(Dispatchers.Default) { activeSession.renderPreview(look) }
            result.fold(
                onSuccess = { bytes -> preview = PreviewState(bytes, decodeImageBitmapOrNull(bytes)); renderError = null },
                onFailure = { error -> renderError = error.message ?: "Errore sconosciuto durante il rendering" }
            )
        }
    }

    // Se l'app viene chiusa (o questo composable smonta) con una sessione
    // ancora aperta, liberala comunque — legge lo stato corrente al momento
    // della dismissione, non quello catturato qui.
    DisposableEffect(Unit) {
        onDispose { session?.close() }
    }

    fun importInto(bytes: ByteArray, fileName: String, onDone: (ImportState) -> Unit, onError: (String) -> Unit) {
        Engine.importPhoto(bytes, fileName).fold(
            onSuccess = { photo ->
                onDone(
                    ImportState(
                        fileName = photo.fileName,
                        rawBytes = bytes,
                        previewImageBytes = photo.previewImageBytes,
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
        importInto(
            bytes,
            fileName,
            onDone = { sampleState = it },
            onError = { sampleError = it; sampleState = null }
        )
    }

    val launchTargetPicker = rememberFilePickerLauncher { bytes, fileName ->
        targetError = null
        importInto(
            bytes,
            fileName,
            onDone = { state -> targetState = state; resetEditingStateFor(state) },
            onError = { targetError = it; targetState = null; resetEditingStateFor(null) }
        )
    }

    val launchExport = rememberFileSaverLauncher(
        onSaved = { destination -> exportMessage = "Foto esportata: $destination"; exportError = null; exportBusy = false },
        onError = { error -> exportError = error; exportMessage = null; exportBusy = false },
    )

    MaterialTheme(colors = RawForgeDarkColors) {
        Surface(modifier = Modifier.fillMaxSize(), color = PanelBackground) {
            Column(modifier = Modifier.fillMaxSize()) {
                TopBar(
                    engineInfo = engineInfo,
                    xmpPreview = xmpPreview,
                    onCheckEngine = { engineInfo = Engine.versionInfo() },
                    onGenerateSampleXmp = { xmpPreview = Engine.generateSampleXmpPreset() },
                )
                Divider(color = PanelDivider, thickness = 1.dp)

                Row(modifier = Modifier.fillMaxSize()) {
                    // Colonna principale: le due foto affiancate + le azioni
                    // di "incolla impostazioni" ed esporta. Niente scroll
                    // qui: le immagini si ridimensionano per stare entrambe
                    // a video, come nel modulo Develop di Lightroom.
                    Column(modifier = Modifier.weight(1f).fillMaxHeight().padding(16.dp)) {
                        Row(modifier = Modifier.weight(1f).fillMaxWidth()) {
                            PhotoPanel(
                                modifier = Modifier.weight(1f).fillMaxHeight().padding(end = 8.dp),
                                title = "Campione (il look da copiare)",
                                state = sampleState,
                                error = sampleError,
                                onImportClick = { launchSamplePicker() },
                                importLabel = "Importa foto campione…",
                            ) {
                                Spacer(Modifier.height(8.dp))
                                Button(
                                    onClick = {
                                        val sample = sampleState ?: return@Button
                                        harmonicError = null
                                        Engine.extractLookAndExportXmp(
                                            sample.rawBytes,
                                            sample.fileName,
                                            "Look da ${sample.fileName}"
                                        ).fold(
                                            onSuccess = { xmp -> harmonicXmp = xmp },
                                            onFailure = { error -> harmonicError = error.message ?: "Errore sconosciuto" }
                                        )
                                    },
                                    colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised),
                                ) {
                                    Text("Esporta preset .xmp", style = MaterialTheme.typography.caption)
                                }
                                harmonicError?.let {
                                    Spacer(Modifier.height(4.dp))
                                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                                }
                                harmonicXmp?.let {
                                    Spacer(Modifier.height(4.dp))
                                    Text(
                                        it.take(300) + if (it.length > 300) "\n… (troncato)" else "",
                                        style = MaterialTheme.typography.caption,
                                        color = TextMuted,
                                    )
                                }
                            }

                            PhotoPanel(
                                modifier = Modifier.weight(1f).fillMaxHeight().padding(start = 8.dp),
                                title = "Foto da modificare",
                                state = targetState,
                                error = targetError ?: sessionError,
                                onImportClick = { launchTargetPicker() },
                                importLabel = "Apri foto da modificare…",
                                overrideBitmap = preview?.bitmap,
                            ) {
                                Spacer(Modifier.height(8.dp))
                                Button(
                                    onClick = {
                                        val activeSession = session ?: return@Button
                                        exportError = null
                                        exportBusy = true
                                        // A piena risoluzione, non la copia ridotta
                                        // usata per l'editing interattivo: qui la
                                        // velocità non è più la priorità, la qualità sì.
                                        activeSession.renderFullResolution(currentLook).fold(
                                            onSuccess = { bytes ->
                                                val suggested = (targetState?.fileName ?: "foto")
                                                    .substringBeforeLast('.') + "_rawforge.png"
                                                launchExport(bytes, suggested)
                                            },
                                            onFailure = { error ->
                                                exportError = error.message ?: "Errore durante il rendering per l'esportazione"
                                                exportBusy = false
                                            }
                                        )
                                    },
                                    enabled = session != null && !exportBusy,
                                    colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised),
                                ) {
                                    Text(if (exportBusy) "Esportazione…" else "Esporta foto…", style = MaterialTheme.typography.caption)
                                }
                                exportMessage?.let {
                                    Spacer(Modifier.height(4.dp))
                                    Text(it, style = MaterialTheme.typography.caption, color = TextMuted)
                                }
                                exportError?.let {
                                    Spacer(Modifier.height(4.dp))
                                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                                }
                            }
                        }

                        if (sampleState != null && targetState != null) {
                            Spacer(Modifier.height(12.dp))
                            Column(
                                modifier = Modifier.fillMaxWidth()
                                    .background(PanelSurface, RoundedCornerShape(6.dp))
                                    .padding(12.dp)
                            ) {
                                Text(
                                    "Intensità adattamento: ${(overrideStrength * 100).roundToInt()}% " +
                                        "(0% = impostazioni identiche alla foto campione, " +
                                        "100% = massimo adattamento intelligente alla scena)",
                                    style = MaterialTheme.typography.caption,
                                    color = TextMuted,
                                )
                                Slider(
                                    value = overrideStrength,
                                    onValueChange = { overrideStrength = it },
                                    modifier = Modifier.fillMaxWidth(),
                                )
                                Button(
                                    onClick = {
                                        val sample = sampleState
                                        val activeSession = session
                                        if (sample == null || activeSession == null) return@Button
                                        pasteError = null
                                        activeSession.pasteLookFromSample(
                                            sampleBytes = sample.rawBytes,
                                            sampleFileName = sample.fileName,
                                            lookName = "Look da ${sample.fileName}",
                                            overrideStrength = overrideStrength,
                                        ).fold(
                                            onSuccess = { adapted ->
                                                currentLook = adapted.appliedLook
                                                preview = PreviewState(
                                                    adapted.renderedImageBytes,
                                                    decodeImageBitmapOrNull(adapted.renderedImageBytes),
                                                )
                                            },
                                            onFailure = { error -> pasteError = error.message ?: "Errore sconosciuto" }
                                        )
                                    },
                                    enabled = session != null,
                                ) {
                                    Text("Incolla impostazioni (adattamento intelligente)")
                                }
                                pasteError?.let {
                                    Spacer(Modifier.height(4.dp))
                                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                                }
                                renderError?.let {
                                    Spacer(Modifier.height(4.dp))
                                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                                }
                            }
                        }
                    }

                    if (targetState != null) {
                        // Non `Divider`: quel componente applica al suo interno
                        // `.fillMaxWidth().height(thickness)` DOPO il modifier
                        // passato, quindi ignorerebbe la larghezza fissa/altezza
                        // piena richieste qui per un separatore verticale.
                        Box(modifier = Modifier.fillMaxHeight().width(1.dp).background(PanelDivider))
                        DevelopPanel(
                            modifier = Modifier.width(320.dp).fillMaxHeight(),
                            look = currentLook,
                            onEdit = { mutate -> currentLook = mutate(currentLook) },
                            onReset = { currentLook = EditableLook() },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun TopBar(
    engineInfo: String?,
    xmpPreview: String?,
    onCheckEngine: () -> Unit,
    onGenerateSampleXmp: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth().background(PanelSurface).padding(horizontal = 16.dp, vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("RawForge", style = MaterialTheme.typography.h6, fontWeight = FontWeight.Bold, color = TextPrimary)
            Text(
                "Motore RAW ultra-veloce — motore Rust collegato via UniFFI",
                style = MaterialTheme.typography.caption,
                color = TextMuted,
            )
            Spacer(Modifier.weight(1f))
            TextButton(onClick = onCheckEngine) { Text("Stato motore", style = MaterialTheme.typography.caption) }
            TextButton(onClick = onGenerateSampleXmp) { Text("Preset XMP demo", style = MaterialTheme.typography.caption) }
        }
        engineInfo?.let {
            Text(it, style = MaterialTheme.typography.caption, color = TextMuted)
        }
        xmpPreview?.let {
            Text(
                it.take(200) + if (it.length > 200) "…" else "",
                style = MaterialTheme.typography.caption,
                color = TextMuted,
            )
        }
    }
}

@Composable
private fun PhotoPanel(
    modifier: Modifier,
    title: String,
    state: ImportState?,
    error: String?,
    onImportClick: () -> Unit,
    importLabel: String,
    overrideBitmap: ImageBitmap? = null,
    actions: @Composable ColumnScope.() -> Unit = {},
) {
    val photo = state
    Column(
        modifier = modifier.background(PanelSurface, RoundedCornerShape(6.dp)).padding(12.dp),
    ) {
        Text(title, style = MaterialTheme.typography.subtitle2, color = TextPrimary, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Button(onClick = onImportClick, colors = ButtonDefaults.buttonColors(backgroundColor = AccentBlue)) {
            Text(importLabel, style = MaterialTheme.typography.caption)
        }
        error?.let {
            Spacer(Modifier.height(4.dp))
            Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
        }
        Box(
            modifier = Modifier.weight(1f).fillMaxWidth().padding(top = 8.dp)
                .background(PanelBackground, RoundedCornerShape(4.dp)),
            contentAlignment = Alignment.Center,
        ) {
            val bitmap = overrideBitmap ?: photo?.bitmap
            if (bitmap != null) {
                Image(
                    bitmap = bitmap,
                    contentDescription = title,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.fillMaxSize().clip(RoundedCornerShape(4.dp)),
                )
            } else if (photo != null) {
                Text("(anteprima non decodificabile, ma il motore ha letto i metadati)", style = MaterialTheme.typography.caption, color = TextMuted)
            } else {
                Text("Nessuna foto importata", style = MaterialTheme.typography.caption, color = TextMuted)
            }
        }
        photo?.let { s ->
            Spacer(Modifier.height(4.dp))
            Text(s.fileName, style = MaterialTheme.typography.caption, color = TextPrimary)
            s.cameraLabel?.let { Text(it, style = MaterialTheme.typography.caption, color = TextMuted) }
        }
        actions()
    }
}

@Composable
private fun DevelopPanel(
    modifier: Modifier,
    look: EditableLook,
    onEdit: ((EditableLook) -> EditableLook) -> Unit,
    onReset: () -> Unit,
) {
    Column(
        modifier = modifier.background(PanelSurface).verticalScroll(rememberScrollState()).padding(16.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Develop", style = MaterialTheme.typography.h6, fontWeight = FontWeight.Bold, color = TextPrimary)
            TextButton(onClick = onReset) { Text("Reimposta", style = MaterialTheme.typography.caption) }
        }
        Spacer(Modifier.height(8.dp))

        DevelopSection("Base") {
            FloatSlider("Esposizione (EV)", look.exposureEv, -5f..5f, { onEdit { l -> l.copy(exposureEv = it) } }) { "%.2f".format(it) }
            IntSlider("Contrasto", look.contrast, -100..100) { onEdit { l -> l.copy(contrast = it) } }
            IntSlider("Alte luci", look.highlights, -100..100) { onEdit { l -> l.copy(highlights = it) } }
            IntSlider("Ombre", look.shadows, -100..100) { onEdit { l -> l.copy(shadows = it) } }
            IntSlider("Bianchi", look.whites, -100..100) { onEdit { l -> l.copy(whites = it) } }
            IntSlider("Neri", look.blacks, -100..100) { onEdit { l -> l.copy(blacks = it) } }
        }

        DevelopSection("Colore") {
            IntSlider("Temperatura (K)", look.whiteBalanceTemp, 2000..12000) { onEdit { l -> l.copy(whiteBalanceTemp = it) } }
            IntSlider("Tinta", look.whiteBalanceTint, -100..100) { onEdit { l -> l.copy(whiteBalanceTint = it) } }
            IntSlider("Vivacità", look.vibrance, -100..100) { onEdit { l -> l.copy(vibrance = it) } }
            IntSlider("Saturazione", look.saturation, -100..100) { onEdit { l -> l.copy(saturation = it) } }
        }

        DevelopSection("Viraggio (Split Toning)") {
            IntSlider("Tinta ombre (°)", look.shadowHue, 0..360) { onEdit { l -> l.copy(shadowHue = it) } }
            IntSlider("Saturazione ombre", look.shadowSat, 0..100) { onEdit { l -> l.copy(shadowSat = it) } }
            IntSlider("Tinta luci (°)", look.highlightHue, 0..360) { onEdit { l -> l.copy(highlightHue = it) } }
            IntSlider("Saturazione luci", look.highlightSat, 0..100) { onEdit { l -> l.copy(highlightSat = it) } }
            IntSlider("Bilanciamento", look.splitToningBalance, -100..100) { onEdit { l -> l.copy(splitToningBalance = it) } }
        }

        Spacer(Modifier.height(8.dp))
        Text(
            "Curva tonale e HSL per singola banda colore non ancora modificabili a mano da qui — " +
                "prossimo incremento (il motore li applica già, dalla Sintesi Armonica).",
            style = MaterialTheme.typography.caption,
            color = TextMuted,
        )
    }
}

@Composable
private fun DevelopSection(title: String, content: @Composable ColumnScope.() -> Unit) {
    Text(title.uppercase(), style = MaterialTheme.typography.overline, color = AccentBlue, fontWeight = FontWeight.Bold)
    Spacer(Modifier.height(4.dp))
    Column(content = content)
    Spacer(Modifier.height(12.dp))
    Divider(color = PanelDivider, thickness = 1.dp)
    Spacer(Modifier.height(12.dp))
}

/** Slider intero: aggiorna lo stato ad ogni tick di trascinamento
 * (`onValueChange`), non solo al rilascio — il rendering dal vivo che ne
 * consegue è gestito centralmente dal `LaunchedEffect` in `RawForgeApp`. */
@Composable
private fun IntSlider(
    label: String,
    value: Int,
    range: IntRange,
    onChange: (Int) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.caption, color = TextPrimary)
            Text(value.toString(), style = MaterialTheme.typography.caption, color = TextMuted)
        }
        Slider(
            value = value.toFloat(),
            onValueChange = { onChange(it.roundToInt()) },
            valueRange = range.first.toFloat()..range.last.toFloat(),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun FloatSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    onChange: (Float) -> Unit,
    format: (Float) -> String,
) {
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.caption, color = TextPrimary)
            Text(format(value), style = MaterialTheme.typography.caption, color = TextMuted)
        }
        Slider(
            value = value,
            onValueChange = onChange,
            valueRange = range,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}
