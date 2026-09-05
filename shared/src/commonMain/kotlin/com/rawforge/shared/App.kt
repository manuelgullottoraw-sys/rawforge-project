package com.rawforge.shared

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlin.math.abs
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
 * decodificato. `shadowClipFraction`/`highlightClipFraction` (0f..1f, `null`
 * finché non è ancora arrivato un rendering dal motore — es. subito dopo
 * l'importazione) sono il segnale per "slider sicuri": la frazione di pixel
 * ai limiti dinamici dell'ULTIMO rendering, non dell'intero range possibile
 * di uno slider (vedi `RenderedPreview`). */
private data class PreviewState(
    val bytes: ByteArray,
    val bitmap: ImageBitmap?,
    val shadowClipFraction: Float? = null,
    val highlightClipFraction: Float? = null,
)

// Palette scura in stile "camera oscura" da software di editing fotografico
// professionale (pannelli grigio molto scuro, testo quasi bianco, un solo
// accento blu per i controlli attivi) — non i colori Material di default.
private val PanelBackground = Color(0xFF1B1B1B)
private val PanelSurface = Color(0xFF262626)
private val PanelSurfaceRaised = Color(0xFF2F2F2F)
private val PanelDivider = Color(0xFF3A3A3A)
private val AccentBlue = Color(0xFF4FA8FF)
private val AccentViolet = Color(0xFFB07CFF)
private val TextPrimary = Color(0xFFE6E6E6)
private val TextMuted = Color(0xFFA0A0A0)

// Gradiente d'accento (blu -> viola) usato con parsimonia — il marchio nella
// barra in alto, il bordo/sfondo dei pulsanti primari — per dare un unico
// punto di colore "vivo" a un'interfaccia altrimenti scura e neutra.
private val AccentGradient = Brush.linearGradient(listOf(AccentBlue, AccentViolet))

// Raggi degli angoli condivisi: più ampi delle versioni precedenti (era
// 4-6dp ovunque) per un look più morbido/contemporaneo.
private val CardShape = RoundedCornerShape(14.dp)
private val InnerShape = RoundedCornerShape(8.dp)
private val PillShape = RoundedCornerShape(10.dp)

/** Elevazione/sfondo/angoli condivisi da tutti i pannelli principali
 * (foto campione, foto target, Develop) — un `Card` vero con un'ombra
 * leggera al posto di un semplice riquadro colorato piatto. */
@Composable
private fun PanelCard(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Card(
        modifier = modifier,
        shape = CardShape,
        backgroundColor = PanelSurface,
        elevation = 6.dp,
        content = content,
    )
}

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

private fun lerp(from: Float, to: Float, t: Float): Float = from + (to - from) * t

/**
 * Applica il dial "Intensità edit" (`intensity`, 1f = 100% = l'editing esatto
 * così com'è, 0f = nessun editing, >1f = esagerato oltre il valore scelto):
 * interpola OGNI campo di `EditableLook` fra il suo valore NEUTRO (quello di
 * `EditableLook()`, o l'identità per la tone curve) e il valore attuale.
 * Pura — non modifica `currentLook`, viene ricalcolata solo al momento del
 * rendering/esportazione — così spostare il dial a 100% ritorna sempre
 * esattamente all'editing originale, senza arrotondamenti che si accumulano
 * avanti e indietro.
 *
 * La tinta del viraggio (`shadowHue`/`highlightHue`) resta invariata: sono
 * gradi assoluti senza un "neutro" naturale, e contano solo quando la
 * rispettiva saturazione (che invece scaliamo) è diversa da zero — a
 * intensità 0 il viraggio sparisce comunque perché la sua saturazione è 0.
 */
private fun EditableLook.scaledBy(intensity: Float): EditableLook {
    if (intensity == 1f) return this
    fun scaleInt(value: Int, range: IntRange): Int =
        lerp(0f, value.toFloat(), intensity).roundToInt().coerceIn(range.first, range.last)
    return copy(
        whiteBalanceTemp = lerp(5500f, whiteBalanceTemp.toFloat(), intensity).roundToInt().coerceIn(2000, 12000),
        whiteBalanceTint = scaleInt(whiteBalanceTint, -100..100),
        exposureEv = lerp(0f, exposureEv, intensity).coerceIn(-5f, 5f),
        contrast = scaleInt(contrast, -100..100),
        highlights = scaleInt(highlights, -100..100),
        shadows = scaleInt(shadows, -100..100),
        whites = scaleInt(whites, -100..100),
        blacks = scaleInt(blacks, -100..100),
        vibrance = scaleInt(vibrance, -100..100),
        saturation = scaleInt(saturation, -100..100),
        toneCurve = toneCurve.map { p -> p.copy(y = lerp(p.x.toFloat(), p.y.toFloat(), intensity).roundToInt().coerceIn(0, 255)) },
        hslHue = hslHue.map { scaleInt(it, -100..100) },
        hslSat = hslSat.map { scaleInt(it, -100..100) },
        hslLum = hslLum.map { scaleInt(it, -100..100) },
        shadowSat = scaleInt(shadowSat, 0..100),
        highlightSat = scaleInt(highlightSat, 0..100),
        splitToningBalance = scaleInt(splitToningBalance, -100..100),
        textureFine = scaleInt(textureFine, -100..100),
        textureMedium = scaleInt(textureMedium, -100..100),
        textureCoarse = scaleInt(textureCoarse, -100..100),
        // Zona B del WB a gradiente: stessa logica della zona A. `wbGradientEnabled`/
        // `wbGradientVertical`/`wbGradientPosition`/`wbGradientSpread` restano
        // INVARIATI (non passati a `copy`, quindi mantenuti automaticamente) —
        // sono parametri di GEOMETRIA del gradiente (dove/quanto è ampia la
        // transizione), non un'intensità di correzione: scalarli verso un
        // "neutro" non avrebbe un significato analogo a scalare un colore.
        whiteBalanceBTemp = lerp(5500f, whiteBalanceBTemp.toFloat(), intensity).roundToInt().coerceIn(2000, 12000),
        whiteBalanceBTint = scaleInt(whiteBalanceBTint, -100..100),
        noiseReductionLuma = scaleInt(noiseReductionLuma, 0..100),
        noiseReductionColor = scaleInt(noiseReductionColor, 0..100),
        // `subjectMaskEnabled`/`subjectMaskTarget` restano INVARIATI (stessa
        // logica di `wbGradientEnabled`/`wbGradientVertical` sopra): sono
        // scelte binarie di CONFIGURAZIONE della maschera (attiva/non attiva,
        // quale regione), non un'intensità continua da scalare — a intensità
        // 0 la maschera resta comunque innocua perché le TRE regolazioni
        // sotto, quelle sì scalate, tendono a 0.
        subjectMaskExposureEv = lerp(0f, subjectMaskExposureEv, intensity).coerceIn(-5f, 5f),
        subjectMaskContrast = scaleInt(subjectMaskContrast, -100..100),
        subjectMaskSaturation = scaleInt(subjectMaskSaturation, -100..100),
    )
}

/**
 * UI condivisa (identica su Android e Windows), in stile "Develop module" di
 * Lightroom: tema scuro, le due foto (campione/target) affiancate in modo che
 * si vedano entrambe senza dover scorrere, un pannello di editing manuale a
 * destra con gli slider che ri-renderizzano la foto DAL VIVO mentre si
 * trascina (non solo al rilascio), e un pulsante per esportare il risultato
 * a piena risoluzione, più una sezione "Maschera Soggetto/Sfondo" che applica
 * esposizione/contrasto/saturazione locali guidati da una mappa di salienza
 * (vedi `engine/README.md` per l'euristica e i suoi limiti dichiarati). La
 * libreria a griglia e il batch su centinaia di foto restano da costruire
 * sopra questa base (vedi `docs/ARCHITECTURE.md`).
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
    // Va chiamata prima di qualunque uso di `LibraryStorage`/
    // `rememberFolderPickerLauncher` (vedi il commento su
    // `InitializeLibraryPlatform` in `PlatformContext.kt`).
    InitializeLibraryPlatform()

    var engineInfo by remember { mutableStateOf<String?>(null) }
    var xmpPreview by remember { mutableStateOf<String?>(null) }

    var sampleState by remember { mutableStateOf<ImportState?>(null) }
    var sampleError by remember { mutableStateOf<String?>(null) }
    var harmonicXmp by remember { mutableStateOf<String?>(null) }
    var harmonicError by remember { mutableStateOf<String?>(null) }
    var presetSaveMessage by remember { mutableStateOf<String?>(null) }
    var presetSaveError by remember { mutableStateOf<String?>(null) }

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

    // Dial "Intensità edit" (vedi `EditableLook.scaledBy`): 1f = editing
    // esatto, 0f = nessun editing, oltre 1f lo esagera. Applicato solo al
    // momento del rendering/esportazione, mai su `currentLook` stesso.
    var editIntensity by remember { mutableStateOf(1f) }

    var exportBusy by remember { mutableStateOf(false) }
    var exportMessage by remember { mutableStateOf<String?>(null) }
    var exportError by remember { mutableStateOf<String?>(null) }
    // Bytes/nome del master TIFF in attesa di essere salvati SUBITO DOPO che
    // l'utente ha scelto la destinazione del JPEG (vedi `exportCurrentPhoto`
    // sotto): incatenare i due selettori nativi di destinazione invece di
    // lanciarli entrambi in un colpo solo evita di aprire due dialoghi di
    // sistema sovrapposti (comportamento non garantito, specie su Android,
    // dove il secondo `launch()` di un `ActivityResultLauncher` prima che il
    // primo sia tornato non è un flusso supportato).
    var pendingMasterTiffBytes by remember { mutableStateOf<ByteArray?>(null) }
    var pendingMasterTiffSuggestedName by remember { mutableStateOf<String?>(null) }

    // "Rileva soggetto" (vedi `engine/README.md`, sezione salienza): mappa in
    // scala di grigi ispezionabile, calcolata su richiesta esplicita
    // dell'utente (non ad ogni modifica: è un'analisi separata dal
    // rendering dal vivo) — NON collegata di per sé a nessuna regolazione:
    // è la sezione "Maschera Soggetto/Sfondo" del pannello Develop (dove
    // l'utente sceglie target/esposizione/contrasto/saturazione) che la usa
    // per davvero, lato motore, quando l'utente la attiva.
    var saliencyBitmap by remember { mutableStateOf<ImageBitmap?>(null) }
    var saliencyBusy by remember { mutableStateOf(false) }
    var saliencyError by remember { mutableStateOf<String?>(null) }

    // Modalità "Develop a schermo intero", in stile Lightroom: nasconde il
    // confronto affiancato con la foto campione e mostra solo la foto target,
    // grande, con il pannello Develop accanto — pensata per la fase di
    // editing fine, dopo un eventuale "Incolla impostazioni". `false` mostra
    // invece il confronto normale.
    var fullScreenEditing by remember { mutableStateOf(false) }

    // Libreria (docs/ARCHITECTURE.md — vedi `LibraryStorage` per l'onestà sui
    // limiti di questa prima versione: una sola cartella, non ricorsiva,
    // nessuna cache miniature su disco). `showLibrary` sostituisce
    // temporaneamente il confronto foto campione/target con la griglia,
    // esattamente come `fullScreenEditing` la sostituisce con la vista a
    // schermo intero — le due modalità non si aprono mai insieme.
    var showLibrary by remember { mutableStateOf(false) }
    var libraryFolder by remember { mutableStateOf<String?>(null) }
    var libraryPhotos by remember { mutableStateOf<List<LibraryPhotoEntry>>(emptyList()) }
    var libraryBusy by remember { mutableStateOf(false) }
    var libraryError by remember { mutableStateOf<String?>(null) }
    // Incrementato dal pulsante "Aggiorna": la Libreria non osserva il
    // filesystem da sola (limite dichiarato in `LibraryStorage`), quindi
    // rileggere l'elenco richiede un segnale esplicito — cambiare questo
    // valore fa ripartire lo stesso `LaunchedEffect` che legge `libraryFolder`.
    var libraryRefreshTick by remember { mutableStateOf(0) }

    fun openLibraryFolder(folderId: String) {
        LibraryStorage.rememberFolder(folderId)
        libraryFolder = folderId
    }

    // Al primo avvio, riapre in automatico l'ultima cartella Libreria
    // ricordata (persistenza fra riavvii — scelta dell'utente per un
    // "catalogo persistente" invece di un elenco valido solo per la sessione).
    LaunchedEffect(Unit) {
        LibraryStorage.rememberedFolder()?.let { libraryFolder = it }
    }

    // Rilegge l'elenco delle foto ogni volta che cambia la cartella scelta, o
    // che l'utente chiede esplicitamente un aggiornamento — non ad ogni
    // apertura della schermata Libreria, per non ripetere il lavoro se
    // l'utente la chiude e riapre senza che nulla sia cambiato sul disco.
    LaunchedEffect(libraryFolder, libraryRefreshTick) {
        val folder = libraryFolder ?: return@LaunchedEffect
        libraryError = null
        libraryBusy = true
        val result = withContext(Dispatchers.Default) { LibraryStorage.listPhotos(folder) }
        result.fold(
            onSuccess = { photos -> libraryPhotos = photos },
            onFailure = { error -> libraryPhotos = emptyList(); libraryError = error.message ?: "Errore durante la lettura della cartella" }
        )
        libraryBusy = false
    }

    // Elaborazione in batch (Smart-Batch Contestuale, docs/ARCHITECTURE.md
    // §4.2): applica il Look di UNA foto campione, adattato per ciascuna
    // foto target, a un'intera cartella insieme — a differenza di "Incolla
    // impostazioni" sopra (una foto target alla volta). Per ciascun file
    // produce SIA la foto renderizzata (PNG) SIA il preset `.xmp` — scelta
    // esplicita dell'utente, "Entrambi". Cartella di INPUT e cartella di
    // OUTPUT sono scelte separatamente (possono coincidere, vedi
    // `onUseInputAsOutput` in `BatchScreen`) e non condividono lo stato con
    // la Libreria: sono un catalogo di lavoro temporaneo per la singola
    // sessione di batch, non ricordato fra riavvii.
    var showBatch by remember { mutableStateOf(false) }
    var batchSampleState by remember { mutableStateOf<ImportState?>(null) }
    var batchSampleError by remember { mutableStateOf<String?>(null) }
    var batchInputFolder by remember { mutableStateOf<String?>(null) }
    var batchOutputFolder by remember { mutableStateOf<String?>(null) }
    var batchPhotos by remember { mutableStateOf<List<LibraryPhotoEntry>>(emptyList()) }
    var batchListBusy by remember { mutableStateOf(false) }
    var batchListError by remember { mutableStateOf<String?>(null) }
    // Stesso significato di `overrideStrength` sopra ("Intensità
    // adattamento"), ma una variabile di stato SEPARATA: il batch elabora
    // una cartella intera con un'unica intensità scelta prima di partire,
    // indipendente da quella eventualmente in uso nel pannello Develop.
    var batchOverrideStrength by remember { mutableStateOf(1f) }
    // Riduzione del rumore da applicare a OGNI foto del batch (0..100,
    // default 0 = comportamento invariato). Necessaria perché il Look che il
    // batch applica viene sempre ricalcolato da zero con la Sintesi Armonica
    // Automatica (`pasteLookFromSample`) — MAI dal pannello Develop della
    // foto campione, anche se l'utente lì avesse già impostato una riduzione
    // rumore a mano: un adattamento automatico non stima da solo QUANTO
    // rumore ridurre, quindi senza questo override il batch non applicherebbe
    // mai alcuna riduzione rumore, qualunque cosa l'utente avesse fatto nel
    // Develop. Overridden qui SOLO per il batch, applicato dopo l'adattamento
    // (vedi il ciclo sotto), non tocca in alcun modo l'editing manuale.
    var batchNoiseReductionLuma by remember { mutableStateOf(0) }
    var batchNoiseReductionColor by remember { mutableStateOf(0) }
    var batchRunning by remember { mutableStateOf(false) }
    var batchCancelRequested by remember { mutableStateOf(false) }
    var batchDone by remember { mutableStateOf(0) }
    var batchTotal by remember { mutableStateOf(0) }
    var batchCurrentFileName by remember { mutableStateOf<String?>(null) }
    var batchSuccessCount by remember { mutableStateOf(0) }
    // Limitata alle ultime 20 (`takeLast`): "grandi quantità di file" può
    // voler dire centinaia di foto — tenere un errore per ciascuna
    // renderebbe la schermata inutilizzabile molto prima che diventi utile;
    // il conteggio dei successi resta comunque esatto, solo il dettaglio
    // degli errori più vecchi non viene mostrato.
    var batchErrors by remember { mutableStateOf<List<String>>(emptyList()) }

    LaunchedEffect(batchInputFolder) {
        val folder = batchInputFolder ?: return@LaunchedEffect
        batchListError = null
        batchListBusy = true
        val result = withContext(Dispatchers.Default) { LibraryStorage.listPhotos(folder) }
        result.fold(
            onSuccess = { photos -> batchPhotos = photos },
            onFailure = { error -> batchPhotos = emptyList(); batchListError = error.message ?: "Errore durante la lettura della cartella" }
        )
        batchListBusy = false
    }

    // Il ciclo vero e proprio: parte quando `batchRunning` diventa vero
    // (pulsante "Avvia elaborazione" in `BatchScreen`), elabora i file UNO
    // ALLA VOLTA (non in parallelo — il motore Rust non è stato pensato per
    // essere chiamato da più thread contemporaneamente sulla stessa sessione,
    // e comunque su centinaia di foto il collo di bottiglia è la decodifica/
    // il rendering CPU, non l'attesa fra un file e l'altro) e si ferma da
    // sola a batch finito, o prima se l'utente annulla
    // (`batchCancelRequested`, controllato fra un file e il successivo — non
    // interrompe un file a metà, solo prima del prossimo).
    LaunchedEffect(batchRunning) {
        if (!batchRunning) return@LaunchedEffect
        val sample = batchSampleState
        val outputFolder = batchOutputFolder
        if (sample == null || outputFolder == null) {
            batchRunning = false
            return@LaunchedEffect
        }
        val photosSnapshot = batchPhotos
        batchDone = 0
        batchTotal = photosSnapshot.size
        batchSuccessCount = 0
        batchErrors = emptyList()
        val lookName = "Look da ${sample.fileName}"
        withContext(Dispatchers.Default) {
            for (entry in photosSnapshot) {
                if (batchCancelRequested) break
                batchCurrentFileName = entry.displayName
                val outcome = runCatching {
                    val targetBytes = LibraryStorage.readPhotoBytes(entry.id).getOrThrow()
                    val batchSession = Engine.openPhotoForEditing(targetBytes, entry.displayName).getOrThrow()
                    try {
                        val adapted = batchSession.pasteLookFromSample(
                            sampleBytes = sample.rawBytes,
                            sampleFileName = sample.fileName,
                            lookName = lookName,
                            overrideStrength = batchOverrideStrength,
                        ).getOrThrow()
                        // Override di riduzione rumore (vedi il commento su
                        // `batchNoiseReductionLuma`/`batchNoiseReductionColor`
                        // sopra): applicato DOPO l'adattamento automatico,
                        // sullo stesso Look che verrà sia renderizzato sia
                        // esportato come preset — mai due Look diversi fra
                        // PNG e .xmp per lo stesso file.
                        val finalLook = adapted.appliedLook.copy(
                            noiseReductionLuma = batchNoiseReductionLuma,
                            noiseReductionColor = batchNoiseReductionColor,
                        )
                        // JPEG ad alta qualità (92) + master TIFF 16 bit
                        // senza perdita, dallo STESSO rendering a piena
                        // risoluzione — per un RAW vero, dal demosaic
                        // completo del sensore (`Engine.openPhotoForEditing`
                        // ora lo esegue sempre per il file target, non più
                        // solo l'anteprima incorporata dalla fotocamera: vedi
                        // `PhotoEditSession.renderFullResolutionExport` lato
                        // Rust). Il master TIFF è la richiesta esplicita
                        // dell'utente per un uso editoriale ("non è ammessa
                        // la minima imperfezione"), qui applicata anche al
                        // batch, non solo alla foto singola.
                        val export = batchSession.renderFullResolutionExport(finalLook).getOrThrow()
                        val baseName = entry.displayName.substringBeforeLast('.').ifBlank { entry.displayName }
                        BatchExport.writeBytes(outputFolder, "${baseName}_rawforge.jpg", export.jpegBytes).getOrThrow()
                        BatchExport.writeBytes(outputFolder, "${baseName}_rawforge_master.tiff", export.masterTiffBytes).getOrThrow()
                        val xmpText = Engine.generateXmpForLook(finalLook.copy(name = baseName)).getOrThrow()
                        BatchExport.writeBytes(outputFolder, "${baseName}_rawforge.xmp", xmpText.encodeToByteArray()).getOrThrow()
                    } finally {
                        batchSession.close()
                    }
                }
                outcome.fold(
                    onSuccess = { batchSuccessCount++ },
                    onFailure = { error ->
                        batchErrors = (batchErrors + "${entry.displayName}: ${error.message ?: "errore sconosciuto"}").takeLast(20)
                    }
                )
                batchDone++
            }
        }
        batchCurrentFileName = null
        batchRunning = false
        batchCancelRequested = false
    }

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
        editIntensity = 1f
        pasteError = null
        renderError = null
        exportMessage = null
        exportError = null
        saliencyBitmap = null
        saliencyBusy = false
        saliencyError = null
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
        snapshotFlow { currentLook.scaledBy(editIntensity) }.collectLatest { look ->
            val result = withContext(Dispatchers.Default) { activeSession.renderPreview(look) }
            result.fold(
                onSuccess = { rendered ->
                    preview = PreviewState(
                        rendered.imageBytes,
                        decodeImageBitmapOrNull(rendered.imageBytes),
                        rendered.shadowClipFraction,
                        rendered.highlightClipFraction,
                    )
                    renderError = null
                },
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
        presetSaveMessage = null
        presetSaveError = null
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

    val launchFolderPicker = rememberFolderPickerLauncher { folderId -> openLibraryFolder(folderId) }

    // Apre una foto scelta dalla griglia della Libreria esattamente come
    // `launchTargetPicker` apre una foto scelta dal selettore di file: stessa
    // `importInto`, stesso `resetEditingStateFor`, così la Libreria è solo
    // un modo alternativo di arrivare alla stessa foto target, non un
    // percorso di editing separato.
    fun openLibraryPhoto(entry: LibraryPhotoEntry) {
        libraryError = null
        libraryBusy = true
        LibraryStorage.readPhotoBytes(entry.id).fold(
            onSuccess = { bytes ->
                targetError = null
                importInto(
                    bytes,
                    entry.displayName,
                    onDone = { state -> targetState = state; resetEditingStateFor(state); showLibrary = false; libraryBusy = false },
                    onError = { error -> targetError = error; targetState = null; resetEditingStateFor(null); libraryBusy = false }
                )
            },
            onFailure = { error ->
                libraryError = error.message ?: "Errore durante la lettura della foto"
                libraryBusy = false
            }
        )
    }

    val launchBatchSamplePicker = rememberFilePickerLauncher { bytes, fileName ->
        batchSampleError = null
        importInto(
            bytes,
            fileName,
            onDone = { state -> batchSampleState = state },
            onError = { error -> batchSampleError = error; batchSampleState = null }
        )
    }
    val launchBatchInputFolderPicker = rememberFolderPickerLauncher { folderId -> batchInputFolder = folderId }
    val launchBatchOutputFolderPicker = rememberFolderPickerLauncher { folderId -> batchOutputFolder = folderId }

    // Master TIFF a 16 bit senza perdita, accanto al JPEG di consegna —
    // richiesta esplicita dell'utente per un uso editoriale ("non è ammessa
    // la minima imperfezione"). Selettore di destinazione separato (stesso
    // principio di `launchExportXmp` accanto a `launchExport`), ma incatenato
    // DOPO che l'utente ha scelto la destinazione del JPEG (`launchExport`
    // sotto lo richiama dal proprio `onSaved`) invece di essere lanciato in
    // parallelo: vedi il commento su `pendingMasterTiffBytes` sopra.
    val launchExportMasterTiff = rememberMasterTiffSaverLauncher(
        onSaved = { destination -> exportMessage = "Foto + master esportati (master: $destination)"; exportError = null; exportBusy = false },
        onError = { error -> exportError = error; exportMessage = null; exportBusy = false },
    )
    val launchExport = rememberFileSaverLauncher(
        onSaved = { destination ->
            val tiffBytes = pendingMasterTiffBytes
            val tiffName = pendingMasterTiffSuggestedName
            pendingMasterTiffBytes = null
            pendingMasterTiffSuggestedName = null
            if (tiffBytes != null && tiffName != null) {
                exportMessage = "Foto esportata: $destination — scegli ora dove salvare il master TIFF"
                launchExportMasterTiff(tiffBytes, tiffName)
            } else {
                exportMessage = "Foto esportata: $destination"
                exportBusy = false
            }
            exportError = null
        },
        onError = { error ->
            pendingMasterTiffBytes = null
            pendingMasterTiffSuggestedName = null
            exportError = error
            exportMessage = null
            exportBusy = false
        },
    )

    // "Esporta preset .xmp": calcola il testo del preset e lo scrive subito
    // su un file vero, lasciando scegliere all'utente la cartella di
    // destinazione (e il nome file, precompilato) tramite lo stesso
    // selettore nativo già usato per l'esportazione della foto — prima non
    // veniva mai scritto su disco, solo mostrato come anteprima di testo.
    val launchExportXmp = rememberPresetSaverLauncher(
        onSaved = { destination -> presetSaveMessage = "Preset salvato: $destination"; presetSaveError = null },
        onError = { error -> presetSaveError = error; presetSaveMessage = null },
    )

    // Esporta a piena risoluzione la foto corrente con il Look attuale.
    // Condivisa fra il pulsante nel pannello di confronto e quello della
    // modalità a schermo intero: stessa azione, due punti da cui richiamarla.
    fun exportCurrentPhoto() {
        val activeSession = session ?: return
        exportError = null
        exportBusy = true
        // A piena risoluzione — per un RAW vero, dal demosaic completo del
        // sensore, non più solo l'anteprima incorporata dalla fotocamera —
        // non la copia ridotta usata per l'editing interattivo: qui la
        // velocità non è più la priorità, la qualità sì. Un solo rendering
        // f32 produce sia il JPEG di consegna sia il master TIFF a 16 bit
        // (vedi `PhotoEditSession.renderFullResolutionExport` lato Rust).
        activeSession.renderFullResolutionExport(currentLook.scaledBy(editIntensity)).fold(
            onSuccess = { export ->
                val baseName = (targetState?.fileName ?: "foto").substringBeforeLast('.')
                // Il master TIFF viene salvato SUBITO DOPO che l'utente ha
                // scelto la destinazione del JPEG (vedi `launchExport` sopra):
                // qui prepariamo solo bytes e nome suggerito.
                pendingMasterTiffBytes = export.masterTiffBytes
                pendingMasterTiffSuggestedName = "${baseName}_rawforge_master.tiff"
                launchExport(export.jpegBytes, "${baseName}_rawforge.jpg")
            },
            onFailure = { error ->
                pendingMasterTiffBytes = null
                pendingMasterTiffSuggestedName = null
                exportError = error.message ?: "Errore durante il rendering per l'esportazione"
                exportBusy = false
            }
        )
    }

    // "Rileva soggetto": lavora sulla stessa anteprima già decodificata
    // dall'import (`previewImageBytes`), MAI sui byte grezzi del file target
    // — `compute_subject_saliency_preview` lato Rust si aspetta byte già
    // decodificabili (JPEG/PNG), non un file RAW originale (vedi il commento
    // su quella funzione in `engine/ffi`); `previewImageBytes` è già nella
    // forma corretta per entrambi i casi (RAW e non), esattamente come per
    // `extractLookAndExportXmp`.
    fun detectSubject() {
        val target = targetState ?: return
        saliencyError = null
        saliencyBusy = true
        Engine.computeSubjectSaliencyPreview(target.previewImageBytes).fold(
            onSuccess = { bytes ->
                saliencyBitmap = decodeImageBitmapOrNull(bytes)
                saliencyBusy = false
            },
            onFailure = { error ->
                saliencyError = error.message ?: "Errore durante il rilevamento del soggetto"
                saliencyBusy = false
            }
        )
    }

    MaterialTheme(colors = RawForgeDarkColors) {
        Surface(modifier = Modifier.fillMaxSize(), color = PanelBackground) {
            Column(modifier = Modifier.fillMaxSize()) {
                TopBar(
                    engineInfo = engineInfo,
                    xmpPreview = xmpPreview,
                    onCheckEngine = { engineInfo = Engine.versionInfo() },
                    onGenerateSampleXmp = { xmpPreview = Engine.generateSampleXmpPreset() },
                    onOpenLibrary = { showLibrary = true; showBatch = false },
                    onOpenBatch = { showBatch = true; showLibrary = false },
                )
                // Sottile filo di colore al posto del solito `Divider` piatto:
                // è l'unico accento "vivo" nella barra in alto.
                Box(modifier = Modifier.fillMaxWidth().height(2.dp).background(AccentGradient))

                if (showLibrary) {
                    LibraryScreen(
                        folder = libraryFolder,
                        photos = libraryPhotos,
                        busy = libraryBusy,
                        error = libraryError,
                        onPickFolder = { launchFolderPicker() },
                        onRefresh = { libraryRefreshTick++ },
                        onSelect = { entry -> openLibraryPhoto(entry) },
                        onClose = { showLibrary = false },
                    )
                } else if (showBatch) {
                    BatchScreen(
                        sampleFileName = batchSampleState?.fileName,
                        sampleError = batchSampleError,
                        onPickSample = { launchBatchSamplePicker() },
                        inputFolder = batchInputFolder,
                        outputFolder = batchOutputFolder,
                        onPickInputFolder = { launchBatchInputFolderPicker() },
                        onPickOutputFolder = { launchBatchOutputFolderPicker() },
                        onUseInputAsOutput = { batchInputFolder?.let { batchOutputFolder = it } },
                        photosCount = batchPhotos.size,
                        listBusy = batchListBusy,
                        listError = batchListError,
                        overrideStrength = batchOverrideStrength,
                        onOverrideStrengthChange = { batchOverrideStrength = it },
                        noiseReductionLuma = batchNoiseReductionLuma,
                        onNoiseReductionLumaChange = { batchNoiseReductionLuma = it },
                        noiseReductionColor = batchNoiseReductionColor,
                        onNoiseReductionColorChange = { batchNoiseReductionColor = it },
                        running = batchRunning,
                        done = batchDone,
                        total = batchTotal,
                        currentFileName = batchCurrentFileName,
                        successCount = batchSuccessCount,
                        errors = batchErrors,
                        canStart = !batchRunning && batchSampleState != null && batchOutputFolder != null && batchPhotos.isNotEmpty(),
                        onStart = { batchCancelRequested = false; batchRunning = true },
                        onCancel = { batchCancelRequested = true },
                        onClose = { showBatch = false },
                    )
                } else if (fullScreenEditing && targetState != null && session != null) {
                    FullScreenDevelopView(
                        title = targetState?.fileName ?: "Foto",
                        bitmap = preview?.bitmap,
                        look = currentLook,
                        onEdit = { mutate -> currentLook = mutate(currentLook) },
                        onReset = { currentLook = EditableLook(); editIntensity = 1f },
                        editIntensity = editIntensity,
                        onEditIntensityChange = { editIntensity = it },
                        onExit = { fullScreenEditing = false },
                        onExport = { exportCurrentPhoto() },
                        exportBusy = exportBusy,
                        exportMessage = exportMessage,
                        exportError = exportError,
                        renderError = renderError,
                        shadowClipFraction = preview?.shadowClipFraction,
                        highlightClipFraction = preview?.highlightClipFraction,
                    )
                } else {
                // Sotto una soglia di larghezza (telefoni, sia in verticale
                // che orizzontale su molti dispositivi) il layout "desktop"
                // qui sotto — due foto affiancate a sinistra + un pannello
                // Develop a larghezza FISSA di 320dp a destra — non ha spazio
                // per respirare: su un telefono largo 360-400dp il solo
                // pannello Develop consuma quasi tutta la larghezza
                // disponibile, lasciando alle due foto pochissimi pixel e
                // costringendo Compose a schiacciare in verticale il resto
                // del contenuto (pulsanti, slider) pur di farcelo stare —
                // il problema segnalato dall'utente ("mi comprime in
                // verticale la UI e l'app diventa inutilizzabile").
                // `BoxWithConstraints` misura la larghezza disponibile e
                // sceglie fra due composizioni alternative dello STESSO
                // contenuto (stessi componenti, stesse azioni, definite una
                // volta sola qui sotto come lambda locali): affiancata
                // (larga) o impilata verticalmente (stretta) — non un
                // ridimensionamento proporzionale, che a queste larghezze
                // non basterebbe comunque.
                BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
                    val isWide = maxWidth > 700.dp

                    val sampleActions: @Composable ColumnScope.() -> Unit = {
                        Spacer(Modifier.height(8.dp))
                        Button(
                            onClick = {
                                val sample = sampleState ?: return@Button
                                harmonicError = null
                                presetSaveMessage = null
                                presetSaveError = null
                                Engine.extractLookAndExportXmp(
                                    sample.rawBytes,
                                    sample.fileName,
                                    "Look da ${sample.fileName}"
                                ).fold(
                                    onSuccess = { xmp ->
                                        harmonicXmp = xmp
                                        // Subito dopo aver calcolato il preset, chiede
                                        // all'utente dove salvarlo — non solo
                                        // un'anteprima di testo come prima.
                                        val suggested = sample.fileName.substringBeforeLast('.') + "_look.xmp"
                                        launchExportXmp(xmp, suggested)
                                    },
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
                        presetSaveError?.let {
                            Spacer(Modifier.height(4.dp))
                            Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                        }
                        presetSaveMessage?.let {
                            Spacer(Modifier.height(4.dp))
                            Text(it, style = MaterialTheme.typography.caption, color = TextMuted)
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

                    val targetActions: @Composable ColumnScope.() -> Unit = {
                        Spacer(Modifier.height(8.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(
                                onClick = { exportCurrentPhoto() },
                                enabled = session != null && !exportBusy,
                                shape = PillShape,
                                colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised),
                            ) {
                                Text(if (exportBusy) "Esportazione…" else "Esporta foto…", style = MaterialTheme.typography.caption)
                            }
                            // Modalità "Develop" a schermo intero, in stile
                            // Lightroom: foto grande + pannello di editing,
                            // niente confronto affiancato con il campione —
                            // pensata per il fine-tuning dopo un eventuale
                            // "Incolla impostazioni" (ma disponibile anche
                            // per editare a mano da zero).
                            Button(
                                onClick = { fullScreenEditing = true },
                                enabled = session != null,
                                shape = PillShape,
                                colors = ButtonDefaults.buttonColors(backgroundColor = AccentBlue),
                            ) {
                                Text("Modifica a schermo intero", style = MaterialTheme.typography.caption)
                            }
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

                    // Card "Incolla impostazioni": identica in entrambi i
                    // layout, si autoesclude finché non sono aperte sia la
                    // foto campione sia quella da modificare.
                    val pasteSettingsCard: @Composable () -> Unit = {
                        if (sampleState != null && targetState != null) {
                            PanelCard(modifier = Modifier.fillMaxWidth()) {
                                Column(modifier = Modifier.padding(16.dp)) {
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
                                        shape = PillShape,
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
                                                    editIntensity = 1f
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
                    }

                    if (isWide) {
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
                                        actions = sampleActions,
                                    )

                                    PhotoPanel(
                                        modifier = Modifier.weight(1f).fillMaxHeight().padding(start = 8.dp),
                                        title = "Foto da modificare",
                                        state = targetState,
                                        error = targetError ?: sessionError,
                                        onImportClick = { launchTargetPicker() },
                                        importLabel = "Apri foto da modificare…",
                                        overrideBitmap = preview?.bitmap,
                                        actions = targetActions,
                                    )
                                }

                                Spacer(Modifier.height(12.dp))
                                pasteSettingsCard()
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
                                    onReset = { currentLook = EditableLook(); editIntensity = 1f },
                                    editIntensity = editIntensity,
                                    onEditIntensityChange = { editIntensity = it },
                                    shadowClipFraction = preview?.shadowClipFraction,
                                    highlightClipFraction = preview?.highlightClipFraction,
                                    onDetectSubject = { detectSubject() },
                                    saliencyBitmap = saliencyBitmap,
                                    saliencyBusy = saliencyBusy,
                                    saliencyError = saliencyError,
                                )
                            }
                        }
                    } else {
                        // Layout stretto (telefono): tutto impilato in
                        // verticale invece che affiancato. Le due foto
                        // restano una accanto all'altra (il confronto
                        // campione/target è il punto centrale di questa
                        // schermata, e anche un telefono stretto ha
                        // abbastanza larghezza per due miniature verticali:
                        // sono per lo più foto in verticale, come quelle
                        // usate per scoprire questo bug) ma con un'altezza
                        // FISSA invece di condividere `weight(1f)` con il
                        // resto della pagina; il pannello Develop passa da
                        // barra laterale a larghezza fissa a blocco a piena
                        // larghezza SOTTO le foto, con un'altezza assegnata
                        // via `weight` invece di `fillMaxHeight()` (che qui
                        // non avrebbe un limite finito da riempire) — il suo
                        // scroll verticale interno (già presente, vedi
                        // `DevelopPanel`) resta invariato, ora semplicemente
                        // su un'area più bassa e larga invece che stretta e
                        // alta.
                        Column(modifier = Modifier.fillMaxSize()) {
                            // Niente `weight` qui: questo blocco (foto ad
                            // altezza fissa + card "incolla impostazioni")
                            // deve occupare solo lo spazio che il suo
                            // contenuto richiede davvero, non una quota
                            // proporzionale fissa dello schermo — altrimenti
                            // uno split 50/50 con `DevelopPanel` sotto
                            // rischierebbe di tagliare il contenuto di
                            // QUESTO blocco (che non ha scroll proprio) su
                            // schermi bassi, mentre `DevelopPanel` (che ha
                            // già il suo scroll interno) può assorbire
                            // tranquillamente tutto lo spazio residuo,
                            // qualunque esso sia.
                            Column(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
                                Row(modifier = Modifier.height(240.dp).fillMaxWidth()) {
                                    PhotoPanel(
                                        modifier = Modifier.weight(1f).fillMaxHeight().padding(end = 8.dp),
                                        title = "Campione (il look da copiare)",
                                        state = sampleState,
                                        error = sampleError,
                                        onImportClick = { launchSamplePicker() },
                                        importLabel = "Importa foto campione…",
                                        actions = sampleActions,
                                    )

                                    PhotoPanel(
                                        modifier = Modifier.weight(1f).fillMaxHeight().padding(start = 8.dp),
                                        title = "Foto da modificare",
                                        state = targetState,
                                        error = targetError ?: sessionError,
                                        onImportClick = { launchTargetPicker() },
                                        importLabel = "Apri foto da modificare…",
                                        overrideBitmap = preview?.bitmap,
                                        actions = targetActions,
                                    )
                                }

                                Spacer(Modifier.height(12.dp))
                                pasteSettingsCard()
                            }

                            if (targetState != null) {
                                Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(PanelDivider))
                                DevelopPanel(
                                    modifier = Modifier.fillMaxWidth().weight(1f),
                                    look = currentLook,
                                    onEdit = { mutate -> currentLook = mutate(currentLook) },
                                    onReset = { currentLook = EditableLook(); editIntensity = 1f },
                                    editIntensity = editIntensity,
                                    onEditIntensityChange = { editIntensity = it },
                                    shadowClipFraction = preview?.shadowClipFraction,
                                    highlightClipFraction = preview?.highlightClipFraction,
                                    shape = RoundedCornerShape(topStart = 14.dp, topEnd = 14.dp),
                                    onDetectSubject = { detectSubject() },
                                    saliencyBitmap = saliencyBitmap,
                                    saliencyBusy = saliencyBusy,
                                    saliencyError = saliencyError,
                                )
                            }
                        }
                    }
                }
                }
            }
        }
    }
}

/**
 * Modalità "Develop" a schermo intero, in stile Lightroom: niente confronto
 * affiancato con la foto campione, solo la foto target grande al centro con
 * il pannello di editing manuale accanto — pensata per il fine-tuning dopo
 * un eventuale "Incolla impostazioni" (ma disponibile anche per editare a
 * mano da zero, senza aver incollato nulla).
 */
@Composable
private fun FullScreenDevelopView(
    title: String,
    bitmap: ImageBitmap?,
    look: EditableLook,
    onEdit: ((EditableLook) -> EditableLook) -> Unit,
    onReset: () -> Unit,
    editIntensity: Float,
    onEditIntensityChange: (Float) -> Unit,
    onExit: () -> Unit,
    onExport: () -> Unit,
    exportBusy: Boolean,
    exportMessage: String?,
    exportError: String?,
    renderError: String?,
    shadowClipFraction: Float?,
    highlightClipFraction: Float?,
) {
    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().background(PanelSurface).padding(horizontal = 20.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = onExit, shape = PillShape) { Text("← Torna al confronto", style = MaterialTheme.typography.caption) }
            Spacer(Modifier.weight(1f))
            Text(title, style = MaterialTheme.typography.subtitle2, color = TextPrimary, fontWeight = FontWeight.Bold)
            Spacer(Modifier.weight(1f))
            Button(
                onClick = onExport,
                enabled = !exportBusy,
                shape = PillShape,
                colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised),
            ) {
                Text(if (exportBusy) "Esportazione…" else "Esporta foto…", style = MaterialTheme.typography.caption)
            }
        }
        Box(modifier = Modifier.fillMaxWidth().height(2.dp).background(AccentGradient))
        (exportError ?: exportMessage)?.let {
            Text(
                it,
                style = MaterialTheme.typography.caption,
                color = if (exportError != null) MaterialTheme.colors.error else TextMuted,
                modifier = Modifier.fillMaxWidth().background(PanelSurface).padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
        // Stessa logica di `App` (vedi commento esteso lì): sotto una certa
        // larghezza un pannello Develop a 360dp fissi accanto alla foto non
        // lascia spazio a nessuno dei due. `BoxWithConstraints` sceglie fra
        // affiancato (largo) e impilato (stretto, foto sopra/pannello sotto,
        // divisi a metà — qui, a differenza della schermata di confronto,
        // NON c'è un blocco a contenuto fisso da preservare: la foto si
        // adatta a qualunque riquadro le venga dato, quindi uno split 1:1
        // va bene per entrambi).
        BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
            val isWide = maxWidth > 700.dp

            val photoBox: @Composable (Modifier) -> Unit = { boxModifier ->
                Box(
                    modifier = boxModifier.background(PanelBackground).padding(16.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    if (bitmap != null) {
                        ZoomableImage(
                            bitmap = bitmap,
                            contentDescription = title,
                            resetKey = title,
                            modifier = Modifier.fillMaxSize(),
                        )
                    } else {
                        Text("Rendering in corso…", style = MaterialTheme.typography.caption, color = TextMuted)
                    }
                    renderError?.let {
                        Text(
                            "Errore: $it",
                            color = MaterialTheme.colors.error,
                            style = MaterialTheme.typography.caption,
                            modifier = Modifier.align(Alignment.BottomCenter).background(PanelSurface).padding(8.dp),
                        )
                    }
                }
            }

            if (isWide) {
                Row(modifier = Modifier.fillMaxSize()) {
                    photoBox(Modifier.weight(1f).fillMaxHeight())
                    Box(modifier = Modifier.fillMaxHeight().width(1.dp).background(PanelDivider))
                    DevelopPanel(
                        modifier = Modifier.width(360.dp).fillMaxHeight(),
                        look = look,
                        onEdit = onEdit,
                        onReset = onReset,
                        editIntensity = editIntensity,
                        onEditIntensityChange = onEditIntensityChange,
                        shadowClipFraction = shadowClipFraction,
                        highlightClipFraction = highlightClipFraction,
                    )
                }
            } else {
                Column(modifier = Modifier.fillMaxSize()) {
                    photoBox(Modifier.weight(1f).fillMaxWidth())
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(PanelDivider))
                    DevelopPanel(
                        modifier = Modifier.fillMaxWidth().weight(1f),
                        look = look,
                        onEdit = onEdit,
                        onReset = onReset,
                        editIntensity = editIntensity,
                        onEditIntensityChange = onEditIntensityChange,
                        shadowClipFraction = shadowClipFraction,
                        highlightClipFraction = highlightClipFraction,
                        shape = RoundedCornerShape(topStart = 14.dp, topEnd = 14.dp),
                    )
                }
            }
        }
    }
}

