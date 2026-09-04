# RawForge Engine (Rust)

Workspace del motore nativo di RawForge, come descritto in `../docs/ARCHITECTURE.md`.

## Stato attuale

Crate reali, compilati e testati (39 test, tutti verdi):

| Crate | Cosa fa | Rif. architettura |
|---|---|---|
| `core-types` | Strutture dati condivise (`HarmonicLook` e affini) | §5.1 |
| `color-science` | Conversioni sRGB↔lineare, RGB↔Lab, RGB↔HSL | §3.2 |
| `harmonic` | Sintesi Armonica: estrae tone curve, palette (split toning), contrasto e WB da un'immagine di riferimento | §4.1 |
| `smartbatch` | Smart-Batch Contestuale: descrittori di scena da istogramma + calcolo dei delta adattivi con guardrail | §4.2 |
| `metadata` | Sidecar JSON non distruttivo (schema versionato, history di operazioni) | §3.1 |
| `xmp` | Generatore di preset Lightroom `.xmp` dal `HarmonicLook` | §5 |
| `gpu-pipe` | Sorgenti WGSL degli stage di color grading, validati con `naga` (nessuna GPU richiesta per i test) | §3.2, §6.2 |
| `raw-decode` | Decodifica RAW vera (`rawler`, Rust puro): anteprima incorporata dalla fotocamera + metadati base | §2, §9 |
| `ffi` | Superficie **UniFFI** che espone `harmonic`/`xmp`/`smartbatch`/`raw-decode` a Kotlin — è questo il crate che la pipeline CI compila per Android (via `cargo-ndk`) e Windows (nativo), generando anche i binding Kotlin usati da `shared/` | §1, §7 |

**Novità di questo giro**: il crate `raw-decode` collega per la prima volta un file RAW *vero* di
una fotocamera (CR2/CR3/NEF/ARW/RAF/RW2/DNG/...) al resto della pipeline. Scelta deliberata:
`rawler` (Rust puro) invece di LibRaw (C++) descritto nell'architettura originale, proprio per
evitare il problema di cross-compilare una libreria C++ per Android via NDK — `rawler` cross-
compila con `cargo-ndk` come ogni altro crate qui. Il dettaglio completo (perché, cosa NON fa
ancora, nota di licenza LGPL) è nel commento di testa di `raw-decode/src/lib.rs`.

`ffi` espone adesso, oltre alle funzioni già esistenti:

- `decode_raw_file_preview(bytes)` — anteprima + metadati da un file RAW vero.
- `extract_look_from_raw_reference(bytes, look_name)` — Sintesi Armonica direttamente da un file
  RAW, senza passare da una ri-codifica intermedia.
- `is_known_raw_file_name(file_name)` — riconoscimento rapido dell'estensione, usato dalla UI.

Verificato in locale prima di essere consegnato: build e test dell'intero workspace (inclusi i
percorsi di errore di `raw-decode` su input non validi/corrotti — nessun panic), generazione reale
dei binding Kotlin dal `.so` compilato, ispezione del Kotlin generato (nessuna collisione di nomi
come quella già risolta in un giro precedente — `RawFileError` usa `reason`, non `message`).
Confermato anche che `rawler` non ha `build.rs` e non compila codice C/C++.

**Non verificabile in locale** (richiede i runner reali di GitHub Actions, e questo ambiente non
può scaricare un NDK Android per policy di rete): la cross-compilazione di `raw-decode`/`rawler`
per Android tramite NDK, e la build Gradle completa con la nuova UI di importazione — è la parte
che osserveremo insieme nei log della prossima Action.

Non ancora presente:

- **Demosaic completo**: `raw-decode` estrae solo l'anteprima incorporata dalla fotocamera
  (istantanea, nessun calcolo pesante), non l'immagine RAW "sviluppata" pixel per pixel a piena
  risoluzione (§3.2). È il prossimo incremento naturale una volta verde questo giro in CI.
- `cache`, `catalog`, `job-scheduler` — non bloccanti per il flusso attuale a una foto per volta.

## Comandi

```bash
cd engine
cargo build --workspace   # compila tutti i crate
cargo test --workspace    # esegue tutti i test (inclusa la validazione degli shader e i test raw-decode)

# Genera i binding Kotlin (stesso comando usato dalla CI):
cargo build -p rawforge-ffi
cargo run --bin uniffi-bindgen -- generate --library target/debug/librawforge_ffi.so \
  --language kotlin --out-dir /tmp/kotlin-bindings
```
