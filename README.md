# RawForge

Scaffold di partenza per RawForge, l'alternativa ultra-veloce a Lightroom (Windows + Android),
progettata secondo l'architettura descritta in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Cosa contiene questo repository

- **`shared/`, `androidApp/`, `desktopApp/`** — un'app Kotlin Multiplatform + Compose
  Multiplatform reale e completa (stessa UI su Android e Windows). La UI ha due parti:
  1. la demo minimale originale ("Stato motore" / "Genera preset XMP di esempio"), già verificata
     end-to-end in CI su entrambe le piattaforme;
  2. un flusso di importazione **vero**: un pulsante "Importa foto…" apre il selettore di file
     nativo di ciascuna piattaforma (finestra di dialogo AWT su Windows, Storage Access Framework
     su Android), decodifica davvero il file scelto tramite il motore Rust — anche un file **RAW
     vero** di una fotocamera, non solo JPEG/PNG — ne mostra l'anteprima e permette di applicarci
     la Sintesi Armonica Automatica esportando subito un preset Lightroom `.xmp`.
- **`engine/`** — il workspace Rust del motore di elaborazione immagini: `core-types`,
  `color-science`, `harmonic` (Sintesi Armonica), `smartbatch` (Smart-Batch Contestuale),
  `metadata` (sidecar non distruttivo), `xmp` (export preset Lightroom), `gpu-pipe` (shader
  WGSL validati con `naga`), **`raw-decode`** (decodifica RAW vera, vedi sotto) e **`ffi`** (la
  superficie UniFFI che collega tutto quanto sopra a Kotlin). 39 test, tutti verdi, eseguiti in
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

## Cosa ho potuto verificare qui e cosa no

Verificato **per davvero**, in locale, prima di questa consegna: build e test dell'intero
workspace Rust (39 test, incluso `raw-decode` con test sui percorsi di errore — file non
validi/corrotti gestiti senza panic), compilazione del crate `ffi` con la nuova superficie RAW,
generazione reale dei binding Kotlin dal `.so` compilato e ispezione del loro contenuto (le nuove
funzioni `decodeRawFilePreview`/`extractLookFromRawReference`/`isKnownRawFileName` compaiono
correttamente, nessuna collisione di nomi come quella già risolta in precedenza). Confermato anche
che `rawler` non ha uno `build.rs` e non compila codice C/C++: tutte le sue dipendenze sono a loro
volta crate Rust puri.

**Non verificabile da qui** (l'ambiente di sviluppo non ha un Android NDK né un PC Windows, e non
può scaricare un NDK per una verifica autonoma — la rete di questo ambiente blocca `dl.google.com`
per policy): la cross-compilazione effettiva di `raw-decode`/`rawler` per le 3 architetture
Android, l'intera build Gradle con la nuova UI di importazione (file picker + decodifica bitmap +
Sintesi Armonica su un file scelto dall'utente), mai compilata per davvero prima d'ora. Questa è
la parte che osserveremo insieme nei log della prossima esecuzione su GitHub Actions: se qualcosa
è rosso invece che verde, mandami il log dell'errore e lo sistemiamo, come abbiamo già fatto finora.

## Cosa manca ancora (prossimo incremento)

- **Demosaic completo** per l'export a piena risoluzione (oggi `raw-decode` estrae solo
  l'anteprima incorporata dalla fotocamera, non l'immagine RAW "sviluppata" pixel per pixel).
- Collegare `gpu-pipe` (gli shader WGSL già validati) alla UI per un vero modulo Develop con
  slider in tempo reale, invece del solo export XMP.
- `cache`, `catalog` (libreria/grid multi-foto), `job-scheduler` (batch reale su centinaia di
  foto, Smart-Batch Contestuale collegato alla UI) — oggi il flusso è a una foto per volta.

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