/**
 * Griglia della Libreria (vedi `LibraryStorage` per l'architettura e i
 * limiti dichiarati di questa prima versione). Sostituisce il confronto
 * foto campione/target finché `onClose` non viene invocato — proprio come
 * `FullScreenDevelopView` fa per `fullScreenEditing`.
 */
@Composable
private fun LibraryScreen(
    folder: String?,
    photos: List<LibraryPhotoEntry>,
    busy: Boolean,
    error: String?,
    onPickFolder: () -> Unit,
    onRefresh: () -> Unit,
    onSelect: (LibraryPhotoEntry) -> Unit,
    onClose: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxSize().background(PanelBackground).padding(20.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Libreria", style = MaterialTheme.typography.h6, fontWeight = FontWeight.Bold, color = TextPrimary)
            Spacer(Modifier.width(4.dp))
            Text(
                folder ?: "Nessuna cartella scelta",
                style = MaterialTheme.typography.caption,
                color = TextMuted,
                modifier = Modifier.weight(1f),
            )
            if (busy) {
                CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
            }
            TextButton(onClick = onPickFolder) { Text("Scegli cartella", style = MaterialTheme.typography.caption) }
            if (folder != null) {
                TextButton(onClick = onRefresh) { Text("Aggiorna", style = MaterialTheme.typography.caption) }
            }
            TextButton(onClick = onClose) { Text("Chiudi", style = MaterialTheme.typography.caption) }
        }
        error?.let {
            Spacer(Modifier.height(4.dp))
            Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
        }
        Spacer(Modifier.height(16.dp))
        when {
            folder == null -> Text(
                "Scegli una cartella per popolare la Libreria.",
                style = MaterialTheme.typography.body2,
                color = TextMuted,
            )
            photos.isEmpty() && !busy -> Text(
                "Nessuna foto riconosciuta in questa cartella.",
                style = MaterialTheme.typography.body2,
                color = TextMuted,
            )
            else -> LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 130.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(photos, key = { it.id }) { entry ->
                    Column(
                        modifier = Modifier
                            .clickable { onSelect(entry) }
                            .clip(RoundedCornerShape(6.dp)),
                    ) {
                        LibraryThumbnail(
                            entry = entry,
                            modifier = Modifier.fillMaxWidth().height(96.dp),
                        )
                        Text(
                            entry.displayName,
                            style = MaterialTheme.typography.caption,
                            color = TextPrimary,
                            maxLines = 1,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                }
            }
        }
    }
}

