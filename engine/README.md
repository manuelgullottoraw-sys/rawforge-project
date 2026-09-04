# RawForge Engine (Rust)

Workspace del motore nativo di RawForge, come descritto in `../docs/ARCHITECTURE.md`.

## Stato attuale

Crate reali, compilati e testati (27 test, tutti verdi):

| Crate | Cosa fa | Rif. architettura |
|---|---|---|
| `core-types` | Strutture dati condivise (`HarmonicLook` e affini) | §5.1 |
| `color-science` | Conversioni sRGB↔lineare, RGB↔Lab, RGB↔HSL | §3.2 |
| `harmonic` | Sintesi Armonica: estrae tone curve, palette (split toning), contrasto e WB da un'immagine di riferimento | §4.1 |
| `smartbatch` | Smart-Batch Contestuale: descrittori di scena da istogramma + calcolo dei delta adattivi con guardrail | §4.2 |
| `metadata` | Sidecar JSON non distruttivo (schema versionato, history di operazioni) | §3.1 |
| `xmp` | Generatore di preset Lightroom `.xmp` dal `HarmonicLook` | §5 |
| `gpu-pipe` | Sorgenti WGSL degli stage di color grading, validati con `naga` (nessuna GPU richiesta per i test) | §3.2, §6.2 |
| `ffi` | Superficie **UniFFI** che espone `harmonic`/`xmp`/`smartbatch` a Kotlin — è questo il crate che la pipeline CI compila per Android (via `cargo-ndk`) e Windows (nativo), generando anche i binding Kotlin usati da `shared/` | §1, §7 |

**Novità**: il motore non è più "isolato" — `ffi` collega davvero `harmonic` e `xmp` alla UI
Kotlin. `Engine.versionInfo()` e `Engine.generateSampleXmpPreset()` in `shared/` chiamano
adesso il motore Rust vero (non più un placeholder), attraverso i binding generati da
`uniffi-bindgen` e la libreria nativa compilata dalla pipeline CI (`.so` su Android via
`cargo-ndk`, `.dll` su Windows, caricata a runtime tramite JNA).

Verificato in locale prima di essere consegnato: build e test dell'intero workspace,
generazione reale dei binding Kotlin dal `.so` compilato, ispezione del Kotlin generato.
**Non verificabile in locale** (richiede i runner reali di GitHub Actions): la
cross-compilazione per Android tramite NDK e l'intera build Gradle — è la parte che
osserveremo insieme nei log della prima Action.

Non ancora presente:

- `raw-decode` — collegamento a LibRaw per leggere i file RAW veri. `libraw-dev` è stato
  verificato disponibile e compilabile su Linux, ma la cross-compilazione della libreria
  C++ di LibRaw per le ABI Android (via NDK) è un problema a sé, non ancora affrontato:
  è il prossimo incremento naturale una volta che questa pipeline UniFFI si sarà dimostrata
  funzionante su un vero APK/EXE.
- `cache`, `catalog`, `job-scheduler` — non bloccanti per la prima demo end-to-end.

## Comandi

```bash
cd engine
cargo build --workspace   # compila tutti i crate
cargo test --workspace    # esegue tutti i test (inclusa la validazione degli shader)

# Genera i binding Kotlin (stesso comando usato dalla CI):
cargo build -p rawforge-ffi
cargo run --bin uniffi-bindgen -- generate --library target/debug/librawforge_ffi.so \
  --language kotlin --out-dir /tmp/kotlin-bindings
```
