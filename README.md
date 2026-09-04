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
  Look ai pixel su CPU — bilanciamento del bianco, esposizione, tone curve, contrasto,
  highlights/shadows, HSL per banda, split toning) e **`ffi`** (la superficie UniFFI che collega
  tutto quanto sopra a Kotlin, incluso l'oggetto stateful `PhotoEditSession` per il rendering dal
  vivo, vedi sotto). 56 test, tutti verdi, eseguiti in locale prima di ogni consegna. Dettagli in
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

**Cosa NON c'è ancora in questo giro** (dichiarato, non nascosto): editor grafico della tone curve
(trascinare i punti a mano) e slider HSL per singola banda colore — il motore li supporta e li
calcola già (Sintesi Armonica include ora anche l'estrazione HSL per banda, vedi sotto), manca solo
l'interfaccia; le maschere locali (pennello/gradiente/radiale) copiabili nei preset, la
libreria/catalogo delle foto già modificate, e il batch reale su tante foto insieme — tutti
pianificati per gli incrementi successivi, come discusso.

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
workspace Rust (56 test, tutti verdi — inclusi i nuovi test su `PhotoEditSession`, sul
bilanciamento del bianco reso nel renderer, e sull'estrazione HSL per banda). Rigenerati e
ispezionati i binding Kotlin generati da UniFFI per il nuovo oggetto `PhotoEditSession`
(confermata la forma esatta: costruttore, `renderPreview`/`renderFullResolution`/
`pasteLookFromSample`, lifecycle `Disposable`/`AutoCloseable`/`close()`) prima di scrivere a mano
il codice Kotlin che lo richiama.

**Non verificabile da qui** (l'ambiente di sviluppo non ha un Android NDK né un PC Windows, e non
può scaricare un NDK per una verifica autonoma — la rete di questo ambiente blocca `dl.google.com`
per policy): l'intera build Gradle, in particolare le modifiche più estese di questo giro lato
Kotlin — il nuovo `PhotoEditSession` in `Engine.kt`/`Engine.android.kt`/`Engine.desktop.kt` e la
riscrittura di `App.kt` per il rendering dal vivo (`LaunchedEffect`/`snapshotFlow`/`collectLatest`,
gli slider semplificati, i nuovi stati `session`/`exportBusy`). Riletto con attenzione più volte
cercando gli errori tipici di Compose che non si vedono senza compilare (riferimenti residui a
funzioni/parametri rimossi, chiavi ed effetti collaterali degli `Effect`, wiring degli stati) senza
trovarne, ma resta tutto da compilare per la prima volta su CI, come sempre in questo ambiente: se
GitHub Actions segnala un errore Kotlin/Gradle, mandami il log e lo sistemiamo, esattamente come
fatto finora.

## Cosa manca ancora (prossimo incremento)

- **Demosaic completo** per l'export a piena risoluzione (oggi `raw-decode` estrae solo
  l'anteprima incorporata dalla fotocamera, non l'immagine RAW "sviluppata" pixel per pixel) — il
  rendering lavora quindi sull'anteprima, non sul RAW pieno; il bilanciamento del bianco, pur ora
  reso, resta di conseguenza un'approssimazione da color grading e non un vero profilo colore
  camera.
- Collegare `gpu-pipe` (gli shader WGSL già validati) alla UI per il rendering a piena risoluzione
  in tempo reale, al posto della pipeline CPU attuale.
- Editor grafico della tone curve e slider HSL per singola banda nell'interfaccia (il motore li
  supporta e li calcola già, vedi sopra).
- `cache`, `catalog` (libreria/grid multi-foto), `job-scheduler` (batch reale su centinaia di
  foto insieme, non una alla volta) — oggi il flusso è a una foto campione + una foto target.

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