/**
 * Miniatura di una foto della Libreria: decodifica la propria anteprima in
 * background al primo utilizzo (nessuna cache — vedi i limiti dichiarati in
 * `LibraryStorage`), passando per `Engine.importPhoto` esattamente come
 * `importInto` — è l'unico modo per ottenere un'immagine mostrabile anche
 * per un file RAW, che `decodeImageBitmapOrNull` da solo non sa decodificare.
 */
@Composable
private fun LibraryThumbnail(entry: LibraryPhotoEntry, modifier: Modifier = Modifier) {
    val bitmap by produceState<ImageBitmap?>(initialValue = null, entry.id) {
        value = withContext(Dispatchers.Default) {
            LibraryStorage.readPhotoBytes(entry.id).getOrNull()?.let { bytes ->
                Engine.importPhoto(bytes, entry.displayName).getOrNull()?.previewImageBytes?.let(::decodeImageBitmapOrNull)
            }
        }
    }
    Box(modifier = modifier.background(PanelSurfaceRaised), contentAlignment = Alignment.Center) {
        val currentBitmap = bitmap
        if (currentBitmap != null) {
            Image(
                bitmap = currentBitmap,
                contentDescription = entry.displayName,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
        } else {
            CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
        }
    }
}

/**
 * Schermata dell'elaborazione in batch (vedi il commento su `showBatch` in
 * `RawForgeApp` per l'architettura completa). Tutta la logica vive lì
 * dentro (stato + `LaunchedEffect`); questo composable è solo la UI.
 */
@Composable
private fun BatchScreen(
    sampleFileName: String?,
    sampleError: String?,
    onPickSample: () -> Unit,
    inputFolder: String?,
    outputFolder: String?,
    onPickInputFolder: () -> Unit,
    onPickOutputFolder: () -> Unit,
    onUseInputAsOutput: () -> Unit,
    photosCount: Int,
    listBusy: Boolean,
    listError: String?,
    overrideStrength: Float,
    onOverrideStrengthChange: (Float) -> Unit,
    noiseReductionLuma: Int,
    onNoiseReductionLumaChange: (Int) -> Unit,
    noiseReductionColor: Int,
    onNoiseReductionColorChange: (Int) -> Unit,
    running: Boolean,
    done: Int,
    total: Int,
    currentFileName: String?,
    successCount: Int,
    errors: List<String>,
    canStart: Boolean,
    onStart: () -> Unit,
    onCancel: () -> Unit,
    onClose: () -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxSize().background(PanelBackground).padding(20.dp).verticalScroll(rememberScrollState()),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Elaborazione in batch", style = MaterialTheme.typography.h6, fontWeight = FontWeight.Bold, color = TextPrimary)
            Spacer(Modifier.weight(1f))
            TextButton(onClick = onClose, enabled = !running) { Text("Chiudi", style = MaterialTheme.typography.caption) }
        }
        Text(
            "Applica il Look di UNA foto campione, adattato foto per foto (Smart-Batch), a tutte le foto " +
                "di una cartella — per ciascuna produce sia il PNG renderizzato sia il preset .xmp.",
            style = MaterialTheme.typography.caption,
            color = TextMuted,
        )
        Spacer(Modifier.height(16.dp))

        PanelCard(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text("1. Foto campione (da cui copiare il Look)", style = MaterialTheme.typography.subtitle2, color = TextPrimary)
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(onClick = onPickSample, enabled = !running, colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised)) {
                        Text("Scegli foto campione", style = MaterialTheme.typography.caption)
                    }
                    Text(
                        sampleFileName ?: "Nessuna foto scelta",
                        style = MaterialTheme.typography.caption,
                        color = TextMuted,
                    )
                }
                sampleError?.let {
                    Spacer(Modifier.height(4.dp))
                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                }
            }
        }
        Spacer(Modifier.height(12.dp))

        PanelCard(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text("2. Cartelle", style = MaterialTheme.typography.subtitle2, color = TextPrimary)
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(onClick = onPickInputFolder, enabled = !running, colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised)) {
                        Text("Cartella di input", style = MaterialTheme.typography.caption)
                    }
                    Text(inputFolder ?: "Nessuna cartella scelta", style = MaterialTheme.typography.caption, color = TextMuted)
                    if (listBusy) {
                        CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                    }
                }
                listError?.let {
                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                }
                if (inputFolder != null && !listBusy) {
                    Text(
                        "$photosCount foto riconosciute in questa cartella.",
                        style = MaterialTheme.typography.caption,
                        color = TextMuted,
                    )
                }
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(onClick = onPickOutputFolder, enabled = !running, colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised)) {
                        Text("Cartella di output", style = MaterialTheme.typography.caption)
                    }
                    Text(outputFolder ?: "Nessuna cartella scelta", style = MaterialTheme.typography.caption, color = TextMuted)
                    if (inputFolder != null) {
                        TextButton(onClick = onUseInputAsOutput, enabled = !running) {
                            Text("Come input", style = MaterialTheme.typography.caption)
                        }
                    }
                }
            }
        }
        Spacer(Modifier.height(12.dp))

        PanelCard(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    "3. Intensità adattamento: ${(overrideStrength * 100).roundToInt()}% " +
                        "(0% = impostazioni identiche alla foto campione, 100% = massimo adattamento intelligente per scena)",
                    style = MaterialTheme.typography.caption,
                    color = TextMuted,
                )
                Slider(
                    value = overrideStrength,
                    onValueChange = onOverrideStrengthChange,
                    enabled = !running,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
        Spacer(Modifier.height(12.dp))

        PanelCard(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    "4. Riduzione rumore (opzionale)",
                    style = MaterialTheme.typography.subtitle2,
                    color = TextPrimary,
                )
                Text(
                    "L'adattamento automatico (sopra) non stima da sé quanto rumore ridurre — se le foto " +
                        "target hanno grana visibile, alzare qui uno o entrambi i valori: si applicano a TUTTE " +
                        "le foto del batch, in aggiunta al Look copiato dalla foto campione.",
                    style = MaterialTheme.typography.caption,
                    color = TextMuted,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "Luminanza: $noiseReductionLuma",
                    style = MaterialTheme.typography.caption,
                    color = TextMuted,
                )
                Slider(
                    value = noiseReductionLuma.toFloat(),
                    onValueChange = { onNoiseReductionLumaChange(it.roundToInt()) },
                    valueRange = 0f..100f,
                    enabled = !running,
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(
                    "Colore: $noiseReductionColor",
                    style = MaterialTheme.typography.caption,
                    color = TextMuted,
                )
                Slider(
                    value = noiseReductionColor.toFloat(),
                    onValueChange = { onNoiseReductionColorChange(it.roundToInt()) },
                    valueRange = 0f..100f,
                    enabled = !running,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
        Spacer(Modifier.height(16.dp))

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button(
                shape = PillShape,
                onClick = onStart,
                enabled = canStart,
            ) {
                Text(if (running) "Elaborazione in corso…" else "Avvia elaborazione", style = MaterialTheme.typography.caption)
            }
            if (running) {
                Button(
                    shape = PillShape,
                    onClick = onCancel,
                    colors = ButtonDefaults.buttonColors(backgroundColor = PanelSurfaceRaised),
                ) {
                    Text("Annulla", style = MaterialTheme.typography.caption)
                }
            }
        }

        if (running || total > 0) {
            Spacer(Modifier.height(16.dp))
            val progress = if (total > 0) done.toFloat() / total.toFloat() else 0f
            LinearProgressIndicator(progress = progress, modifier = Modifier.fillMaxWidth())
            Spacer(Modifier.height(6.dp))
            Text(
                "$done / $total elaborate — $successCount riuscite" +
                    (currentFileName?.let { " — in corso: $it" } ?: ""),
                style = MaterialTheme.typography.caption,
                color = TextMuted,
            )
        }

        if (errors.isNotEmpty()) {
            Spacer(Modifier.height(12.dp))
            Text(
                "Errori (ultimi ${errors.size}):",
                style = MaterialTheme.typography.caption,
                color = MaterialTheme.colors.error,
            )
            errors.forEach {
                Text(it, style = MaterialTheme.typography.caption, color = MaterialTheme.colors.error)
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
    onOpenLibrary: () -> Unit,
    onOpenBatch: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth().background(PanelSurface).padding(horizontal = 20.dp, vertical = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            // Piccolo marchio: un quadrato arrotondato col gradiente d'accento,
            // al posto di un'icona vera (nessuna libreria icone in questo
            // modulo) — comunque un punto di riconoscibilità in più.
            Box(modifier = Modifier.size(28.dp).clip(RoundedCornerShape(8.dp)).background(AccentGradient))
            Text("RawForge", style = MaterialTheme.typography.h6, fontWeight = FontWeight.Bold, color = TextPrimary)
            Spacer(Modifier.width(4.dp))
            Text(
                "Motore RAW ultra-veloce — motore Rust collegato via UniFFI",
                style = MaterialTheme.typography.caption,
                color = TextMuted,
            )
            Spacer(Modifier.weight(1f))
            TextButton(onClick = onOpenLibrary) { Text("Libreria", style = MaterialTheme.typography.caption) }
            TextButton(onClick = onOpenBatch) { Text("Batch", style = MaterialTheme.typography.caption) }
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
    PanelCard(modifier = modifier) {
        // `fillMaxSize()` prima del padding: senza, questa Column si
        // limiterebbe al contenuto (wrap-content) e il riquadro
        // dell'anteprima sottostante, che usa `weight(1f)` per prendere lo
        // spazio verticale restante, non avrebbe nessuno spazio da cui
        // prenderlo.
        Column(modifier = Modifier.fillMaxSize().padding(16.dp)) {
            Text(title, style = MaterialTheme.typography.subtitle2, color = TextPrimary, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(10.dp))
            Button(
                onClick = onImportClick,
                shape = PillShape,
                colors = ButtonDefaults.buttonColors(backgroundColor = AccentBlue),
            ) {
                Text(importLabel, style = MaterialTheme.typography.caption)
            }
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
            }
            Box(
                modifier = Modifier.weight(1f).fillMaxWidth().padding(top = 10.dp)
                    .background(PanelBackground, InnerShape)
                    .border(1.dp, PanelDivider, InnerShape),
                contentAlignment = Alignment.Center,
            ) {
                val bitmap = overrideBitmap ?: photo?.bitmap
                if (bitmap != null) {
                    ZoomableImage(
                        bitmap = bitmap,
                        contentDescription = title,
                        resetKey = photo?.fileName,
                        modifier = Modifier.fillMaxSize().clip(InnerShape),
                    )
                } else if (photo != null) {
                    Text("(anteprima non decodificabile, ma il motore ha letto i metadati)", style = MaterialTheme.typography.caption, color = TextMuted)
                } else {
                    Text("Nessuna foto importata", style = MaterialTheme.typography.caption, color = TextMuted)
                }
            }
            photo?.let { s ->
                Spacer(Modifier.height(6.dp))
                Text(s.fileName, style = MaterialTheme.typography.caption, color = TextPrimary)
                s.cameraLabel?.let { Text(it, style = MaterialTheme.typography.caption, color = TextMuted) }
            }
            actions()
        }
    }
}

/** Zoom minimo (sempre 1x: la dimensione "adatta al riquadro" di
 * `ContentScale.Fit`, mai più piccola) e massimo consentito con la rotella
 * del mouse — oltre 6x un'anteprima già ridotta per l'editing interattivo
 * (vedi `INTERACTIVE_PREVIEW_MAX_DIM` lato Rust) mostrerebbe solo pixel
 * ingranditi senza dettaglio utile in più. */
private const val MIN_ZOOM = 1f
private const val MAX_ZOOM = 6f

/** Quanto una singola "tacca" di rotella cambia lo zoom: valore scelto per
 * essere reattivo senza far scattare lo zoom in modo brusco. */
private const val ZOOM_SENSITIVITY = 0.08f

/**
 * Un `Image` che si ingrandisce/rimpicciolisce con la rotella del mouse
 * quando il cursore è sopra di esso (lo scroll è consegnato solo al
 * componente sotto il puntatore, non serve altro per limitarlo "quando si
 * passa sopra la foto col cursore"). Zoom centrato (nessun pan): scorrere in
 * avanti (lontano da sé) ingrandisce, scorrere all'indietro rimpicciolisce,
 * fino a tornare esattamente alla dimensione "adatta al riquadro" — mai più
 * piccola. `resetKey` azzera lo zoom quando cambia (es. una foto diversa
 * importata nello stesso riquadro): NON va legato al bitmap stesso, che
 * cambia ad ogni singolo fotogramma durante il rendering dal vivo mentre si
 * trascina uno slider — altrimenti lo zoom si azzererebbe continuamente
 * durante l'editing invece di restare stabile.
 */
@Composable
private fun ZoomableImage(
    bitmap: ImageBitmap,
    contentDescription: String,
    resetKey: Any?,
    modifier: Modifier = Modifier,
) {
    var zoom by remember(resetKey) { mutableStateOf(1f) }
    Box(
        modifier = modifier
            .clipToBounds()
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent()
                        if (event.type == PointerEventType.Scroll) {
                            val scrollDelta = event.changes.firstOrNull()?.scrollDelta?.y ?: 0f
                            if (scrollDelta != 0f) {
                                zoom = (zoom * (1f - scrollDelta * ZOOM_SENSITIVITY)).coerceIn(MIN_ZOOM, MAX_ZOOM)
                                event.changes.forEach { it.consume() }
                            }
                        }
                    }
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Image(
            bitmap = bitmap,
            contentDescription = contentDescription,
            contentScale = ContentScale.Fit,
            modifier = Modifier.fillMaxSize().graphicsLayer(scaleX = zoom, scaleY = zoom),
        )
    }
}

/** Soglia (frazione di pixel, 0f..1f) oltre la quale "slider sicuri" segnala
 * clipping su uno slider — scelta arbitraria ma dichiarata: il 2% dei pixel
 * dell'anteprima è già percepibile come luci bruciate/ombre schiacciate
 * evidenti, non un singolo pixel isolato che non vale la pena segnalare. */
private const val CLIP_WARNING_THRESHOLD = 0.02f
private val ClipWarningColor = Color(0xFFFFB300)

@Composable
private fun DevelopPanel(
    modifier: Modifier,
    look: EditableLook,
    onEdit: ((EditableLook) -> EditableLook) -> Unit,
    onReset: () -> Unit,
    editIntensity: Float,
    onEditIntensityChange: (Float) -> Unit,
    shadowClipFraction: Float? = null,
    highlightClipFraction: Float? = null,
    // Angoli arrotondati solo a sinistra di default: il caso d'uso storico
    // (finestra larga) tiene questo pannello incollato al bordo destro dello
    // schermo. Nel layout stretto per telefono (vedi `App`) il pannello
    // diventa invece un blocco a piena larghezza in fondo alla pagina, e il
    // chiamante passa una forma diversa (arrotondata solo in alto) — da qui
    // il parametro invece di una forma fissa.
    shape: androidx.compose.ui.graphics.Shape = RoundedCornerShape(topStart = 14.dp, bottomStart = 14.dp),
    // "Rileva soggetto" (sezione Maschera più sotto): tutti opzionali con
    // default `null`/`false` in modo che nessuna delle chiamate esistenti a
    // `DevelopPanel` debba cambiare — il pulsante/anteprima compaiono solo
    // dove il chiamante passa `onDetectSubject` (le due schermate con una
    // foto target aperta), non nelle altre.
    onDetectSubject: (() -> Unit)? = null,
    saliencyBitmap: androidx.compose.ui.graphics.ImageBitmap? = null,
    saliencyBusy: Boolean = false,
    saliencyError: String? = null,
) {
    // "Slider sicuri" (idea approvata, vedi README.md): solo un avviso sul
    // valore CORRENTE — quanto di QUESTO rendering sta bruciando le luci o
    // schiacciando le ombre — non una previsione per l'intero range dello
    // slider (richiederebbe ri-renderizzare per ogni posizione possibile).
    val highlightsClipping = (highlightClipFraction ?: 0f) > CLIP_WARNING_THRESHOLD
    val shadowsClipping = (shadowClipFraction ?: 0f) > CLIP_WARNING_THRESHOLD
    Column(
        modifier = modifier.background(PanelSurface, shape).verticalScroll(rememberScrollState()).padding(20.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Develop", style = MaterialTheme.typography.h6, fontWeight = FontWeight.Bold, color = TextPrimary)
            TextButton(onClick = onReset, shape = PillShape) { Text("Reimposta", style = MaterialTheme.typography.caption) }
        }
        Spacer(Modifier.height(4.dp))
        Box(modifier = Modifier.fillMaxWidth().height(2.dp).background(AccentGradient, shape = RoundedCornerShape(1.dp)))
        Spacer(Modifier.height(12.dp))

        // Dial "Intensità edit": scala l'INTERO editing verso lo zero (o lo
        // esagera oltre il 100%) senza dover toccare ogni singolo slider a
        // mano — vedi `EditableLook.scaledBy`. Fuori da `DevelopSection`
        // apposta, per restare visibile in cima e non confondersi con le
        // singole categorie di regolazione sottostanti.
        Text(
            "Intensità edit: ${(editIntensity * 100).roundToInt()}%",
            style = MaterialTheme.typography.caption,
            color = TextPrimary,
        )
        Slider(
            value = editIntensity,
            onValueChange = onEditIntensityChange,
            valueRange = 0f..1.5f,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        Divider(color = PanelDivider, thickness = 1.dp)
        Spacer(Modifier.height(12.dp))

        DevelopSection("Base") {
            FloatSlider(
                "Esposizione (EV)", look.exposureEv, -5f..5f,
                warning = highlightsClipping || shadowsClipping,
                onChange = { onEdit { l -> l.copy(exposureEv = it) } },
            ) { "%.2f".format(it) }
            IntSlider("Contrasto", look.contrast, -100..100) { onEdit { l -> l.copy(contrast = it) } }
            IntSlider("Alte luci", look.highlights, -100..100, warning = highlightsClipping) { onEdit { l -> l.copy(highlights = it) } }
            IntSlider("Ombre", look.shadows, -100..100, warning = shadowsClipping) { onEdit { l -> l.copy(shadows = it) } }
            IntSlider("Bianchi", look.whites, -100..100, warning = highlightsClipping) { onEdit { l -> l.copy(whites = it) } }
            IntSlider("Neri", look.blacks, -100..100, warning = shadowsClipping) { onEdit { l -> l.copy(blacks = it) } }
            if (highlightsClipping || shadowsClipping) {
                Spacer(Modifier.height(4.dp))
                Text(
                    listOfNotNull(
                        "luci bruciate".takeIf { highlightsClipping },
                        "ombre schiacciate".takeIf { shadowsClipping },
                    ).joinToString(prefix = "Attenzione: ", separator = " e "),
                    style = MaterialTheme.typography.caption,
                    color = ClipWarningColor,
                )
            }
        }

        DevelopSection("Colore") {
            IntSlider("Temperatura (K)", look.whiteBalanceTemp, 2000..12000) { onEdit { l -> l.copy(whiteBalanceTemp = it) } }
            IntSlider("Tinta", look.whiteBalanceTint, -100..100) { onEdit { l -> l.copy(whiteBalanceTint = it) } }
            IntSlider("Vivacità", look.vibrance, -100..100) { onEdit { l -> l.copy(vibrance = it) } }
            IntSlider("Saturazione", look.saturation, -100..100) { onEdit { l -> l.copy(saturation = it) } }
        }

        DevelopSection("Bilanciamento del bianco a gradiente") {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Switch(
                    checked = look.wbGradientEnabled,
                    onCheckedChange = { onEdit { l -> l.copy(wbGradientEnabled = it) } },
                    colors = SwitchDefaults.colors(checkedThumbColor = AccentBlue, checkedTrackColor = AccentBlue),
                )
                Text(
                    if (look.wbGradientEnabled) "Attivo — due zone di WB sfumate" else "Disattivo (un solo WB, sopra)",
                    style = MaterialTheme.typography.caption,
                    color = TextPrimary,
                )
            }
            if (look.wbGradientEnabled) {
                Spacer(Modifier.height(8.dp))
                Text("Asse della transizione", style = MaterialTheme.typography.caption, color = TextMuted)
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    listOf(true to "Verticale (alto → basso)", false to "Orizzontale (sinistra → destra)").forEach { (vertical, label) ->
                        val selected = look.wbGradientVertical == vertical
                        TextButton(
                            onClick = { onEdit { l -> l.copy(wbGradientVertical = vertical) } },
                            shape = PillShape,
                            colors = ButtonDefaults.textButtonColors(
                                backgroundColor = if (selected) PanelSurfaceRaised else Color.Transparent,
                            ),
                        ) {
                            Text(
                                label,
                                style = MaterialTheme.typography.caption,
                                color = if (selected) AccentBlue else TextMuted,
                                fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
                            )
                        }
                    }
                }
                IntSlider("Posizione transizione", look.wbGradientPosition, 0..100) { onEdit { l -> l.copy(wbGradientPosition = it) } }
                IntSlider("Ampiezza transizione", look.wbGradientSpread, 0..100) { onEdit { l -> l.copy(wbGradientSpread = it) } }
                Spacer(Modifier.height(4.dp))
                Text("Zona B (l'altra estremità del gradiente)", style = MaterialTheme.typography.caption, color = TextMuted, fontWeight = FontWeight.Bold)
                IntSlider("Temperatura zona B (K)", look.whiteBalanceBTemp, 2000..12000) { onEdit { l -> l.copy(whiteBalanceBTemp = it) } }
                IntSlider("Tinta zona B", look.whiteBalanceBTint, -100..100) { onEdit { l -> l.copy(whiteBalanceBTint = it) } }
            }
        }

        DevelopSection("Curva tonale") {
            ToneCurveEditor(look.toneCurve) { updated -> onEdit { l -> l.copy(toneCurve = updated) } }
        }

        DevelopSection("HSL per banda colore") {
            HslPanel(look, onEdit)
        }

        DevelopSection("Dettaglio (Texture)") {
            IntSlider("Fine", look.textureFine, -100..100) { onEdit { l -> l.copy(textureFine = it) } }
            IntSlider("Media", look.textureMedium, -100..100) { onEdit { l -> l.copy(textureMedium = it) } }
            IntSlider("Grossa", look.textureCoarse, -100..100) { onEdit { l -> l.copy(textureCoarse = it) } }
        }

        DevelopSection("Riduzione del rumore") {
            IntSlider("Luminanza", look.noiseReductionLuma, 0..100) { onEdit { l -> l.copy(noiseReductionLuma = it) } }
            IntSlider("Colore", look.noiseReductionColor, 0..100) { onEdit { l -> l.copy(noiseReductionColor = it) } }
        }

        DevelopSection("Maschera Soggetto/Sfondo") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Attiva maschera", style = MaterialTheme.typography.caption, color = TextPrimary)
                Switch(
                    checked = look.subjectMaskEnabled,
                    onCheckedChange = { onEdit { l -> l.copy(subjectMaskEnabled = it) } },
                )
            }
            Spacer(Modifier.height(6.dp))

            if (onDetectSubject != null) {
                // "Rileva soggetto": calcola/mostra la mappa di salienza —
                // NON attiva da sola la maschera (l'utente decide comunque
                // con lo `Switch` sopra), è solo l'anteprima ispezionabile
                // di dove il motore "guarderebbe" (vedi `engine/README.md`
                // per i limiti onesti dell'euristica: un'analisi globale per
                // colore + centratura, non una segmentazione vera).
                TextButton(
                    onClick = onDetectSubject,
                    shape = PillShape,
                    colors = ButtonDefaults.textButtonColors(backgroundColor = PanelSurfaceRaised),
                    enabled = !saliencyBusy,
                ) {
                    Text(
                        if (saliencyBusy) "Rilevamento…" else "Rileva soggetto",
                        style = MaterialTheme.typography.caption,
                    )
                }
                saliencyError?.let {
                    Spacer(Modifier.height(4.dp))
                    Text("Errore: $it", color = MaterialTheme.colors.error, style = MaterialTheme.typography.caption)
                }
                saliencyBitmap?.let { bitmap ->
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Mappa di salienza (bianco = alta, nero = bassa)",
                        style = MaterialTheme.typography.caption,
                        color = TextMuted,
                    )
                    Spacer(Modifier.height(4.dp))
                    Image(
                        bitmap = bitmap,
                        contentDescription = "Mappa di salienza del soggetto",
                        contentScale = ContentScale.Fit,
                        modifier = Modifier.fillMaxWidth().height(160.dp).clip(InnerShape),
                    )
                }
                Spacer(Modifier.height(10.dp))
            }

            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                listOf(
                    MaskTarget.SUBJECT to "Soggetto",
                    MaskTarget.BACKGROUND to "Sfondo",
                ).forEach { (candidate, label) ->
                    val selected = look.subjectMaskTarget == candidate
                    TextButton(
                        onClick = { onEdit { l -> l.copy(subjectMaskTarget = candidate) } },
                        shape = PillShape,
                        colors = ButtonDefaults.textButtonColors(
                            backgroundColor = if (selected) PanelSurfaceRaised else Color.Transparent,
                        ),
                    ) {
                        Text(
                            label,
                            style = MaterialTheme.typography.caption,
                            color = if (selected) AccentBlue else TextMuted,
                            fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
                        )
                    }
                }
            }
            Spacer(Modifier.height(4.dp))
            FloatSlider(
                "Esposizione maschera (EV)", look.subjectMaskExposureEv, -2f..2f,
                onChange = { onEdit { l -> l.copy(subjectMaskExposureEv = it) } },
            ) { "%.2f".format(it) }
            IntSlider("Contrasto maschera", look.subjectMaskContrast, -100..100) {
                onEdit { l -> l.copy(subjectMaskContrast = it) }
            }
            IntSlider("Saturazione maschera", look.subjectMaskSaturation, -100..100) {
                onEdit { l -> l.copy(subjectMaskSaturation = it) }
            }
        }

        DevelopSection("Viraggio (Split Toning)") {
            IntSlider("Tinta ombre (°)", look.shadowHue, 0..360) { onEdit { l -> l.copy(shadowHue = it) } }
            IntSlider("Saturazione ombre", look.shadowSat, 0..100) { onEdit { l -> l.copy(shadowSat = it) } }
            IntSlider("Tinta luci (°)", look.highlightHue, 0..360) { onEdit { l -> l.copy(highlightHue = it) } }
            IntSlider("Saturazione luci", look.highlightSat, 0..100) { onEdit { l -> l.copy(highlightSat = it) } }
            IntSlider("Bilanciamento", look.splitToningBalance, -100..100) { onEdit { l -> l.copy(splitToningBalance = it) } }
        }
    }
}

