# RawForge

Scaffold di partenza per RawForge, l'alternativa ultra-veloce a Lightroom (Windows + Android),
progettata secondo l'architettura descritta in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Cosa contiene questo repository

- **`shared/`, `androidApp/`, `desktopApp/`** — un'app Kotlin Multiplatform + Compose
  Multiplatform reale e completa (stessa UI su Android e Windows). I due pulsanti della UI
  ("Stato motore" e "Genera preset XMP di esempio") chiamano **davvero** il motore Rust
  tramite i binding UniFFI, non più un placeholder.
- **`engine/`** — il workspace Rust del motore di elaborazione immagini: `core-types`,
  `color-science`, `harmonic` (Sintesi Armonica), `smartbatch` (Smart-Batch Contestuale),
  `metadata` (sidecar non distruttivo), `xmp` (export preset Lightroom), `gpu-pipe` (shader
  WGSL validati con `naga`) e **`ffi`** (la superficie UniFFI che collega tutto quanto sopra
  a Kotlin). 27 test, tutti verdi, eseguiti in locale prima di ogni consegna. Dettagli in
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

## Cosa ho potuto verificare qui e cosa no

Verificato **per davvero**, in locale, prima di questa consegna: build e test dell'intero
workspace Rust (27 test), compilazione del crate `ffi`, generazione reale dei binding
Kotlin dal `.so` compilato e ispezione del loro contenuto (nomi di funzioni, tipi, gestione
errori — tutto corrisponde). Ho anche corretto un bug reale in uno shader WGSL che la
validazione con `naga` ha scovato (indicizzazione dinamica non ammessa in un array di
uniform buffer).

**Non verificabile da qui** (l'ambiente di sviluppo non ha un Android NDK né un PC Windows):
la cross-compilazione effettiva per le 3 architetture Android via `cargo-ndk`, e l'intera
build Gradle (Android + Compose Desktop) — l'app Kotlin Multiplatform non è mai stata
compilata per davvero prima d'ora. Questa è la parte che osserveremo insieme nei log della
prima esecuzione su GitHub Actions: se qualcosa è rosso invece che verde, mandami il log
dell'errore e lo sistemiamo.

## Cosa manca ancora (prossimo incremento)

`raw-decode` — il collegamento a LibRaw per leggere i file RAW veri delle fotocamere.
`libraw-dev` compila e passa i test su Linux, ma cross-compilare la libreria C++ di LibRaw
per le ABI Android (via NDK) è un problema a sé, volutamente lasciato fuori da questo giro:
ha senso affrontarlo una volta che questa pipeline UniFFI si sarà dimostrata verde su un
vero APK/EXE.

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
