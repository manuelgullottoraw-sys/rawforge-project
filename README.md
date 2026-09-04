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
  Look ai pixel su CPU, per l'anteprima "incolla impostazioni", vedi sotto) e **`ffi`** (la
  superficie UniFFI che collega tutto quanto sopra a Kotlin). 45 test, tutti verdi, eseguiti in
  locale prima di ogni consegna. Dettagli in [`engine/README.md`](engine/README.md).
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

## Cosa ho potuto verificare qui e cosa no

Verificato **per davvero**, in locale, prima di questa consegna: build e test dell'intero
workspace Rust (45 test, tutti verdi — inclusi 7 test di proprietà su `look-render`: un Look
neutro non altera l'immagine, esposizione positiva/negativa schiarisce/scurisce, il recupero ombre
schiarisce i pixel scuri più di quelli chiari, dimensioni dell'immagine invariate, e così via),
compilazione del crate `ffi` con la nuova superficie, generazione reale dei binding Kotlin dal
`.so` compilato e ispezione del loro contenuto (`pasteLookOntoTargetPhoto` prende solo
bytes/stringhe primitive, `HarmonicLookFfi`/`TonePointFfi`/`AdaptedRenderFfi` hanno la forma
attesa, nessuna collisione di nomi come quella già risolta in precedenza).

**Non verificabile da qui** (l'ambiente di sviluppo non ha un Android NDK né un PC Windows, e non
può scaricare un NDK per una verifica autonoma — la rete di questo ambiente blocca `dl.google.com`
per policy): l'intera build Gradle con la nuova UI (due import, uno slider, il rendering
dell'anteprima incollata), mai compilata per davvero prima d'ora — la parte Kotlin più estesa
consegnata finora in un colpo solo. Questa è la parte che osserveremo insieme nei log della
prossima esecuzione su GitHub Actions: se qualcosa è rosso invece che verde, mandami il log
dell'errore e lo sistemiamo, come abbiamo già fatto finora.

## Cosa manca ancora (prossimo incremento)

- **Demosaic completo** per l'export a piena risoluzione (oggi `raw-decode` estrae solo
  l'anteprima incorporata dalla fotocamera, non l'immagine RAW "sviluppata" pixel per pixel) — il
  rendering di "incolla impostazioni" lavora quindi sull'anteprima, non sul RAW pieno.
  Il bilanciamento del bianco (temp/tint) non viene ancora applicato nel rendering per lo stesso
  motivo (richiederebbe un profilo colore camera).
- Collegare `gpu-pipe` (gli shader WGSL già validati) alla UI per il rendering a piena risoluzione
  in tempo reale, al posto della pipeline CPU attuale.
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