/** Editor grafico semplificato della tone curve, in stile "point curve" di
 * Lightroom: 5 punti di controllo a X FISSA (0/64/128/192/255 — ombre,
 * scure, medi, chiare, luci), ognuno trascinabile solo in verticale. Tocca/
 * trascina in un punto qualsiasi del grafico: sposta il punto di controllo
 * più vicino in X alla posizione del dito, non serve centrare esattamente
 * l'handle. `rememberUpdatedState` tiene la callback e i punti sempre
 * aggiornati SENZA riavviare il rilevatore di trascinamento ad ogni singolo
 * fotogramma (altrimenti ogni tick interromperebbe il trascinamento in corso
 * invece di continuarlo). */
@Composable
private fun ToneCurveEditor(
    points: List<TonePoint>,
    onChange: (List<TonePoint>) -> Unit,
) {
    val latestPoints = rememberUpdatedState(points)
    val latestOnChange = rememberUpdatedState(onChange)
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Canvas(
            modifier = Modifier
                .fillMaxWidth()
                .height(160.dp)
                .background(PanelBackground, InnerShape)
                .border(1.dp, PanelDivider, InnerShape)
                .pointerInput(Unit) {
                    detectDragGestures { change, _ ->
                        change.consume()
                        val w = size.width.toFloat()
                        val h = size.height.toFloat()
                        if (w <= 0f || h <= 0f) return@detectDragGestures
                        val px = change.position.x.coerceIn(0f, w)
                        val py = change.position.y.coerceIn(0f, h)
                        val pointerXValue = px / w * 255f
                        val current = latestPoints.value
                        val nearestIndex = current.indices.minByOrNull { i -> abs(current[i].x - pointerXValue) }
                            ?: return@detectDragGestures
                        val newY = (255f - py / h * 255f).roundToInt().coerceIn(0, 255)
                        val updated = current.mapIndexed { i, p -> if (i == nearestIndex) p.copy(y = newY) else p }
                        latestOnChange.value(updated)
                    }
                },
        ) {
            val w = size.width
            val h = size.height
            // Diagonale di riferimento (curva identità, nessuna correzione).
            drawLine(color = PanelDivider, start = Offset(0f, h), end = Offset(w, 0f), strokeWidth = 1f)
            val path = Path()
            points.forEachIndexed { i, p ->
                val x = p.x / 255f * w
                val y = h - p.y / 255f * h
                if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
            }
            drawPath(path, color = AccentBlue, style = Stroke(width = 3f))
            points.forEach { p ->
                val x = p.x / 255f * w
                val y = h - p.y / 255f * h
                drawCircle(color = AccentBlue, radius = 6f, center = Offset(x, y))
            }
        }
    }
}

