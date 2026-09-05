# RawForge

Scaffold di partenza per RawForge, l'alternativa ultra-veloce a Lightroom (Windows + Android),
progettata secondo l'architettura descritta in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Cosa contiene questo repository

- **`shared/`, `androidApp/`, `desktopApp/`** — un'app Kotlin Multiplatform + Compose
  Multiplatform reale e completa (stessa UI su Android e Windows). La UI ha tre parti:
  1. la demo minimale originale ("Stato motore" / "Genera preset XMP di esempio"), già verificata
     end-to-end in CI su entrambe le piattaforme;
  2. **importa la foto campione**: un pulsante apre il selettore di file nativo di ciascuna
     piattaforma (finestra di dialogo AWT su Windows, Storage Access Framework su Android),
     decodifica davvero il file scelto tramite il motore Rust — anche un file **RAW vero** di una
     fotocamera, non solo JPEG/PNG — ne mostra l'anteprima e permette di copiarne le impostazioni
     come preset Lightroom `.xmp`;
  3. **apri la foto da modificare e incolla le impostazioni**: un secondo import per la foto
     target, uno slider "intensità adattamento" e un pulsante che applica le impostazioni copiate
     dal campione — non identiche, ma **adattate in modo intelligente** alla scena specifica del
     target (Smart-Batch Contestuale, §4.2: esposizione/luci/ombre calcolate dai descrittori di
     scena di entrambe le foto, con i guardrail dell'architettura) — e mostra subito l'anteprima
     renderizzata **dentro l'app**, senza dover passare da un file `.xmp` esterno.
- **`engine/`** — il workspace Rust del motore di elaborazione immagini: `core-types`,
  `color-science`, `harmonic` (Sintesi Armonica), `smartbatch` (Smart-Batch Contestuale),
  `metadata` (sidecar non distruttivo), `xmp` (export preset Lightroom), `gpu-pipe` (shader
  WGSL validati con `naga`), `raw-decode` (decodifica RAW vera), **`look-render`** (applica un
  Look ai pixel su CPU — bilanciamento del bianco anche a gradiente, esposizione, tone curve,
  contrasto, highlights/shadows, HSL per banda, split toning, texture a bande di frequenza) e
  **`ffi`** (la superficie UniFFI che collega tutto quanto sopra a Kotlin, incluso l'oggetto
  stateful `PhotoEditSession` per il rendering dal vivo, vedi sotto). 103 test, tutti verdi,
  eseguiti in locale prima di ogni consegna. Dettagli in
  [`engine/README.md`](engine/README.md).
- **`.github/workflows/build.yml`** — la pipeline di build automatica, in 5 fasi:
  1. `rust-tests` — compila e testa l'intero workspace Rust.
  2. `generate-bindings` — compila il crate `ffi` per l'host e genera i binding Kotlin
     (`uniffi-bindgen`), pubblicandoli come artifact condiviso dagli altri job.
  3. `android` — cross-compila il motore Rust per Android (arm64-v8a, armeabi-v7a, x86_64)
     con `cargo-ndk`, scarica i binding generati, compila l'APK.
  4. `windows` — compila il motore Rust nativamente per Windows (`rawforge_ffi.dll`), lo
     colloca dove JNA lo trova a runtime, scarica i binding, compila l'installer `.exe`.
  5. `release` — pubblica i due file nella pagina "Releases" del repository.

## Decodifica RAW vera: `rawler` invece di LibRaw

L'architettura originale (§1.1) prevedeva LibRaw (C++) come motore di decodifica RAW principale.
In questo incremento ho usato invece **`rawler`** (crate Rust puro, licenza LGPL-2.1, dallo stesso
progetto `dnglab`), che l'architettura stessa citava come piano B proprio per il caso Android. La
ragione: essendo scritto interamente in Rust, `rawler` cross-compila per Android con `cargo-ndk`
esattamente come ogni altro crate di questo workspace — **nessun toolchain NDK C++/CMake da
configurare per la libreria di decodifica**, che era il blocco tecnico più difficile rimasto
aperto dopo la prima consegna. Il crate `raw-decode` estrae l'anteprima incorporata dalla
fotocamera stessa (JPEG "embedded", livello di cache L0 dell'architettura, §2.1) e i metadati
base (marca/modello); il demosaic completo a piena risoluzione (§3.2) resta il prossimo
incremento, una volta verde questo giro su CI.

**Nota legale** (non ancora una valutazione legale vera e propria): `rawler` è LGPL-2.1 come
LibRaw — lo stesso rischio di conformità già segnalato in `docs/ARCHITECTURE.md` §9 per una
distribuzione commerciale, specie su Android, si applica anche qui.

## "Incolla impostazioni": rendering CPU + Smart-Batch collegato alla UI

Prima di questo incremento lo Smart-Batch Contestuale (`smartbatch`, §4.2) esisteva già nel motore
ma non era raggiungibile dalla UI, e l'unico modo di "usare" un Look estratto era esportarlo come
`.xmp` e aprirlo in un altro programma. Ora c'è un nuovo crate, **`look-render`**, che applica un
`HarmonicLook` ai pixel di un'immagine — esposizione, tone curve, contrasto, highlights/shadows,
HSL per banda, split toning, vibrance/saturazione — direttamente su CPU (`rayon`, nessuna GPU
necessaria). La funzione UniFFI `paste_look_onto_target_photo` mette in fila: estrazione del Look
dalla foto campione, calcolo dei descrittori di scena di campione e target, calcolo dei delta
adattivi con `smartbatch` (già scritto e testato), applicazione del Look adattato, rendering
dell'anteprima — e la UI mostra subito il risultato, senza passare da un file esterno.

**Perché CPU e non la pipeline GPU (`gpu-pipe`, già scritta con `wgpu`/WGSL)**: collegare `wgpu` a
Kotlin via UniFFI/JNA su entrambe le piattaforme (gestione di un device GPU reale, superfici
condivise, ecc.) è un lavoro sostanzialmente più grande — rimandato di proposito. La pipeline CPU è
più lenta su immagini a piena risoluzione ma sufficiente per un'anteprima, ed è l'unica via
testabile in questo ambiente di sviluppo (nessuna GPU disponibile qui). Dettagli e semplificazioni
dichiarate nel commento di testa di `look-render/src/lib.rs`.

**Correzione di fedeltà**: `HarmonicLookFfi` (il tipo che attraversa il confine Rust↔Kotlin)
portava originariamente solo 9 dei ~18 campi di un `HarmonicLook` — una scelta della primissima
demo. Questo significava che highlights, shadows, whites, blacks, saturation, tone curve, HSL,
bilanciamento del "balance" di split-toning e il tint del bilanciamento del bianco venivano
silenziosamente azzerati ad ogni giro Kotlin→Rust→Kotlin, **compreso l'export `.xmp` già esistente**
(bug pre-esistente, non introdotto ora — solo scoperto e corretto costruendo questa funzionalità).
Ora `HarmonicLookFfi` porta tutti i campi; un test dedicato (`harmonic_look_ffi_round_trip_preserves_all_fields`)
verifica che il giro di andata e ritorno non perda più nulla.

## Nuova UI: tema scuro, foto affiancate, editing manuale, esportazione

Su richiesta esplicita, la UI (identica su Android e Windows, `shared/src/commonMain/kotlin/com/rawforge/shared/App.kt`)
è stata riscritta quasi per intero, in stile "modulo Develop" di un software di editing
fotografico professionale:

- **Tema scuro**: pannelli grigio molto scuro, testo quasi bianco, un solo accento blu per i
  controlli — non più il Material chiaro di default.
- **Foto campione e foto target affiancate** in una riga (non più impilate una sopra l'altra):
  si vedono entrambe senza dover scorrere, e si ridimensionano insieme alla finestra.
- **Pannello "Develop" a destra**, con slider veri per esposizione, contrasto, luci/ombre,
  bianchi/neri, bilanciamento del bianco, vibrance/saturazione e viraggio (split toning) — legati
  in tempo reale al motore Rust, ora anche **durante il trascinamento**, non solo al rilascio (vedi
  la sezione successiva). Dopo "Incolla impostazioni", il pannello parte già dai valori decisi da
  Smart-Batch, e l'utente può correggerli a mano da lì.
- **Pulsante "Esporta foto…"**: renderizza a piena risoluzione (non più la copia ridotta usata per
  l'editing interattivo) e salva su un file scelto dall'utente — finestra di salvataggio nativa su
  Windows, selettore di destinazione di sistema su Android (nessun permesso runtime richiesto). Il
  pulsante mostra "Esportazione…" ed è disabilitato mentre il rendering finale è in corso.

**Novità di questo giro, su richiesta esplicita — "rendi l'interfaccia più simile a Lightroom"**:

- **Modifica a schermo intero**: un nuovo pulsante ("Modifica a schermo intero", accanto a
  "Esporta foto…") passa a una modalità in stile modulo Develop di Lightroom — niente più
  confronto affiancato con la foto campione, solo la foto target grande al centro con il pannello
  "Develop" accanto, e un pulsante "← Torna al confronto" per uscirne. Disponibile non solo dopo
  "Incolla impostazioni", ma ogni volta che una foto è aperta per l'editing.
- **Editor grafico della tone curve**: un grafico trascinabile con 5 punti di controllo (ombre,
  scure, medi, chiare, luci, come il "point curve" semplificato di Lightroom) — trascina in un
  punto qualsiasi del grafico per spostare in verticale il punto di controllo più vicino.
- **Pannello HSL per banda colore**: 8 bande (Rosso/Arancio/Giallo/Verde/Acqua/Blu/Viola/Magenta,
  stesso ordine usato dal motore), con tre "tab" Tonalità/Saturazione/Luminanza come in Lightroom,
  invece di 24 slider tutti insieme.

Con questo, la fase 1 del piano concordato (tema, layout, export, editing manuale) è completa: il
motore calcolava già tone curve e HSL per banda dalla Sintesi Armonica, mancava solo poterli
correggere a mano da qui.

**Passata di rifinitura visiva**, su richiesta esplicita prima di questa consegna ("rendi
l'interfaccia più accattivante, un look più moderno"): i pannelli piatti a sfondo colorato sono
diventati `Card` vere con un'ombra leggera; gli angoli sono più ampi e morbidi ovunque; un filo
sottile con un gradiente blu→viola (l'unico accento di colore "vivo" dell'app) segna la barra in
alto e l'intestazione del pannello Develop, accanto a un piccolo marchio quadrato con lo stesso
gradiente; i pulsanti principali sono diventati "pill" arrotondate; il riquadro dell'anteprima
foto e il grafico della tone curve hanno ora un bordo sottile invece di un semplice sfondo scuro;
i valori numerici degli slider sono in un piccolo badge arrotondato invece di testo nudo; e ogni
slider HSL ha accanto un pallino colorato che richiama la banda a cui appartiene (Rosso/Arancio/
Giallo/Verde/Acqua/Blu/Viola/Magenta), come le etichette colorate del pannello HSL vero di
Lightroom. Nessuna nuova dipendenza Gradle (niente libreria di icone: i "pulsanti pill" e il
marchio sono forme disegnate, non icone).

**Cosa NON c'è ancora** (dichiarato, non nascosto, prossime fasi del piano concordato): le maschere
locali (pennello/gradiente/radiale) copiabili nei preset, la libreria/catalogo delle foto già
modificate, e il batch reale su tante foto insieme.

## Nuovo: rendering dal vivo mentre si trascina uno slider (`PhotoEditSession`)

Richiesta esplicita dell'utente dopo un uso reale: "è veramente difficile da utilizzare" — il
rendering avveniva solo al rilascio dello slider, e ogni chiamata ripartiva da zero: ri-decodificava
l'intera foto target dai bytes grezzi (RAW compreso) e renderizzava a piena risoluzione originale,
ritrasmettendo l'intera foto attraverso il confine Kotlin/nativo ogni volta.

Corretto sostituendo le vecchie funzioni "usa e getta" con un nuovo oggetto nativo **stateful**,
`PhotoEditSession`: quando si apre (o si cambia) la foto da modificare, il motore la decodifica
UNA SOLA volta e la mantiene cacheiata in memoria in due copie — una a piena risoluzione (per
l'export finale) e una ridotta apposta per l'editing interattivo (lato più lungo max 1024px). Ogni
movimento di uno slider aggiorna solo lo stato leggero del Look e chiama il rendering veloce sulla
copia ridotta già cacheiata: niente ri-decodifica, niente pixel a piena risoluzione, niente
ri-trasmissione della foto ad ogni tick. Lato Kotlin, un `LaunchedEffect` osserva lo stato dello
slider e richiama il rendering in background (`Dispatchers.Default`, mai sul thread della UI),
scartando automaticamente il risultato di un rendering ormai superato non appena arriva una
modifica più recente (`collectLatest`) — così il rendering insegue sempre l'ultima posizione dello
slider mentre si trascina, invece di accodarsi in ritardo dietro ogni tick. Dettagli tecnici
completi (inclusa la nuova firma UniFFI, `#[derive(uniffi::Object)]`) in
[`engine/README.md`](engine/README.md).

## Nuovo: copia dello stile più fedele (bilanciamento del bianco + HSL per banda)

Insieme alla velocità, richiesto anche di migliorare quanto bene "incolla impostazioni" riproduce
lo stile della foto campione. Due leve del motore che esistevano solo a metà sono state completate:

- **Bilanciamento del bianco** (temperatura/tinta): prima dichiarato esplicitamente come non
  applicato nel rendering. Ora è reso come guadagno per canale in spazio lineare — un'approssimazione
  dichiarata da color grading, non un vero profilo colore camera, ma sufficiente a far corrispondere
  meglio la temperatura quando si copia lo stile da una foto di riferimento più calda o più fredda.
- **HSL per singola banda di colore**: la Sintesi Armonica calcolava già tone curve, contrasto,
  esposizione e split toning dalla foto campione, ma le regolazioni HSL (hue/saturazione/luminanza
  per 8 bande di tonalità, già supportate dal renderer) restavano sempre a zero — semplicemente non
  venivano mai estratte. Ora vengono calcolate davvero dalla foto campione, con le stesse
  precauzioni già applicate correggendo il bug storico dell'esposizione: sempre relative alla scena
  stessa (mai un pivot assoluto fisso), e con una soglia minima di pixel per banda per non produrre
  regolazioni rumorose su colori scarsamente rappresentati nella foto campione.

Dettagli tecnici e formule complete in [`engine/README.md`](engine/README.md).

**Onestà sulla verifica**: questo è, per distacco, il cambiamento Kotlin più esteso consegnato in
un colpo solo finora — quasi tutto `App.kt`, più due file nuovi (`FileSaverLauncher.kt` e le sue
implementazioni Android/Desktop) e le modifiche a `Engine.kt`/`Engine.android.kt`/`Engine.desktop.kt`
per il nuovo tipo `EditableLook`. L'ho riletto con attenzione più volte cercando proprio gli errori
tipici di Compose che non si vedono senza compilare (un bug concreto l'ho trovato e corretto così:
stavo usando il componente `Divider` per un separatore verticale, ma quel componente forza al suo
interno `.fillMaxWidth().height(...)`, quindi avrebbe ignorato la larghezza fissa richiesta — ora
uso una `Box` semplice). Ma resta tutto da compilare per la prima volta su CI, come sempre in
questo ambiente: se GitHub Actions segnala un errore Kotlin/Gradle, mandami il log e lo sistemiamo,
esattamente come fatto finora.

## Corretto: esposizione/tonalità troppo aggressive su "incolla impostazioni"

Dopo il giro precedente, un test reale ha mostrato un problema concreto: usando come campione una
foto scattata ed editata dall'utente stesso in basso-chiave (molto asfalto/ombre scure) e come
target la stessa foto non editata, il risultato era innaturalmente scuro e desaturato —
"esposizione -1.09 EV" applicata, ben oltre il ±0.5 EV che il guardrail di Smart-Batch dovrebbe
garantire come massimo.

Causa reale (non un'ipotesi): `harmonic::extract_look_from_reference` calcola `exposure_ev` come lo
scostamento ASSOLUTO tra la luminosità mediana della foto campione e un pivot di grigio neutro —
cioè "quanto è scura quella specifica foto", non "quanta correzione di esposizione andrebbe
replicata su un'altra scena" (impossibile saperlo con certezza da una sola immagine finale, senza
l'originale non editato per confronto). Questo valore, senza freni fino a ±2.0 EV, veniva sommato
per intero al delta di Smart-Batch — che invece è correttamente limitato — dominando il risultato
anche con lo slider "intensità adattamento" al 100%. La tone curve aveva lo stesso problema in
forma diversa: i suoi punti di controllo erano i percentili assoluti di luminosità del campione,
quindi una foto campione scura trascinava verso il basso il midtone di qualsiasi target, sommandosi
silenziosamente all'esposizione.

Corretto in due punti, entrambi con un test dedicato che riproduce lo scenario segnalato:
l'esposizione assoluta del campione ora si interpola con `(1 - intensità adattamento)` prima di
sommare il delta guardrailato di Smart-Batch (a slider 100% il campo resta per intero al delta
contestuale, ≤0.5 EV); la tone curve è ora calcolata relativa alla mediana del campione stesso, con
il midtone sempre ancorato al pivot neutro, così trasporta solo la forma del contrasto e non la
luminosità assoluta della scena campione. Aggiunto anche un guardrail difensivo sul moltiplicatore
di saturazione/vibrance nel renderer, per evitare desaturazioni estreme in casi simili. Dettagli
tecnici completi in [`engine/README.md`](engine/README.md).

## Cosa ho potuto verificare qui e cosa no

Verificato **per davvero**, in locale, prima di questa consegna: build e test dell'intero
workspace Rust (69 test, tutti verdi — inclusi i nuovi test su texture a bande di frequenza
(separazione di frequenza gaussiana), bilanciamento del bianco a gradiente per pixel, le
frazioni di clipping ombre/luci di "slider sicuri", e i cinque nuovi test sulla correzione del bug
di posterizzazione ai confini delle bande HSL — vedi la sezione "Corretto" più sotto). Rigenerati e ispezionati i binding Kotlin
generati da UniFFI dopo ogni cambio di forma dei dati passati al motore — sia per il nuovo oggetto
`PhotoEditSession` (costruttore, `renderPreview`/`renderFullResolution`/`pasteLookFromSample`,
lifecycle `Disposable`/`AutoCloseable`/`close()`) sia, in questo giro, per i nuovi campi di
`HarmonicLookFfi` (texture, WB zona B e parametri del gradiente) e per il nuovo tipo di ritorno di
`renderPreview` (`RenderedPreviewFfi`, che ora porta anche le due frazioni di clipping) — prima di
scrivere a mano il codice Kotlin che li richiama.

**Non verificabile da qui** (l'ambiente di sviluppo non ha un Android NDK né un PC Windows, e non
può scaricare un NDK per una verifica autonoma — la rete di questo ambiente blocca `dl.google.com`
per policy, verificato anche provando a scaricare direttamente i sorgenti di Compose per un
controllo extra sulle API usate): l'intera build Gradle, in particolare le modifiche più estese
lato Kotlin — il nuovo `PhotoEditSession` in `Engine.kt`/`Engine.android.kt`/`Engine.desktop.kt`, la
riscrittura di `App.kt` per il rendering dal vivo (`LaunchedEffect`/`snapshotFlow`/`collectLatest`),
la modalità a schermo intero, l'editor grafico della tone curve, il pannello HSL a tab, e in questo
giro anche i tre nuovi controlli (sezione "Dettaglio (Texture)", la colorazione ambra "slider
sicuri" su Esposizione/Alte luci/Ombre/Bianchi/Neri, e la sezione "Bilanciamento del bianco a
gradiente" con `Switch`/selettore d'asse). Riletto con attenzione più volte cercando gli errori
tipici di Compose che non si vedono senza compilare (riferimenti residui a funzioni/parametri
rimossi, chiavi ed effetti collaterali degli `Effect`, bilanciamento delle graffe, firme delle API
di disegno su `Canvas`, firme di `SliderDefaults.colors`/`SwitchDefaults.colors`) senza trovarne, ma
resta tutto da compilare per la prima volta su CI, come sempre in questo ambiente: se GitHub
Actions segnala un errore Kotlin/Gradle, mandami il log e lo sistemiamo, esattamente come fatto
finora.

## Cosa manca ancora (prossimo incremento)

- **Demosaic completo** per l'export a piena risoluzione (oggi `raw-decode` estrae solo
  l'anteprima incorporata dalla fotocamera, non l'immagine RAW "sviluppata" pixel per pixel) — il
  rendering lavora quindi sull'anteprima, non sul RAW pieno; il bilanciamento del bianco, pur ora
  reso, resta di conseguenza un'approssimazione da color grading e non un vero profilo colore
  camera.
- Collegare `gpu-pipe` (gli shader WGSL già validati) alla UI per il rendering a piena risoluzione
  in tempo reale, al posto della pipeline CPU attuale.
- `cache`, `catalog` (libreria/grid multi-foto), `job-scheduler` (batch reale su centinaia di
  foto insieme, non una alla volta) — oggi il flusso è a una foto campione + una foto target.
- Maschere locali (pennello/gradiente/radiale) copiabili nei preset.
- **"Coerenza di Set"** (idea originale, discussa e approvata con l'utente, da pianificare insieme
  alla fase libreria/batch): data un'intera cartella di uno shooting, raggruppare automaticamente
  le foto in "cluster di luce" (clustering sui descrittori di scena che `smartbatch` calcola già
  oggi per singola foto — nessun modello ML esterno, k-means su poche dimensioni) e applicare
  un'unica intenzione artistica di riferimento adattata PER CLUSTER, non foto per foto in
  isolamento — per garantire che un'intera galleria consegnata a un cliente sembri una storia
  visiva coerente anche quando le condizioni di luce cambiano scena per scena (cerimonia in
  chiesa, ricevimento al tramonto, sala buia col flash...). Differenzia da "Sincronizza
  impostazioni" (cieco alla scena) e da un "match color" foto-per-foto (non garantisce coerenza
  collettiva sull'intero set).
- **Quattro idee sui valori personalizzabili in editing**, discusse e approvate con l'utente,
  ordinate per rapporto valore/sforzo — **tutte e quattro fatte**, a questo giro:
  1. ~~Dial "Intensità edit" che scala l'intero editing verso lo zero (o lo esagera oltre il
     100%)~~ — vedi sotto.
  2. ~~**Texture a bande di frequenza** (fine/media/grossa invece di un solo slider
     "Texture")~~ — vedi sotto: separazione di frequenza gaussiana reale (`image::imageops::blur`
     a tre raggi), non un semplice contrasto locale.
  3. ~~**Slider "sicuri"**: colorare il binario dello slider quando il valore corrente produce
     clipping~~ — vedi sotto.
  4. ~~**Bilanciamento del bianco a più punti/a gradiente**~~ — vedi sotto: due zone (non punti
     liberi), sfumate linearmente lungo un asse verticale/orizzontale — la versione più semplice
     che risolve comunque il caso reale (cielo freddo/terreno caldo nella stessa inquadratura).

## Nuovo: dial "Intensità edit" (prima delle quattro idee sui valori personalizzabili)

Un unico slider in cima al pannello Develop (0%–150%, 100% = editing esatto) che scala l'INTERO
editing corrente verso lo zero (o lo esagera oltre il valore scelto), senza dover tornare indietro
slider per slider — la cosa che oggi in Lightroom non esiste: l'unico modo di "attenuare" un
editing è farlo in fase di applicazione di un preset ("Ammontare"), non su un editing che si sta
già facendo a mano.

Costruito interamente lato Kotlin, senza toccare il motore Rust: una funzione pura,
`EditableLook.scaledBy(intensity)`, interpola ogni singolo campo tra il suo valore NEUTRO (quello
di default, o l'identità per la tone curve) e il valore attualmente impostato dall'utente. Non
modifica mai `currentLook` — viene ricalcolata solo al momento del rendering/esportazione — così
riportare il dial al 100% ritorna sempre esattamente all'editing originale, senza arrotondamenti
che si accumulano avanti e indietro, e ogni singolo slider continua a modificare "il 100%"
esattamente come prima (nessuna complicazione nel back-solving dei valori mentre il dial è a una
posizione diversa da 100%). La tinta del viraggio (`shadowHue`/`highlightHue`, gradi assoluti senza
un "neutro" naturale) resta apposta invariata dal dial; conta solo quando la sua saturazione
(quella sì scalata) è diversa da zero.

## Nuovo: texture a bande di frequenza, slider "sicuri", bilanciamento del bianco a gradiente

Le altre tre idee approvate sui valori personalizzabili (§ sopra), tutte e tre in questo giro.

**Texture a bande di frequenza** (sezione "Dettaglio (Texture)", tre slider Fine/Media/Grossa,
-100..100 ciascuno). Vera separazione di frequenza, non un contrasto locale mascherato da un altro
nome: `look-render` sfoca l'immagine già color-gradata a tre raggi crescenti
(`image::imageops::blur`, sigma 1.2/4/10), ricava le bande di dettaglio per differenza tra sfocature
successive (quella più sfocata di tutte è il "residuo" a bassa frequenza — colore e tono di base,
mai toccato), poi ricompone scalando ogni banda di `1 + amount/100`. Con tutti gli amount a 0 la
ricostruzione è esatta (residuo + somma delle differenze = l'immagine originale): un'immagine a
tinta piatta resta quindi invariata qualunque sia l'amount, e solo il *dettaglio* locale cambia
ampiezza — non la luminosità media, a differenza di "Chiarezza"/contrasto. È un passo separato dal
loop per-pixel principale (un'operazione spaziale, non può vivere dentro un ciclo che processa un
pixel alla volta), eseguito solo se almeno uno dei tre amount è diverso da 0.

**Slider "sicuri"** (colorazione ambra su Esposizione/Alte luci/Ombre/Bianchi/Neri quando il
valore corrente produce clipping). `PhotoEditSession::render_preview` ora calcola, sull'anteprima
appena renderizzata, la frazione di pixel vicini al nero puro (luma ≤ 2) e al bianco puro (luma ≥
253) e le restituisce insieme ai bytes PNG (`RenderedPreviewFfi`/`RenderedPreview` lato Kotlin). La
UI confronta queste due frazioni con una soglia (2% dei pixel, scelta dichiarata: non un singolo
pixel isolato, ma clipping già percepibile) e colora in ambra badge e binario dello slider
interessato, più un avviso testuale sotto la sezione "Base". **Scelta di design dichiarata**: il
segnale riguarda solo il valore ATTUALE, non un'anteprima dell'intero range possibile dello
slider — dipingere l'intero binario richiederebbe ri-renderizzare l'immagine una volta per ogni
posizione possibile, troppo costoso per un feedback dal vivo mentre si trascina.

**Bilanciamento del bianco a gradiente** (nuova sezione "Bilanciamento del bianco a gradiente",
sotto "Colore"). Una `Switch` per attivarlo, un selettore verticale/orizzontale, due slider
posizione/ampiezza della transizione (0-100) e una seconda coppia temperatura/tinta per la "zona
B". Nel renderer, il loop per-pixel principale ora traccia la posizione `(x, y)` di ogni pixel
(prima assente — serviva solo per far girare rayon su righe/pixel senza sapere dove fossero) e, se
il gradiente è attivo, sfuma linearmente il guadagno WB tra la zona A e la zona B lungo l'asse
scelto, centrato sulla posizione configurata e largo quanto l'ampiezza configurata (0 = bordo
netto, 100 = sfumatura sull'intero fotogramma). **Scelta di design dichiarata**: due zone lungo un
asse, non punti liberi piazzabili a piacere sulla foto (quello richiederebbe una UI di
posizionamento 2D e un'interpolazione multi-punto molto più complessa) — la versione più semplice
che risolve comunque il caso reale discusso (cielo freddo in alto, terreno caldo in basso nella
stessa inquadratura).

## Corretto: immagine "a blocchi"/posterizzata su foto con tonalità che varia con continuità

Bug reale segnalato dall'utente con uno screenshot: su una foto con un fogliame ampio (tanti verdi
diversi, dal giallastro all'azzurrato), il rendering mostrava macchie di colore/luminosità nette a
forma di blocco, che seguivano i contorni della scena invece di una transizione morbida —
un'immagine "rovinata", non un ritocco.

**Causa reale**: `render_preview_with_look` (in `look-render`) applica gli aggiustamenti HSL per
banda colore (`HarmonicLook.hsl`, 8 bande — Rosso/Arancio/Giallo/Verde/Acqua/Blu/Viola/Magenta,
45° ciascuna) assegnando ogni pixel a un'UNICA banda in base alla sua tonalità
(`floor(hue / 45) % 8`) e applicandone l'aggiustamento per intero — un confine di banda NETTO ogni
45°, senza alcuna transizione. Su una tinta piatta questo non si vede mai (tutta l'immagine è nella
stessa banda); su una foto reale, dove la tonalità varia con continuità pixel per pixel (un cielo,
un prato, un incarnato), due pixel visivamente quasi identici possono finire in bande diverse se la
loro tonalità cade appena ai due lati di un confine — e se quella banda ha un aggiustamento diverso
(hue/saturazione/luminanza), il salto è visibile come un bordo artificiale netto. Con "Incolla
impostazioni" (che estrae aggiustamenti per banda reali e spesso diversi da una foto campione) o con
un editing manuale della sezione HSL, questo produce esattamente l'effetto "a blocchi" dello
screenshot: la scena non ha mai avuto quei bordi, li ha creati il rendering.

**Corretto**: `look-render` ora interpola linearmente e circolarmente (con wrap a 360°) tra i valori
delle DUE bande più vicine alla tonalità di ogni pixel (`interpolate_hsl_band`), invece di
applicare in blocco quello di un'unica banda. Ogni banda ha il suo pieno effetto solo al centro del
proprio intervallo di 45°; ai bordi fra due bande l'effetto sfuma linearmente 50/50 (la somma dei
pesi resta sempre 1 — nessun salto, nessuna banda "invisibile" durante la transizione). Il resto
della pipeline (bilanciamento del bianco, esposizione, tone curve, contrasto, split toning,
texture) non era interessato da questo bug ed è rimasto invariato.

Quattro nuovi test in `look-render` isolano il comportamento della sola interpolazione
(`hsl_band_interpolation_returns_exact_value_at_band_center`,
`hsl_band_interpolation_blends_evenly_exactly_at_a_boundary`,
`hsl_band_interpolation_wraps_around_360_degrees`,
`hsl_band_interpolation_has_no_hard_jump_across_a_boundary`), più un quinto end-to-end
(`render_preview_hsl_saturation_has_no_hard_jump_across_a_hue_band_boundary`) che riproduce lo
scenario reale: due tinte unite a tonalità quasi identica (134°/136°, appena ai due lati di un
confine banda), con un Look che alza molto la saturazione di una sola delle due bande — con il
vecchio bucket netto le due immagini finivano con saturazioni radicalmente diverse pur partendo da
tonalità quasi identiche, con l'interpolazione la differenza resta piccola.

## Corretto: dominante di colore diffusa segnalata come "peggiorata" dopo la correzione precedente

Secondo bug reale segnalato dall'utente con uno screenshot, subito dopo la consegna della
correzione qui sopra: su un'altra foto (niente più "a blocchi", ma una forte dominante
rosa/arancio su gran parte del fotogramma), con l'avviso "ombre schiacciate" delle slider sicure
visibile e uno slider Neri già portato a -60 in risposta a quell'avviso. Invece di modificare di
nuovo alla cieca partendo solo dallo screenshot, l'indagine è stata condotta scrivendo piccoli
programmi Rust di debug che usano le stesse funzioni del motore per stampare i valori intermedi
(il `HarmonicLook` estratto, i pixel renderizzati) e salvare il rendering risultante, così da
poter verificare — non solo ipotizzare — quale componente fosse davvero responsabile. Sono emersi
due difetti reali, distinti, entrambi corretti:

**1. `hsl_sat` nell'estrazione (`harmonic`) aveva un range troppo ampio.** La saturazione per
banda di tonalità (`HarmonicLook.hsl.sat[banda]`) veniva calcolata come scarto percentuale della
saturazione media della banda rispetto a una soglia fissa (`BASELINE_HSL_SATURATION = 0.35`),
poi limitata a `±100`. Ma quella media è una media aritmetica di valori sempre non-negativi (a
differenza, per esempio, del centroide Lab usato per lo split toning, che invece cancella
naturalmente il rumore in direzioni opposte): nella grande maggioranza delle foto reali, quasi
tutte le 8 bande hanno una saturazione media ben sotto 0.35 (poca scena è davvero satura), il che
spinge quelle bande verso l'estremo -100 ("desatura completamente questa tonalità"); al contrario
una banda che cattura anche poco colore incidentale può schizzare al +100 opposto ("raddoppia la
saturazione di questa tonalità"). Combinato con l'interpolazione circolare della correzione
precedente (che, correttamente, DIFFONDE l'effetto di un valore estremo anche sulle tonalità
vicine), il risultato su "Incolla impostazioni" poteva essere una dominante di colore diffusa e
innaturale — esattamente il tipo di effetto segnalato. **Corretto**: il range è stato ristretto a
`±50`, coerente con il fatto che questo è già il ritocco automatico più "rischioso" fra i tre HSL
per banda (`hsl_lum` era già limitato a ±30, `hsl_hue` a ±15) — lo slider MANUALE dell'HSL nella UI
resta invariato (±100, è una scelta creativa esplicita dell'utente, non un artefatto
dell'estrazione automatica).

**2. Gli slider "Bianchi"/"Neri" non avevano ALCUN effetto sul rendering.** Verificato con una
ricerca diretta nel codice (`grep` di `look.whites`/`look.blacks` in `look-render`): zero
riscontri. I due campi esistono nel modello dati, attraversano FFI e l'export `.xmp`, ma non
venivano mai letti dal renderer — un difetto preesistente, non introdotto in questa consegna, ma
scoperto proprio indagando su questo caso, perché era la causa diretta per cui portare Neri a -60
non aveva avuto alcun effetto visibile sull'avviso "ombre schiacciate" che l'utente stava cercando
di correggere. **Corretto**: aggiunte `blacks_mask`/`whites_mask`, zone tonali più STRETTE di
`shadow_mask`/`highlight_mask` (mirate ai soli estremi nero/bianco puro, non all'ampia metà
inferiore/superiore del range), con lo stesso segno di ombre/luci (positivo = schiarisce quella
zona). **Nota pratica per l'avviso "ombre schiacciate"**: la correzione giusta è portare
Neri/Ombre POSITIVO, non negativo — negativo le schiaccia ulteriormente. Prima di questa
correzione, portare Neri a un valore qualsiasi non cambiava nulla; ora Neri POSITIVO alza il
punto di nero come atteso.

**Verificato ma escluso come causa**: il bilanciamento del bianco a gradiente è stato testato
riproducendo ESATTAMENTE i valori dello screenshot dell'utente (Zona A/B, posizione e ampiezza
della transizione) — il rendering risultante è una transizione fredda/calda morbida e
ragionevole, non una dominante innaturale; la funzione stessa non è la causa del problema
segnalato. Anche `smartbatch::apply_deltas` (Smart-Batch Contestuale) è stato escluso leggendone
il codice per intero: modifica solo esposizione/luci/ombre, mai i campi del bilanciamento del
bianco a gradiente.

## Corretto: la dominante restava, ma SOLO su "Incolla impostazioni" — mai con l'editing manuale

L'utente ha confermato, dopo la consegna qui sopra, che il problema si presenta **esclusivamente**
quando si incollano le impostazioni dalla foto campione — mai modificando gli slider a mano. Un
indizio prezioso: confina la causa alla sola estrazione automatica (`harmonic`), esclude
definitivamente il bilanciamento del bianco a gradiente e ogni altro slider manuale (già peraltro
verificati sopra), e ha portato a un terzo bug reale, distinto dal primo:

**3. `split_toning.shadow_sat`/`highlight_sat` non aveva ALCUNA baseline, a differenza dei suoi
"fratelli".** A differenza di `hsl_sat` e `vibrance` (entrambi già uno SCARTO relativo a una
soglia tipica), lo split toning usava la chroma Lab GREZZA della zona ombre/luci, limitata solo a
`0..100` — nessun confronto con "quanto sia tipicamente colorata" quella zona in una foto
qualunque. Risultato verificato con uno script di debug dedicato: perfino una foto campione
scattata alla luce del giorno, SENZA alcuna intenzione di grading (solo la normale, lieve
differenza di colore fra cielo/ombra e sole diretto che ha qualunque scatto), produceva uno split
toning non trascurabile — copiato per intero sul target con "Incolla impostazioni" (mai
sull'editing manuale, dove lo split toning parte sempre da 0/0), e applicato per giunta su zone
tonali ampie (ombre sotto luma 0.4, luci sopra 0.6 — in molte foto la maggioranza dei pixel).
Questo spiega esattamente perché il problema fosse confinato al solo "Incolla impostazioni".
**Corretto**: sottratta una baseline di chroma tipica (`BASELINE_SPLIT_CHROMA = 6.0`) prima del
clamp, e range dimezzato a `0..50` (stessa proporzione già applicata a `hsl_sat`). Verificato con
uno script di debug end-to-end (estrazione + Smart-Batch + rendering) su una scena con contenuto
vario (cielo, fogliame, carrozzeria, asfalto — non bande piatte di test) che il risultato dopo
"Incolla impostazioni" resta un'interpretazione moderata dello stile, non una dominante estrema:
cielo ancora bluastro, fogliame ancora verdastro, carrozzeria e asfalto restano pressoché neutri.

Nuovo test in `harmonic` (`mild_incidental_color_variation_does_not_produce_split_toning`) verifica
che un cast lieve e non intenzionale non produca più split toning, mantenendo intatto il test
esistente che verifica il caso opposto (uno split "teal & orange" vero, con colori genuinamente
saturi, deve continuare a essere copiato).

**Onestà su cosa resta**: con queste tre correzioni (hsl_sat, whites/blacks, split_toning) tutti i
meccanismi di estrazione automatica che potevano produrre una dominante sproporzionata rispetto
allo stile reale della foto campione sono stati individuati nel codice, corretti e verificati con
test mirati e con ricostruzioni end-to-end del flusso "Incolla impostazioni" — non solo con lo
screenshot originale, che da solo non permette di isolare la causa esatta con certezza assoluta.
Se dovesse ripresentarsi un problema simile, condividere la foto campione e quella target
userebbe a individuare la causa esatta invece di continuare a ipotizzare da uno screenshot.

Due nuovi test in `look-render` (`positive_blacks_lifts_near_black_pixels_more_than_midtones`,
`negative_whites_pulls_near_white_pixels_down_more_than_midtones`) verificano il punto 2, sul
modello dell'analogo test già esistente per ombre/luci. Workspace completo: 72 test, tutti verdi.

**Verifica finale sulle foto vere dell'utente**: per sicurezza, oltre alle scene sintetiche di cui
sopra, le due correzioni sono state rieseguite sulle due foto reali fornite dall'utente (non un
test isolato: l'intero algoritmo di `PhotoEditSession::paste_look_from_sample`, stessi parametri di
default della UI — override_strength 100%, target ridotto a `INTERACTIVE_PREVIEW_MAX_DIM` come fa
davvero l'app). In entrambe le direzioni (ciascuna foto come campione, l'altra come target) lo
split toning estratto risulta `shadow_sat`/`highlight_sat` = 0 (nessun cast residuo da quel
meccanismo) e il rendering risultante, ispezionato visivamente, non mostra alcuna dominante
magenta/rosa: pavimentazione e carrozzeria restano in toni neutri, i sedili rossi restano rossi
(colore reale della foto, non un artefatto). Uniche osservazioni honeste, non legate al bug
segnalato: `vibrance` esce piuttosto negativo su entrambe le foto (-75/-53), quindi il risultato
appare più desaturato/spento del previsto — un effetto collaterale della baseline di vibrance
(`BASELINE_CHROMA`), non della dominante di colore ora corretta; se dovesse risultare eccessivo,
è un candidato separato per un futuro ritocco della baseline, da ricalibrare su un corpus più
ampio di foto reali (come già notato altrove in questo stesso file per le altre baseline
euristiche).

## Nuovo: zoom con rotella del mouse sulle foto, esportazione preset con scelta della cartella

Due richieste dirette dell'utente, entrambe lato UI Kotlin (nessuna modifica al motore Rust):

**Zoom sulle foto.** Ogni foto mostrata nell'app — il riquadro "Campione", il riquadro "Foto da
modificare"/anteprima, e la foto grande nella modalità "Develop a schermo intero" — ora si
ingrandisce/rimpicciolisce con la rotella del mouse quando il cursore è sopra di essa, tramite un
nuovo componente condiviso `ZoomableImage` (in `App.kt`). Lo zoom è centrato (nessun trascinamento
per spostarsi, non richiesto), limitato fra 1x (la dimensione "adatta al riquadro" originale, mai
più piccola) e 6x, e si azzera automaticamente quando una foto DIVERSA viene importata nello stesso
riquadro — ma NON ad ogni singolo fotogramma del rendering dal vivo mentre si trascina uno slider
del pannello Develop (il bitmap mostrato cambia decine di volte al secondo in quel caso: azzerare
lo zoom ad ogni fotogramma lo avrebbe reso di fatto inutilizzabile durante l'editing).

**"Esporta preset .xmp" scrive davvero un file.** Prima di questa modifica, il pulsante calcolava
il preset e ne mostrava solo un'anteprima di testo troncata nella UI — non veniva mai salvato su
disco. Ora, subito dopo il calcolo, si apre lo stesso selettore nativo già usato per "Esporta
foto…" (finestra di salvataggio AWT su Desktop, Storage Access Framework su Android), che lascia
scegliere la cartella di destinazione (e il nome file, precompilato ma modificabile) — nuovo file
`PresetSaverLauncher.kt` (+ le due implementazioni per piattaforma), analogo a
`FileSaverLauncher.kt` già esistente ma con contenuto testuale/XML invece che un'immagine PNG.

**Nota onesta sui limiti di questa verifica**: a differenza di tutte le correzioni Rust di questo
progetto (verificate qui con `cargo test`/`cargo run` prima di ogni consegna), queste due modifiche
sono Kotlin/Compose Multiplatform — in questo ambiente non è disponibile una toolchain
Kotlin/Gradle per compilarle o testarle localmente (limite noto fin dall'inizio di questo
progetto). Il codice segue pattern già presenti e funzionanti altrove in `App.kt` (stesso schema di
`pointerInput`/`detectDragGestures` già usato dall'editor della tone curve, stesso schema di
`rememberFileSaverLauncher` già usato per l'esportazione foto), ma la verifica definitiva resta la
build reale via GitHub Actions CI, come per ogni modifica alla UI di questo progetto.

## Corretto: "Incolla impostazioni" desaturava la foto target MOLTO oltre la foto campione stessa

Segnalazione dell'utente, con screenshot dell'app e le stesse due foto reali della verifica
precedente: il risultato di "Incolla impostazioni" diventava troppo desaturato, con "artefatti" che
lo rendevano inutilizzabile — la richiesta esplicita era che il risultato dovesse restare
visivamente fedele alla foto campione, non un'approssimazione spenta. Misurato con la chroma Lab
media (una metrica di "quanto è colorata" un'immagine, indipendente dalla luminosità — più adatta
di una saturazione HSL grezza, che invece dipende anche da quanto è chiara/scura l'immagine) sulle
due foto vere dell'utente: campione (il "look" da copiare) chroma ~4.46, target originale ~8.39,
target dopo "Incolla impostazioni" (prima di questa correzione) crollato a **~1.45** — un terzo
della chroma della stessa foto campione che doveva riprodurre, non solo "diversa dal target
originale". Isolando ogni stadio della pipeline uno alla volta su queste foto vere (script di
debug dedicati, non ipotesi), sono emerse **due cause reali distinte**, non una sola:

**1. `hsl_sat` ancora incoerente con `hsl_lum`/`hsl_hue`, nonostante la correzione precedente.** La
correzione della sessione precedente aveva solo ristretto il RANGE del bias (±100 → ±50), lasciando
il confronto ancorato a una costante fissa esterna (`BASELINE_HSL_SATURATION`) — a differenza dei
suoi due "fratelli" `hsl_lum`/`hsl_hue`, che confrontano ogni banda con la media dell'INTERA foto
campione (un confronto interno/relativo). Su una foto uniformemente poco satura (la normalità per
gran parte di una scena reale) questo spingeva quasi tutte le bande verso l'estremo negativo — e
quel bias, calcolato dalla STESSA caratteristica "la foto è poco colorata" già catturata da
`vibrance` (bias globale), si sommava moltiplicativamente al bias globale invece di aggiungere
informazione nuova: la foto veniva penalizzata due volte per lo stesso fatto. **Corretto**: `hsl_sat`
ora confronta ogni banda con la saturazione media dell'INTERA foto campione, esattamente come
`hsl_lum`/`hsl_hue` — una banda riceve un bias solo se è più/meno satura del resto di QUESTA foto,
mai per il livello di colore assoluto della foto nel suo complesso (quel giudizio spetta solo a
`vibrance`). Anche `BASELINE_CHROMA` (il bias globale di `vibrance`) è stata ricalibrata: era 18.0,
una stima "a occhio" mai verificata — misurata ora sulle due foto vere, la chroma media effettiva
era ~4.5 e ~8.5, meno della metà della vecchia baseline in entrambi i casi. Portata a 10.0.

**2. Tone curve e contrasto applicati canale per canale, non alla luminosità.** La causa PIÙ
GRANDE, e la più sottile: `render_preview_with_look` applicava sia la tone curve sia il contrasto
in modo indipendente su ciascun canale R/G/B. Applicare la STESSA curva/riscalamento a ciascun
canale separatamente comprime le differenze FRA i canali — cioè comprime la saturazione — come
effetto collaterale non voluto, anche con valori non estremi: misurato sulle foto vere, la sola
tone curve tagliava la chroma media di circa il 40%, il contrasto da solo di un altro ~25%, in
sequenza. La lift di ombre/luci (già esistente) non soffriva di questo problema perché è additiva e
identica su tutti e tre i canali — sposta la luminosità senza toccare le differenze fra i canali.
**Corretto** applicando lo stesso principio anche a tone curve e contrasto: si calcola quanto la
LUMINOSITÀ del pixel dovrebbe cambiare (secondo la curva o il contrasto), poi si sposta ogni canale
di quello stesso ammontare — invece di ricalcolare ogni canale in modo indipendente. Il colore
(hue e chroma) resta intatto, cambia solo la luminosità, esattamente come un vero editor fotografico
professionale.

**Risultato dopo entrambe le correzioni**, misurato sulle stesse foto vere: chroma del rendering
finale passata da ~1.45 a **~6.24** — non più un terzo della chroma del campione, ma superiore alla
chroma del campione stesso (4.46) e ragionevolmente vicina all'originale del target (8.39).
Ispezionato visivamente: i sedili restano rossi vividi (non più smorti), la pavimentazione porta la
tonalità calda della foto campione invece di un grigio spento, nessun artefatto visibile. Nuovo
test in `look-render` (`negative_contrast_and_a_real_tone_curve_do_not_collapse_saturation`) isola
esattamente questo meccanismo con un singolo pixel sintetico, e due nuovi test in `harmonic`
(`saturated_band_gets_a_positive_saturation_bias_others_stay_at_zero`, riscritto per il nuovo
confronto relativo, e `uniformly_saturated_photo_gives_no_per_band_bias_only_global_vibrance`)
coprono il punto 1. Workspace completo: 75 test, tutti verdi.

**Onestà su cosa resta**: due foto reali non sono un corpus — `BASELINE_CHROMA = 10.0` e il
moltiplicatore `150.0` di `hsl_sat` restano stime, seppur ora ancorate a una misura vera invece che
a un'ipotesi, e vanno probabilmente ritarate ulteriormente quando saranno disponibili più foto di
riferimento. Il principio strutturale (curve/contrasto sulla luminosità, non per canale; bias
per-banda relativo alla stessa foto, non a una costante esterna) è invece una correzione solida,
non un numero da ritarare.

## Corretto: la build falliva su GitHub Actions con "Unresolved reference: graphicsLayer"

Il primo giro di CI dopo l'aggiunta dello zoom con rotella del mouse è fallito su
`:shared:compileKotlinDesktop` (runner Windows) con due errori identici in `App.kt`,
righe 19 e 847: `Unresolved reference: graphicsLayer`. La causa era un semplice import
dal pacchetto sbagliato: `androidx.compose.ui.draw.graphicsLayer` invece del pacchetto
corretto `androidx.compose.ui.graphics.graphicsLayer` (l'estensione `Modifier.graphicsLayer`
vive nel modulo `ui-graphics`, non `ui` base, a differenza di `clipToBounds` che invece è
davvero in `androidx.compose.ui.draw` — da qui la confusione). Corretto l'import in
`App.kt`; nessun'altra occorrenza nel progetto. Non è un problema specifico di Windows: lo
stesso identico errore avrebbe bloccato anche la build Android, semplicemente il job
Windows è quello che ha girato ed è fallito per primo.

## Corretto: i sedili rossi restavano desaturati anche dopo la correzione precedente

Dopo il fix di compilazione sopra, l'utente ha potuto finalmente testare dal vivo la correzione
precedente ("Incolla impostazioni" desaturava la foto target...") — e ha confermato che il problema
persisteva sui sedili rossi delle sue foto vere, con un risultato descritto come "desaturato e
glitchato, inutilizzabile". Rieseguendo lo stesso script di debug (questa volta misurando la chroma
Lab SOLO sulla regione dei sedili, non su tutta la foto) è emerso che il fix precedente aveva
davvero migliorato la chroma MEDIA dell'intera immagine, ma i sedili specificamente uscivano ancora
molto più desaturati (18.2) sia del target originale (35.0) sia del campione (36.7) — l'opposto
dell'obiettivo.

Causa reale: `vibrance` (fortemente negativo su questa foto, per via del suo ampio asfalto grigio
quasi neutro) veniva applicato come moltiplicatore PIATTO, uguale su ogni pixel qualunque fosse la
sua saturazione di partenza — che riduce in valore ASSOLUTO più i pixel già molto saturi (i sedili
rossi) di quelli quasi grigi. È l'opposto di cosa significa "vibrance" in un editor fotografico
reale: a differenza di "saturation" (moltiplicatore piatto, intenzionale), la vibrance è pensata per
proteggere i colori già vividi. Corretto sostituendo il moltiplicatore piatto con la formula
standard di vibrance non lineare, che protegge quadraticamente i pixel già saturi (moltiplicatore →
1.0, nessun effetto, quando la saturazione di partenza è già alta) e concentra l'effetto sui pixel
quasi neutri (dove comunque non è percepibile). Risultato misurato: chroma dei sedili 18.2 → 35.5,
ora vicina sia al target originale sia al campione. Dettagli tecnici e numeri completi in
`engine/README.md`.

## Corretto: su Android non si vedevano entrambe le foto — UI compressa in verticale

Stesso messaggio dell'utente sopra segnalava anche che su Android l'app risultava "inutilizzabile":
il pannello "Develop" a destra ha una larghezza fissa (320dp nella schermata principale, 360dp in
"Modifica a schermo intero") pensata per una finestra desktop larga; su un telefono (tipicamente
360-400dp di larghezza TOTALE) quel solo pannello consuma quasi tutto lo schermo, lasciando alle foto
pochi pixel e costringendo il resto del contenuto (pulsanti, slider) a schiacciarsi in verticale per
starci comunque.

Corretto in entrambe le schermate: sotto una soglia di larghezza (700dp) il layout passa da
affiancato a impilato verticalmente — stesso contenuto, disposizione diversa, decisa a runtime in
base allo spazio davvero disponibile invece che assumere sempre una finestra larga. Sulla schermata
di confronto le due foto restano affiancate (c'è comunque spazio per due miniature anche su un
telefono) con un'altezza fissa, e il pannello Develop scende sotto a piena larghezza; nella modalità
a schermo intero foto e pannello Develop si dividono lo spazio verticale a metà. **Non verificabile
in questo ambiente** (limite noto, solo Rust è testabile localmente qui): la verifica reale resta la
prossima build Android su GitHub Actions.

## Corretto: glitch/rumore a chiazze ancora visibile dopo il fix della vibrance

Dopo il fix della vibrance sopra, l'utente ha segnalato che il glitch persisteva ancora ("glitch
non risolti") e ha indicato quattro possibili cause tecniche: mismatch di spazio colore/profilo,
mancanza di uno spazio di lavoro comune a 32 bit in virgola mobile, uno shader GPU che satura, una
curva di transfer non normalizzata. Invece di applicare queste ipotesi alla cieca, ognuna è stata
verificata direttamente sul codice e sulle due foto vere, con risposta onesta punto per punto:

- **Spazio colore/profilo ICC**: le due foto vere sono state ispezionate direttamente (PIL/
  ImageCms). Sono entrambe sRGB (una con profilo esplicito, una senza ma comunque sRGB) — **nessun
  mismatch reale per queste foto**. Il motore però non ha alcuna gestione dei profili ICC in
  generale: un limite architetturale reale, disclosurato per onestà, ma non la causa di questo bug.
- **Spazio di lavoro a 32 bit float**: già vero, verificato — l'intera pipeline in `look-render`
  lavora in `f32` dall'inizio alla fine di ogni stage, con conversioni a 8 bit solo in lettura/
  scrittura.
- **Shader GPU**: il percorso di rendering live è interamente CPU; il crate `gpu-pipe` (WGSL) esiste
  ma non è collegato a questa funzionalità, quindi non può saturare né produrre questi artefatti.
- **Curva di transfer/"matrice di trasformazione cromatica"**: la curva tonale è già applicata solo
  sulla luminosità (fix di un giro precedente), e questa architettura non usa affatto una matrice di
  trasformazione cromatica — funziona per bande di tonalità HSL, non per matrice 3x3.

La causa reale del glitch era un'altra, trovata misurando i valori HSL estratti dalla foto
campione: un salto fino a 45 punti fra bande di tonalità circolarmente adiacenti, che amplifica il
normale jitter di tonalità pixel-per-pixel (texture, compressione JPEG, rumore sensore) in
un'oscillazione di saturazione visibile come chiazza/speckle. Corretto con una nuova media mobile
circolare a 3 prese sulle bande HSL estratte, prima di applicare il Look. Salto massimo fra bande
adiacenti sceso da 45 a 17 punti; metrica di rugosità (rumore ad alta frequenza, proxy autoprodotta)
scesa da 2.13 a 1.96, ora sotto il valore della foto originale stessa (2.29). Dettagli tecnici,
numeri completi e nuovo test in `engine/README.md`.

## Corretto (questo giro): aggiustamento HSL per banda a piena forza su pixel quasi neri, e verifica seria della "grana" nel paraurti

Dopo tutti i fix precedenti l'utente ha risposto con due screenshot: "non ci siamo per niente...
trova la causa di questo schifo e risolvilo una volta per tutte", chiedendo esplicitamente di non
rispondere finché la causa non fosse trovata. Ispezionando pixel per pixel la zona più scura delle
sue foto vere (la presa d'aria/griglia sotto il paraurti) è emerso un bug reale distinto da tutti
quelli già corretti: un pixel quasi grigio ha una tonalità numericamente instabile (il minimo
rumore di sensore o compressione JPEG, presente in qualunque foto reale, fa oscillare selvaggiamente
quale canale risulti max/min), ma l'aggiustamento hue-selettivo per banda veniva comunque applicato
a piena forza, ignorando quanto fosse davvero colorato il pixel. Un primo tentativo di correzione
(pesare in base alla saturazione HSL) è stato scartato dopo aver misurato che non funzionava — la
formula della saturazione HSL ha un polo matematico vicino al nero puro che fa apparire "molto
saturo" anche un pixel quasi nero con solo rumore minimo. **Corretto** pesando invece sulla croma
assoluta (la vera differenza fra i canali, senza quel polo). Dettagli tecnici, formula e nuovi test
in `engine/README.md`.

**Verifica seria e onesta della grana visibile nel paraurti**: oltre al fix sopra, è stato
verificato empiricamente SE la grana visibile in quella zona scura fosse causata dalla pipeline di
elaborazione — renderizzando la stessa foto vera sia con il Look completo di "Incolla impostazioni"
sia con un Look reso quasi nullo (tutti i controlli azzerati o a identità). La grana in quella zona
è risultata IDENTICA nei due casi, ed è già presente anche nell'anteprima ridotta senza alcun Look
applicato: è rumore di sensore/compressione JPEG già presente nella foto originale, reso più
visibile solo dallo zoom elevato usato per ispezionarlo — non qualcosa che questo motore introduce o
amplifica. Una vera riduzione del rumore (limite già noto e pianificato per una fase successiva
della roadmap) resta l'unico modo per attenuare attivamente quella grana specifica, e non è ancora
stata implementata in questo giro.

## Corretto (questo giro): "Intensità adattamento" non attenuava affatto il contrasto/tone curve del campione

Dopo il giro precedente l'utente ha inviato tre foto, chiarendo che il problema reale non era
"grana" ma — testuale — "i rettangoli grigi che si creano e la mancanza totale di contrasto":
la pavimentazione della foto (lastroni rettangolari con texture reale) diventava piatta e senza
dettaglio dopo "Incolla impostazioni", a qualunque valore dello slider "Intensità adattamento".

Causa reale: `contrast` e `tone_curve` — a differenza di esposizione/luci/ombre, già tarati in
base a quello slider — venivano sempre copiati LETTERALMENTE dalla foto campione, ignorando
completamente la posizione dello slider. Se il campione ha una grana volutamente piatta (come in
questo caso), quella piattezza finiva sul target senza che l'utente avesse alcun modo di attenuarla
con lo strumento pensato apposta per farlo. **Corretto** sfumando anche questi due campi verso un
valore neutro in proporzione allo stesso slider, con lo stesso principio già usato per
l'esposizione. Misurato sulla stessa foto vera: contrasto locale della pavimentazione tornato dal
55% al 94% di quello originale. Dettagli tecnici e nuovi test in `engine/README.md`.

## Corretto (questo giro): la tinta del target non convergeva verso quella del campione dopo "Incolla impostazioni"

Dopo il fix del contrasto sopra, l'utente ha segnalato l'ultimo problema visibile: "i colori non
corrispondono del tutto... la tinta anche è parecchio diversa". Misurato sui sedili rossi delle due
foto vere dell'utente: la chroma era già ben recuperata (94% dell'originale, vicina anche a quella
del campione), ma la tonalità restava sostanzialmente quella di partenza del target — praticamente
invariata dopo "Incolla impostazioni" (340.2° → 339.7°, in gradi HSL), lontanissima dai 333.0° del
campione.

Causa reale: l'HSL per banda copiato dal campione è, per costruzione, uno scarto RELATIVO al
proprio centro-banda (un "vezzo stilistico" del campione), non un valore assoluto verso cui il
target deve convergere — due foto dello stesso soggetto sotto luce diversa restano quindi diverse
fra loro dopo il paste esattamente quanto lo erano prima, e lo slider "Intensità adattamento" non
aveva alcuna leva su questo. **Corretto** con un meccanismo di hue-matching nuovo e complementare:
per ogni banda di tonalità con popolazione sufficiente in ENTRAMBE le foto, si confronta la
tonalità MISURATA del campione con quella MISURATA del target (non il centro-banda canonico) e si
applica la differenza, pesata dallo stesso slider "Intensità adattamento" (0% = nessun cambiamento,
100% = massimo consentito da un guardrail di 45°). Rimisurato sulla stessa foto vera: tonalità del
target dopo il fix 334.3°, a soli 1.3° dal campione (contro i 6.7° di prima) — una riduzione
dell'81% del divario, con la saturazione già recuperata rimasta invariata. Dettagli tecnici, nuovi
test e i limiti onesti di questo approccio (un confronto globale per banda, non un riconoscimento
del soggetto) in `engine/README.md`.

## Nuovo (questo giro): riduzione del rumore, e un riconoscimento del soggetto onestamente NON collegato al color-matching

Richiesta dell'utente: "puoi provare anche a implementare un riconoscimento soggetto, riduzione del
rumore colore e luminanza?".

**Riduzione del rumore**: fatta e verificata sulle foto vere. Due nuovi controlli (luminanza e
colore, 0-100, esportati nei tag `.xmp` reali del pannello Dettaglio Lightroom/ACR) che sfocano
separatamente la luminosità e il colore in spazio Lab, con protezione ai bordi in modo che i dettagli
veri della scena (per esempio la mesh di una griglia) restino nitidi mentre le zone piatte e rumorose
vengono ripulite. Misurato con la rugosità locale (non la deviazione standard grezza, che confonderebbe
un vero gradiente di luce con rumore): 39-45% di riduzione del rumore di luminanza, 35.7% del rumore
cromatico, a piena intensità.

**Riconoscimento soggetto**: implementato con una tecnica classica (contrasto di colore + centratura
fotografica, non un modello ML) e verificato — su una foto vera, illumina correttamente l'auto e
lascia scuro l'asfalto. Ho però provato a usarlo per migliorare ulteriormente "Incolla impostazioni"
(pesare l'abbinamento colore verso il soggetto invece che sull'intera foto) e la misura sulle foto
vere dell'utente ha mostrato un peggioramento, non un miglioramento: lo scarto di tonalità dei sedili,
già ridotto a 1.3° dal fix precedente, saliva a 9.5° con questo collegamento attivo. Invece di
consegnare una regressione mascherata da funzionalità nuova, ho annullato completamente quel
collegamento (riverificando che il fix della tinta tornasse esattamente com'era) e ho pubblicato il
riconoscimento del soggetto come funzione indipendente e ispezionabile
(`compute_subject_saliency_preview`, restituisce una mappa in scala di grigi), pronta per un futuro
utilizzo guidato dall'utente in UI ma non ancora collegata a nessuna regolazione automatica. Dettagli
tecnici completi, inclusa la causa della regressione, in `engine/README.md`.

## Build locale (facoltativo, per chi ha già Android Studio / JDK 17 / NDK installati)

```bash
# Motore Rust (tutti i crate e i loro test)
cd engine && cargo test --workspace

# Android (richiede NDK + cargo-ndk installati)
./gradlew :androidApp:assembleDebug

# Windows (installer .exe, solo da Windows con WiX Toolset installato)
./gradlew :desktopApp:packageExe
```

Per l'uso quotidiano però non serve nulla di tutto questo: basta caricare il repository
su GitHub e lasciare che sia GitHub Actions a compilare tutto. Le istruzioni passo-passo
sono nel messaggio che accompagna questo file.