/** Ordine e nomi delle 8 bande di tonalità, identici a quelli usati dal
 * motore in estrazione/rendering (`core_types::HslAdjustments`, commento
 * "Ordine bande: Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta"). */
private val HslBandNames = listOf("Rosso", "Arancio", "Giallo", "Verde", "Acqua", "Blu", "Viola", "Magenta")

// Colori puramente decorativi per i pallini accanto a ogni slider — aiutano
// a riconoscere a colpo d'occhio la banda, come le etichette colorate del
// pannello HSL vero di Lightroom. Non hanno alcun legame con i gradi esatti
// usati dal motore per definire i confini di banda.
private val HslBandColors = listOf(
    Color(0xFFE5484D), // Rosso
    Color(0xFFF2994A), // Arancio
    Color(0xFFF2C94C), // Giallo
    Color(0xFF6FCF97), // Verde
    Color(0xFF56CCF2), // Acqua
    Color(0xFF4A90E2), // Blu
    Color(0xFF9B6BE0), // Viola
    Color(0xFFE0679B), // Magenta
)

private enum class HslChannel { HUE, SAT, LUM }

/** Pannello HSL per banda colore, in stile Lightroom: tre "tab" (Tonalità/
 * Saturazione/Luminanza, non tutte e tre insieme) e sotto uno slider per
 * ciascuna delle 8 bande del canale selezionato — evita di mostrare 24
 * slider insieme. */
@Composable
private fun HslPanel(
    look: EditableLook,
    onEdit: ((EditableLook) -> EditableLook) -> Unit,
) {
    var channel by remember { mutableStateOf(HslChannel.HUE) }
    Column(modifier = Modifier.fillMaxWidth()) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            listOf(
                HslChannel.HUE to "Tonalità",
                HslChannel.SAT to "Saturazione",
                HslChannel.LUM to "Luminanza",
            ).forEach { (candidate, label) ->
                val selected = channel == candidate
                TextButton(
                    onClick = { channel = candidate },
                    shape = PillShape,
                    colors = ButtonDefaults.textButtonColors(
                        backgroundColor = if (selected) PanelSurfaceRaised else Color.Transparent,
                    ),
                ) {
                    Text(
                        label,
                        style = MaterialTheme.typography.caption,
                        color = if (selected) AccentBlue else TextMuted,
                        fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
                    )
                }
            }
        }
        Spacer(Modifier.height(4.dp))
        HslBandNames.forEachIndexed { i, name ->
            val currentValue = when (channel) {
                HslChannel.HUE -> look.hslHue[i]
                HslChannel.SAT -> look.hslSat[i]
                HslChannel.LUM -> look.hslLum[i]
            }
            IntSlider(name, currentValue, -100..100, swatchColor = HslBandColors[i]) { newValue ->
                onEdit { l ->
                    when (channel) {
                        HslChannel.HUE -> l.copy(hslHue = l.hslHue.mapIndexed { idx, v -> if (idx == i) newValue else v })
                        HslChannel.SAT -> l.copy(hslSat = l.hslSat.mapIndexed { idx, v -> if (idx == i) newValue else v })
                        HslChannel.LUM -> l.copy(hslLum = l.hslLum.mapIndexed { idx, v -> if (idx == i) newValue else v })
                    }
                }
            }
        }
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
 * consegue è gestito centralmente dal `LaunchedEffect` in `RawForgeApp`.
 * `warning = true` ("slider sicuri") colora il badge del valore e lo slider
 * stesso in ambra: segnala che il valore ATTUALE di QUESTO slider corrisponde
 * a un rendering con luci bruciate/ombre schiacciate oltre soglia — non una
 * previsione sull'intero range possibile dello slider. */
@Composable
private fun IntSlider(
    label: String,
    value: Int,
    range: IntRange,
    swatchColor: Color? = null,
    warning: Boolean = false,
    onChange: (Int) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                swatchColor?.let { Box(modifier = Modifier.size(8.dp).clip(CircleShape).background(it)) }
                Text(label, style = MaterialTheme.typography.caption, color = TextPrimary)
            }
            Text(
                value.toString(),
                style = MaterialTheme.typography.caption,
                color = if (warning) ClipWarningColor else TextMuted,
                modifier = Modifier
                    .clip(RoundedCornerShape(6.dp))
                    .background(PanelSurfaceRaised)
                    .padding(horizontal = 6.dp, vertical = 1.dp),
            )
        }
        Slider(
            value = value.toFloat(),
            onValueChange = { onChange(it.roundToInt()) },
            valueRange = range.first.toFloat()..range.last.toFloat(),
            colors = if (warning) {
                SliderDefaults.colors(thumbColor = ClipWarningColor, activeTrackColor = ClipWarningColor)
            } else {
                SliderDefaults.colors()
            },
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun FloatSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    warning: Boolean = false,
    onChange: (Float) -> Unit,
    format: (Float) -> String,
) {
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.caption, color = TextPrimary)
            Text(format(value), style = MaterialTheme.typography.caption, color = if (warning) ClipWarningColor else TextMuted)
        }
        Slider(
            value = value,
            onValueChange = onChange,
            valueRange = range,
            colors = if (warning) {
                SliderDefaults.colors(thumbColor = ClipWarningColor, activeTrackColor = ClipWarningColor)
            } else {
                SliderDefaults.colors()
            },
            modifier = Modifier.fillMaxWidth(),
        )
    }
}
