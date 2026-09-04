# RawForge — Architettura di un RAW Processor Ultra-Veloce (Windows + Android)

**Documento**: Architecture & Implementation Guide
**Target**: Applicazione desktop (Windows) + mobile (Android), alternativa a Lightroom orientata a velocità e batch professionale.
**Autore**: Principal Architecture Review — v1.0

---

## 0. Principi guida (non negoziabili)

Prima dello stack, fissiamo i vincoli che guidano ogni scelta successiva:

1. **Zero-cost abstraction ovunque sia possibile**: il motore non deve mai pagare il prezzo di un garbage collector, di una VM, o di un binding "chatty" tra UI e core sul percorso critico (decode → preview → render).
2. **Un solo motore, N binding**: la logica di imaging si scrive **una volta** in linguaggio nativo compilato; tutto ciò che è specifico di piattaforma (UI, lifecycle, permessi, storage) resta fuori dal motore.
3. **La UI non tocca mai un pixel**: la UI orchestran e visualizza; il calcolo pesante (decode RAW, color science, GPU compute) vive interamente nel core nativo, disaccoppiato da qualunque event loop di UI.
4. **Non distruttivo by design**: nessuna scrittura mai sul file RAW originale. Ogni modifica è un'operazione registrata in un sidecar (JSON nativo + export/import `.xmp`).
5. **Il batch è un cittadino di prima classe**, non un ciclo `for` sopra il flusso single-image. La pipeline è pensata fin dal giorno 1 per elaborare N immagini in parallelo con scheduling GPU/CPU condiviso.

---

## 1. Stack Tecnologico

### 1.1 Decisione architetturale

| Livello | Scelta | Motivazione |
|---|---|---|
| **Core Engine** | **Rust** (workspace multi-crate) | Memory safety senza GC, performance pari a C++, ecosistema FFI maturo, cross-compilazione nativa verso `aarch64-linux-android` e `x86_64-pc-windows-msvc` dallo stesso codebase. |
| **Decodifica RAW** | **LibRaw** (C++) via binding FFI sottile in un crate isolato, con **rawler** (Rust puro) come motore alternativo/fallback | LibRaw resta lo standard de-facto per copertura sensori/CFA/white-balance-as-shot su centinaia di fotocamere; isolarlo in un crate `unsafe`-contained limita il rischio. `rawler` (100% Rust, no unsafe C) è tenuto come seconda via per i formati più comuni, utile anche per audit di sicurezza e per Android dove linkare LGPL dinamicamente è più scomodo. |
| **GPU Compute** | **wgpu** (Rust) → Vulkan su Android e Windows (fallback D3D12/Metal se necessario) | Un solo shader language (WGSL) compilato/tradotto per entrambe le piattaforme, API moderna esplicita (command buffers, bind groups), nessun overhead di astrazioni legacy tipo OpenGL ES. |
| **SIMD fallback CPU** | `std::arch` intrinsics (AVX2/FMA su x86_64, NEON su ARM64) incapsulati dietro il crate **`pulp`** (dispatch runtime sicuro) | Necessario per: anteprime a bassa risoluzione dove il setup GPU non conviene, dispositivi Android low-end senza Vulkan 1.1 completo, ed elaborazione di metadati/istogrammi dove il compute GPU sarebbe overkill. |
| **Binding cross-linguaggio** | **UniFFI** (Mozilla) | Genera automaticamente binding Kotlin (Android + Desktop JVM/Native) da un'unica interfaccia dichiarata in Rust (UDL o proc-macro). Elimina JNI scritto a mano, gestisce marshalling di tipi complessi (struct, enum, callback asincroni) in modo type-safe. |
| **UI Layer** | **Kotlin Multiplatform + Compose Multiplatform** | Compose *è* già Jetpack Compose su Android (zero astrazioni aggiuntive, piena integrazione con lifecycle/permessi/storage Android nativo) e Compose for Desktop (Skia-based, GPU-accelerated) copre Windows con lo stesso linguaggio. Il codice di stato/business-logic (ViewModel, orchestrazione job, cache manager) è condiviso in Kotlin puro (`commonMain`), non solo la UI. |
| **Rendering canvas "Develop"** | Texture off-screen renderizzata da wgpu, esposta alla UI come superficie nativa (non bitmap-copy quando evitabile) | Su Android: `SurfaceTexture`/`AHardwareBuffer` condivisa con Vulkan. Su Windows: swapchain Vulkan/D3D12 interop montato in un `SwapchainPanel`/finestra nativa embeddata nel layout Compose. Il fallback (dispositivi che non supportano interop) è un blit GPU→`ImageBitmap` a ogni frame, comunque più economico di un round-trip CPU. |

### 1.2 Perché non Flutter

Flutter resta un'alternativa valida (parità desktop/mobile oggi molto matura, community enorme), ed è la scelta corretta se il team ha già competenze Dart/Flutter consolidate. La preferiamo comunque a KMP+Compose per due ragioni specifiche a questo dominio:

- **Interop nativo con Rust**: UniFFI genera binding Kotlin/Swift/Python "di prima classe"; per Dart si passa quasi sempre da `dart:ffi` scritto a mano o da generatori meno maturi (es. `flutter_rust_bridge`, ottimo ma non ufficiale/Mozilla-maintained).
- **Superficie GPU nativa Android**: Compose, essendo Jetpack Compose, ha accesso diretto e documentato a `SurfaceView`/`AndroidExternalSurface` per montare contenuti Vulkan renderizzati esternamente: esattamente il pattern richiesto dal canvas di sviluppo a bassissima latenza. Su Flutter questo richiede plugin platform-channel custom con più superficie di attrito.

Se il team ha già forte expertise Flutter/Dart, lo stack alternativo (**Rust core + flutter_rust_bridge + Flutter UI**) è architetturalmente equivalente per tutto ciò che riguarda il motore descritto in questo documento: le sezioni 2-6 non cambiano, cambia solo il layer 1.1 "UI Layer"/"Binding".

### 1.3 Diagramma di alto livello

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         PRESENTATION LAYER (Kotlin)                       │
│  commonMain: ViewModel, AppState (MVI), JobOrchestrator, CacheFacade      │
│  androidMain: Activity, SurfaceTexture bridge, MediaStore, permissions   │
│  desktopMain (Windows): Compose Desktop window, Win32 file dialogs       │
└───────────────────────────────┬────────────────────────────────────────┘
                                 │  UniFFI-generated Kotlin bindings
                                 │  (async, callback-based, zero-copy buffers)
┌───────────────────────────────▼────────────────────────────────────────┐
│                        ENGINE CORE (Rust workspace)                      │
│                                                                          │
│  ┌────────────┐ ┌───────────┐ ┌───────────┐ ┌────────────┐ ┌──────────┐ │
│  │ raw-decode │ │  gpu-pipe │ │  color    │ │  harmonic  │ │smartbatch│ │
│  │ (LibRaw/   │ │  (wgpu/   │ │  science  │ │  (palette/ │ │ (scene   │ │
│  │  rawler)   │ │  WGSL)    │ │           │ │  curve fit)│ │ analysis)│ │
│  └─────┬──────┘ └─────┬─────┘ └─────┬─────┘ └─────┬──────┘ └────┬─────┘ │
│        │              │             │             │             │       │
│  ┌─────▼──────────────▼─────────────▼─────────────▼─────────────▼────┐ │
│  │                     job-scheduler (rayon + tokio)                  │ │
│  │            (batch DAG, priority queue, GPU cmd batching)           │ │
│  └─────┬────────────────────────────────────────────────────────┬────┘ │
│  ┌─────▼─────┐ ┌────────────┐ ┌────────────┐ ┌──────────────┐ ┌─▼────┐ │
│  │  cache    │ │  metadata  │ │    xmp     │ │   catalog    │ │ ffi  │ │
│  │ (tiled LRU│ │ (sidecar   │ │ (LR export)│ │ (SQLite idx) │ │(uniffi)│
│  │  disk+mem)│ │  JSON/XMP) │ │            │ │              │ └──────┘ │
│  └───────────┘ └────────────┘ └────────────┘ └──────────────┘          │
└──────────────────────────────────────────────────────────────────────────┘
                                 │
                    ┌────────────┴────────────┐
                    ▼                         ▼
              Vulkan (Android)          Vulkan/D3D12 (Windows)
```

---

## 2. Caricamento RAW, Decodifica e Gestione della Cache

### 2.1 Pipeline di ingestione

1. **Scan della cartella/rullino**: enumerazione file (async, I/O thread pool separato da quello di calcolo) → estrazione JPEG embedded (EXIF thumbnail, quasi istantaneo, 0 decode RAW) → popolamento immediato della grid view.
2. **Decode "preview" a bassa priorità**: per ogni immagine visibile (+ margine di prefetch, es. ±20 celle oltre il viewport), decodifica LibRaw a **half/quarter resolution** (parametro `libraw::half_size` o scaling nel demosaic) → tile GPU-resident.
3. **Decode "full" on-demand**: solo quando l'utente entra nel modulo Develop su una singola immagine, o quando parte un export/batch-render, si esegue la decodifica a piena risoluzione.
4. **Ogni fase è cancellabile**: se l'utente scorre velocemente la grid, i job di decode preview non ancora iniziati vengono droppati dalla coda (priorità basata su "distanza dal viewport corrente", ricalcolata ad ogni scroll event con debounce ~16ms).

### 2.2 Architettura di cache a livelli

| Livello | Contenuto | Storage | Scope |
|---|---|---|---|
| **L0** | JPEG embedded EXIF | Memoria (decodificato lazy) | Istantaneo, sempre disponibile |
| **L1** | Tile GPU-resident (texture wgpu) | VRAM, budget dinamico da `adapter.limits()` | Sessione corrente, LRU per tile |
| **L2** | Proxy compresso (half-res, 16-bit lineare o JPEG-XL) | Disco, cartella cache per-catalogo | Persistente tra sessioni |
| **L3** | RAW originale | Disco (immutabile) | Fonte di verità, mai modificato |

L'invalidazione della cache L2/L1 è basata su **hash di contenuto**: `hash(raw_file_bytes_header) + hash(edit_operations_json)`. Se l'utente cambia un solo parametro, solo i tile dipendenti da quello stage della pipeline vengono invalidati (vedi §3.3, DAG a stage).

### 2.3 Differenze Desktop vs Mobile

**Windows (Desktop)**:
- Budget di cache L1 calcolato da VRAM totale meno un margine di sicurezza (query `wgpu::Adapter` + fallback a stima euristica se l'API non espone il totale, es. via DXGI su Direct3D interop).
- Decode multi-thread aggressivo: pool dimensionato su `num_cpus::get_physical()`, con affinità NUMA-aware su workstation multi-socket (opzionale, fase 4).
- Cache L2 su disco può essere generosa (decine di GB), storage tipicamente NVMe → il costo di un cache-miss è basso.

**Android (Mobile)**:
- Budget di memoria letto da `ActivityManager.getMemoryInfo()` via binding Kotlin, passato al core Rust come parametro di configurazione a runtime (il motore non assume mai una quantità fissa di RAM).
- **Tiling obbligatorio**: mai decodificare un frame intero a piena risoluzione se non richiesto esplicitamente (export). Il canvas Develop richiede solo i tile visibili alla risoluzione dello schermo corrente (mip-map style, simile a Lightroom Mobile).
- Throttling termico: il job-scheduler monitora (via callback Kotlin→Rust) lo stato termico riportato da Android (`PowerManager.getThermalStatus`) e riduce dinamicamente il grado di parallelismo GPU/CPU per evitare frame drop da throttling.
- Cache L2 su storage interno con quota configurabile dall'utente (default 2GB), eviction LRU per-catalogo.

---

## 3. Pipeline di Elaborazione Immagine (Non-Distruttiva)

### 3.1 Modello dei dati: Edit come lista di operazioni

Ogni immagine ha un **sidecar JSON** (fonte di verità primaria, non l'XMP — l'XMP è un formato di **interscambio**, non lo storage nativo):

```json
{
  "schema_version": 3,
  "source_file": "IMG_0421.CR3",
  "source_hash": "blake3:8f2a...",
  "created_at": "2026-09-04T10:12:00Z",
  "history": [
    { "op": "white_balance", "params": { "temp": 5200, "tint": 4 } },
    { "op": "exposure", "params": { "ev": 0.35 } },
    { "op": "tone_curve", "params": { "points": [[0,0],[64,58],[128,130],[192,205],[255,255]] } },
    { "op": "hsl", "params": { "hue": [0,0,0,0,0,0,0,0], "sat": [0,5,-10,0,0,0,0,0], "lum": [0,0,0,0,0,0,0,0] } },
    { "op": "split_toning", "params": { "shadow_hue": 210, "shadow_sat": 8, "highlight_hue": 45, "highlight_sat": 5, "balance": 0 } }
  ],
  "harmonic_look_ref": "looks/cinematic_teal_orange.json",
  "smart_batch_deltas": { "exposure_ev": -0.12, "wb_temp_delta": -80, "reason": "scene=backlit, clipped_highlight_frac=0.18" }
}
```

Questo modello permette: **undo/redo infinito** (è uno stack di operazioni), **riproducibilità esatta** (rigenerare l'immagine da zero è deterministico), e **diff/merge tra versioni** per la sincronizzazione cross-device (fase 2).

### 3.2 Stage della pipeline GPU (per immagine, in spazio colore lineare)

```
RAW bytes (CPU, LibRaw)
   │  demosaic + black/white-level normalization
   ▼
Sensor-linear RGB (16f, camera color space)  ──┐
   │  camera→working-space 3x3 (o DCP profile) │  ogni stage è un
   ▼                                            │  compute pass wgpu
White Balance (matrice diagonale scalata)      │  separato → cache-
   ▼                                            │  abile e ricompo-
Exposure (guadagno scalare)                     │  nibile indipenden-
   ▼                                            │  temente (solo lo
Tone Curve (parametrica + curva utente, LUT 1D) │  stage modificato
   ▼                                            │  viene ricalcolato)
HSL / Color Grading (8 bande di hue)           │
   ▼                                            │
Detail: sharpening / NR (fase 3-4, wavelet)    │
   ▼                                            │
Output Transform (working space → sRGB/P3,     │
   tone-mapping percettivo per preview)  ───────┘
   ▼
Preview surface / Export buffer
```

Ogni stage è un **compute shader indipendente** con input/output su texture intermedie (`wgpu::TextureView` in formato `Rgba16Float`). Il DAG di dipendenza tra stage permette di **invalidare e ricalcolare solo dal primo stage modificato in poi** — se l'utente cambia solo la Tone Curve, White Balance ed Exposure non vengono ricalcolati, si riparte dalla texture cache-ata post-Exposure.

### 3.3 Import/Export XMP

- **Import**: se l'utente importa foto già editate in Lightroom (file `.xmp` sidecar presenti), il modulo `xmp` parsa i campi `crs:*` noti e li traduce in `history` operations equivalenti (mapping inverso di quello descritto in §5).
- **Export**: (a) sidecar XMP standard accanto al RAW per interoperabilità con altri tool Adobe-compatibili; (b) **Lightroom Preset** esportabile come `.xmp` standalone da installare nella cartella Develop Presets di Lightroom — è la funzionalità "Sintesi Armonica → Preset LR" richiesta, dettagliata in §5.

---

## 4. Algoritmi Core

### 4.1 Sintesi Armonica Automatica (Reference-based Color Matching)

**Obiettivo**: da un'immagine di riferimento (look cinematografico o foto guida), estrarre un set di parametri "Look" applicabile a un intero set di foto.

**Algoritmo** (crate `harmonic`):

1. **Downsample di analisi**: riduci l'immagine di riferimento a 512px sul lato lungo (bicubic), lavora sempre in questa risoluzione per l'analisi statistica — il costo dell'estrazione deve essere trascurabile (<50ms).
2. **Conversione a Lab** (o IPT per migliore separazione hue/luminanza percettiva).
3. **Estrazione tone curve**:
   - Calcola l'istogramma cumulativo (CDF) del canale L.
   - Campiona la CDF ai percentili {5, 25, 50, 75, 95} → questi diventano i control point di una curva monotona (spline cubica di Hermite, garantita monotona con il metodo di Fritsch-Carlson) rispetto a una CDF "neutra" di riferimento (immagine linear/flat ipotetica).
   - Il risultato è `tone_curve.points[]`, direttamente compatibile con `crs:ToneCurvePV2012`.
4. **Estrazione palette / color grading**:
   - Segmenta i pixel in 3 bucket di luminanza (ombre L<33, mezzitoni 33-66, luci >66).
   - Per ciascun bucket, esegui **k-means in a\*b\*** (k=3), pesato per popolazione del bucket → hue/chroma dominante per zona tonale.
   - Mappa il bucket ombre → `split_toning.shadow_{hue,sat}`, bucket luci → `split_toning.highlight_{hue,sat}`.
   - Il bucket mezzitoni contribuisce a un bias di saturazione/vibrance globale.
5. **Estrazione contrasto**: deviazione standard e IQR (interquartile range) del canale L → normalizzati contro un range "standard" empirico determinano `contrast` e (fase successiva) `clarity` (energia high-pass locale via un semplice filtro Laplaciano a bassa risoluzione).
6. **Estrazione white balance bias**: stima illuminante via gray-world (media R/G/B pesata) o, se disponibile, tramite patch neutra rilevata automaticamente (pixel a bassa saturazione e luminanza medio-alta) → confronto con illuminante standard D65 produce `wb_shift {temp_delta, tint_delta}`.
7. **Output**: struct `HarmonicLook` (vedi §5.1) serializzabile sia come sidecar interno che come `.xmp` preset.
8. **Applicazione al set**: il Look può essere applicato (a) come **preset assoluto** identico su tutte le foto, oppure (b) passato come "target" allo Smart-Batch Contestuale (§4.2), che lo adatta per-immagine.

### 4.2 Smart-Batch Contestuale (Adattamento Dinamico per-Immagine)

**Obiettivo**: applicare un Look (manuale o da Sintesi Armonica) su centinaia di scatti mantenendo resa omogenea, senza che scene molto diverse (controluce, notturno, ritratto) risultino sovra/sotto-corrette.

**Algoritmo** (crate `smartbatch`, eseguito in parallelo su tutte le immagini del batch prima del render):

1. **Descrittori per-immagine** (calcolati su preview a bassa risoluzione, CPU rayon-parallel):
   - Istogramma luminanza a 256 bin.
   - `clipped_highlight_frac` = frazione pixel con L > 250/255.
   - `crushed_shadow_frac` = frazione pixel con L < 5/255.
   - `mean_luminance`, `luminance_std` (proxy di dynamic range/contrasto della scena).
   - Stima illuminante (gray-world) → `estimated_temp`.
   - **Classificazione scena** (MVP: euristiche; fase 3+: modello leggero tipo MobileNetV3-Small/EfficientNet-Lite distillato, <5MB, quantizzato INT8, inferenza on-device via `tract` — inferenza ONNX pura Rust, coerente con lo stack): classi {ritratto, paesaggio, controluce/high-key, notturno/low-key, architettura/interni, macro}.
   - Se `ritratto`: attiva rilevamento volti leggero (facoltativo, modello tipo BlazeFace) per proteggere la banda di hue delle pelli (evita che color-grading globale sposti gli incarnati).
2. **Calcolo del delta adattivo** rispetto al Look target:
   ```
   exposure_delta   = clamp(k_e * (target.mean_lum - image.mean_lum), -EV_MAX, +EV_MAX)
   highlights_delta = target.highlights - k_h * image.clipped_highlight_frac * 100
   shadows_delta    = target.shadows    + k_s * image.crushed_shadow_frac   * 100
   wb_temp_delta    = lerp(target.wb_temp_delta, image.gray_world_delta, 1 - scene_confidence)
   contrast_delta   = target.contrast * (reference.luminance_std / image.luminance_std)
   ```
   dove `k_e, k_h, k_s` sono costanti calibrate empiricamente (esposte come slider avanzati "aggressività adattamento").
3. **Guardrail**: ogni delta è clampato entro una **deviazione massima configurabile** dal Look di base (default ±0.5 EV, ±15 punti highlights/shadows) — l'obiettivo è coerenza, non "auto-tutto" incontrollato. Uno slider "Override Strength" (0-100%) nella UI interpola linearmente tra "applica il Look letterale" e "applica il delta massimo consentito".
4. **Scheduling**: i descrittori di tutte le N immagini sono calcolati in parallelo (rayon `par_iter`); i job di rendering GPU vengono poi accodati con **command batching** — invece di N submit separati con setup/teardown pipeline ripetuto, il `job-scheduler` raggruppa i render in batch di command buffer condivisi, ammortizzando l'overhead di pipeline binding su GPU (fondamentale per centinaia di immagini).

---

## 5. Export Preset Lightroom (`.xmp`)

### 5.1 Struct sorgente (Rust)

```rust
pub struct HarmonicLook {
    pub name: String,
    pub process_version: String,      // es. "15.4"
    pub white_balance: WhiteBalance,   // { temp: u32, tint: i32 }
    pub exposure_ev: f32,
    pub contrast: i32,                 // -100..100
    pub highlights: i32,
    pub shadows: i32,
    pub whites: i32,
    pub blacks: i32,
    pub vibrance: i32,
    pub saturation: i32,
    pub tone_curve: Vec<(u8, u8)>,      // control points 0-255
    pub hsl: HslAdjustments,            // 8 hues x {hue, sat, lum}
    pub split_toning: SplitToning,      // shadow/highlight hue+sat, balance
}
```

### 5.2 Mapping ai campi Adobe Camera Raw (`crs:` namespace)

| Campo interno | Campo XMP (`crs:`) | Note |
|---|---|---|
| `white_balance.temp/tint` | `WhiteBalance="Custom"`, `Temperature`, `Tint` | |
| `exposure_ev` | `Exposure2012` | Process Version 2012+ |
| `contrast` | `Contrast2012` | |
| `highlights` | `Highlights2012` | |
| `shadows` | `Shadows2012` | |
| `whites` | `Whites2012` | |
| `blacks` | `Blacks2012` | |
| `vibrance` / `saturation` | `Vibrance` / `Saturation` | |
| `tone_curve` | `ToneCurvePV2012` (+ `ToneCurvePV2012Red/Green/Blue` se per-canale) | Serializzato come `rdf:Seq` di stringhe `"x, y"` |
| `hsl.hue[8]` | `HueAdjustmentRed`…`HueAdjustmentMagenta` | 8 campi |
| `hsl.sat[8]` | `SaturationAdjustmentRed`…`Magenta` | 8 campi |
| `hsl.lum[8]` | `LuminanceAdjustmentRed`…`Magenta` | 8 campi |
| `split_toning.*` | `SplitToningShadowHue/Saturation`, `SplitToningHighlightHue/Saturation`, `SplitToningBalance` | Legacy panel; per il pannello "Color Grading" moderno si aggiungono in parallelo `ColorGradeMidtoneHue/Sat/Lum`, `ColorGradeShadowLum`, `ColorGradeHighlightLum`, `ColorGradeBlending`, `ColorGradeGlobalHue/Sat/Lum` per compatibilità con ACR/LR recenti. |

### 5.3 Funzione di generazione (Rust)

```rust
use std::fmt::Write;

pub fn generate_lightroom_xmp(look: &HarmonicLook) -> String {
    let mut curve = String::new();
    for (x, y) in &look.tone_curve {
        let _ = write!(curve, "<rdf:li>{}, {}</rdf:li>", x, y);
    }

    let hue_names = ["Red","Orange","Yellow","Green","Aqua","Blue","Purple","Magenta"];
    let mut hsl_fields = String::new();
    for (i, name) in hue_names.iter().enumerate() {
        let _ = write!(hsl_fields,
            "crs:HueAdjustment{n}=\"{h}\" crs:SaturationAdjustment{n}=\"{s}\" crs:LuminanceAdjustment{n}=\"{l}\"\n            ",
            n = name, h = look.hsl.hue[i], s = look.hsl.sat[i], l = look.hsl.lum[i]);
    }

    format!(
r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="RawForge 1.0">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:PresetType="Normal"
        crs:Version="17.0"
        crs:ProcessVersion="{pv}"
        crs:WhiteBalance="Custom"
        crs:Temperature="{temp}"
        crs:Tint="{tint}"
        crs:Exposure2012="{exposure:.2}"
        crs:Contrast2012="{contrast}"
        crs:Highlights2012="{highlights}"
        crs:Shadows2012="{shadows}"
        crs:Whites2012="{whites}"
        crs:Blacks2012="{blacks}"
        crs:Vibrance="{vibrance}"
        crs:Saturation="{saturation}"
        {hsl_fields}crs:SplitToningShadowHue="{sh_hue}"
        crs:SplitToningShadowSaturation="{sh_sat}"
        crs:SplitToningHighlightHue="{hl_hue}"
        crs:SplitToningHighlightSaturation="{hl_sat}"
        crs:SplitToningBalance="{balance}"
        crs:HasSettings="True">
      <crs:Name>
        <rdf:Alt><rdf:li xml:lang="x-default">{name}</rdf:li></rdf:Alt>
      </crs:Name>
      <crs:ToneCurvePV2012>
        <rdf:Seq>{curve}</rdf:Seq>
      </crs:ToneCurvePV2012>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#,
        pv = look.process_version,
        temp = look.white_balance.temp,
        tint = look.white_balance.tint,
        exposure = look.exposure_ev,
        contrast = look.contrast,
        highlights = look.highlights,
        shadows = look.shadows,
        whites = look.whites,
        blacks = look.blacks,
        vibrance = look.vibrance,
        saturation = look.saturation,
        hsl_fields = hsl_fields,
        sh_hue = look.split_toning.shadow_hue,
        sh_sat = look.split_toning.shadow_sat,
        hl_hue = look.split_toning.highlight_hue,
        hl_sat = look.split_toning.highlight_sat,
        balance = look.split_toning.balance,
        name = look.name,
        curve = curve,
    )
}
```

Il file risultante, salvato con estensione `.xmp` nella cartella `Lightroom/Develop Presets/RawForge/`, viene riconosciuto da Lightroom Classic/CC come preset importabile — nessuna conversione lato utente richiesta.

---

## 6. Esempi di Codice

### 6.1 Pipeline parallela multi-thread su buffer di pixel (Rust + rayon)

Esempio di applicazione combinata White Balance + Exposure + Tone Curve su un buffer `Rgba16Float`, parallelizzato per righe (chunk-based, cache-friendly, vettorizzabile dal compilatore):

```rust
use rayon::prelude::*;

#[derive(Clone, Copy)]
pub struct PixelOpsParams {
    pub wb_gain: [f32; 3],      // guadagni R/G/B da white balance
    pub exposure_mul: f32,      // 2^ev
    pub curve_lut: [f32; 256],  // tone curve pre-campionata come LUT
}

/// Applica WB + Exposure + Tone Curve in-place su un buffer RGBA f32 planare a interleaved.
/// `buffer` ha lunghezza width*height*4 (RGBA).
pub fn apply_pixel_ops_parallel(buffer: &mut [f32], width: usize, params: &PixelOpsParams) {
    // Parallelizza per righe: ogni riga è un chunk indipendente, nessuna
    // dipendenza tra iterazioni -> nessuna sincronizzazione necessaria.
    buffer
        .par_chunks_mut(width * 4)
        .for_each(|row| {
            // Loop interno semplice e branch-free: il compilatore (LLVM)
            // può auto-vettorizzare con AVX2/NEON senza intrinsics espliciti,
            // perché non ci sono early-return né allocazioni.
            for px in row.chunks_exact_mut(4) {
                let r = (px[0] * params.wb_gain[0] * params.exposure_mul).clamp(0.0, 1.0);
                let g = (px[1] * params.wb_gain[1] * params.exposure_mul).clamp(0.0, 1.0);
                let b = (px[2] * params.wb_gain[2] * params.exposure_mul).clamp(0.0, 1.0);

                px[0] = sample_lut(&params.curve_lut, r);
                px[1] = sample_lut(&params.curve_lut, g);
                px[2] = sample_lut(&params.curve_lut, b);
                // px[3] = alpha, invariato
            }
        });
}

#[inline(always)]
fn sample_lut(lut: &[f32; 256], v: f32) -> f32 {
    // Interpolazione lineare tra i due campioni LUT più vicini.
    let scaled = v * 255.0;
    let idx = scaled as usize;
    let frac = scaled - idx as f32;
    let a = lut[idx.min(255)];
    let b = lut[(idx + 1).min(255)];
    a + (b - a) * frac
}
```

Per il batch di N immagini, lo stesso pattern si applica a un livello superiore: `images.par_iter_mut().for_each(|img| apply_pixel_ops_parallel(...))`, dove ogni immagine gira su un thread del pool rayon condiviso, mentre la GPU processa in parallelo gli stage successivi (double-buffering CPU/GPU).

### 6.2 Compute Shader WGSL — Color Grading (WB + Exposure + HSL semplificato)

```wgsl
// color_grade.wgsl — eseguito su una texture Rgba16Float, un thread per pixel.

struct GradeParams {
    wb_gain: vec3<f32>,
    exposure_mul: f32,
    contrast: f32,
    saturation: f32,
    // hue-band adjustments compattati: 8 bande x (hue_shift, sat_mul, lum_shift)
    hsl_hue_shift: array<f32, 8>,
    hsl_sat_mul:   array<f32, 8>,
    hsl_lum_shift: array<f32, 8>,
};

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> params: GradeParams;

fn rgb_to_hsl(c: vec3<f32>) -> vec3<f32> {
    let maxc = max(c.r, max(c.g, c.b));
    let minc = min(c.r, min(c.g, c.b));
    let l = (maxc + minc) * 0.5;
    var h = 0.0;
    var s = 0.0;
    let d = maxc - minc;
    if (d > 0.00001) {
        s = d / (1.0 - abs(2.0 * l - 1.0) + 0.00001);
        if (maxc == c.r) { h = ((c.g - c.b) / d) % 6.0; }
        else if (maxc == c.g) { h = (c.b - c.r) / d + 2.0; }
        else { h = (c.r - c.g) / d + 4.0; }
        h = h * 60.0;
        if (h < 0.0) { h = h + 360.0; }
    }
    return vec3<f32>(h, s, l);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let h = hsl.x; let s = hsl.y; let l = hsl.z;
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let x = c * (1.0 - abs(((h / 60.0) % 2.0) - 1.0));
    let m = l - c * 0.5;
    var rgb = vec3<f32>(0.0);
    if (h < 60.0)       { rgb = vec3<f32>(c, x, 0.0); }
    else if (h < 120.0) { rgb = vec3<f32>(x, c, 0.0); }
    else if (h < 180.0) { rgb = vec3<f32>(0.0, c, x); }
    else if (h < 240.0) { rgb = vec3<f32>(0.0, x, c); }
    else if (h < 300.0) { rgb = vec3<f32>(x, 0.0, c); }
    else                { rgb = vec3<f32>(c, 0.0, x); }
    return rgb + vec3<f32>(m);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));

    var color = textureLoad(input_tex, coord, 0).rgb;

    // White balance + exposure (lineare, prima della conversione HSL)
    color = color * params.wb_gain * params.exposure_mul;

    // Contrast attorno al pivot 0.5 (in spazio percettivo semplificato)
    color = (color - vec3<f32>(0.5)) * params.contrast + vec3<f32>(0.5);

    // HSL per-banda: selezione soft della banda tramite hue corrente
    var hsl = rgb_to_hsl(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)));
    let band = i32(hsl.x / 45.0) % 8; // 8 bande da 45 gradi (Red..Magenta)
    hsl.x = hsl.x + params.hsl_hue_shift[band];
    hsl.y = clamp(hsl.y * params.hsl_sat_mul[band] * params.saturation, 0.0, 1.0);
    hsl.z = clamp(hsl.z + params.hsl_lum_shift[band], 0.0, 1.0);

    let out = hsl_to_rgb(hsl);
    textureStore(output_tex, coord, vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}
```

Dispatch dal lato Rust (host):

```rust
let workgroups_x = (width + 7) / 8;
let workgroups_y = (height + 7) / 8;
compute_pass.set_pipeline(&color_grade_pipeline);
compute_pass.set_bind_group(0, &bind_group, &[]);
compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
```

Per il batch, N immagini con parametri diversi condividono la stessa pipeline compilata (pipeline binding immutabile) — cambia solo il `uniform buffer` con i `GradeParams` per immagine, permettendo di accodare centinaia di dispatch in un singolo `CommandEncoder` prima di un solo `queue.submit()`.

---

## 7. Struttura del Progetto

```
rawforge/
├── Cargo.toml                       # workspace root
├── engine/
│   ├── raw-decode/                  # FFI LibRaw + wrapper rawler (fallback)
│   │   ├── src/lib.rs
│   │   ├── src/libraw_ffi.rs
│   │   └── build.rs                 # link statico/dinamico LibRaw
│   ├── gpu-pipe/                    # wgpu device/queue mgmt, DAG di stage
│   │   ├── src/lib.rs
│   │   ├── src/stage.rs             # trait PipelineStage + implementazioni
│   │   └── shaders/
│   │       ├── white_balance.wgsl
│   │       ├── tone_curve.wgsl
│   │       ├── color_grade.wgsl
│   │       └── output_transform.wgsl
│   ├── color-science/               # spazi colore, matrici camera, ICC/DCP
│   ├── harmonic/                    # Sintesi Armonica (palette/curve/contrast)
│   ├── smartbatch/                  # scene analysis + adattamento contestuale
│   │   └── models/                  # modelli ONNX quantizzati (scene classifier)
│   ├── cache/                       # tiled LRU, disk cache, content-hash invalidation
│   ├── metadata/                    # sidecar JSON schema + versioning/migrazioni
│   ├── xmp/                         # import/export XMP + preset LR
│   ├── catalog/                     # SQLite (rusqlite) indicizzazione libreria
│   ├── job-scheduler/               # rayon + tokio, batch DAG, GPU cmd batching
│   └── ffi/                         # UniFFI scaffolding (.udl o proc-macro)
│       └── rawforge.udl
├── ui/
│   ├── settings.gradle.kts
│   ├── shared/                      # KMP commonMain: state, ViewModel, facade su ffi
│   │   ├── commonMain/kotlin/
│   │   ├── androidMain/kotlin/      # Activity, SurfaceTexture bridge, MediaStore
│   │   └── desktopMain/kotlin/      # Compose Desktop window, Win32 file dialogs
│   ├── androidApp/                  # entry point Android
│   └── desktopApp/                  # entry point Windows (Compose for Desktop)
├── tools/
│   ├── xmp-fixtures/                # corpus di .xmp reali per test di regressione mapping
│   └── bench/                       # criterion benchmarks su decode/pipeline
└── docs/
    └── sidecar-schema/               # JSON Schema versionato per il formato .rfjson
```

---

## 8. Roadmap di Sviluppo

### Fase 1 — Core Engine & Desktop MVP (~3-4 mesi)
- Workspace Rust: `raw-decode` (LibRaw FFI) funzionante su ≥20 modelli fotocamera comuni.
- `gpu-pipe` con stage base: WB, Exposure, Tone Curve, Output Transform (no HSL ancora).
- Cache L1/L2 con invalidazione per content-hash.
- Sidecar JSON (schema v1) con undo/redo.
- UI Desktop minimale (Compose for Desktop): grid libreria, modulo Develop single-image, export JPEG/TIFF.
- Benchmark target: decode+preview di un file RAW 24-45MP in **< 150ms** su hardware desktop di fascia media; export batch a **≥ 8 immagini/secondo** su GPU discreta di fascia media.

### Fase 2 — Android Porting & Cross-Platform Sync (~2-3 mesi)
- Cross-compile engine per `aarch64-linux-android`, binding UniFFI Kotlin verificati su device reali (range low/mid/high-end).
- Tiling decode mobile, gestione memoria dinamica da `ActivityManager`, throttling termico.
- UI Android (Compose, stessa codebase `shared`/`commonMain` del desktop) con parità funzionale del modulo Develop.
- Sincronizzazione sidecar cross-device: strategia file-based (cartella condivisa/cloud provider) con merge a livello di `history` operations (conflict resolution: last-write-wins per campo, con log di conflitto visibile all'utente).

### Fase 3 — Smart-Batch, Harmonic Engine & Export Lightroom (~3 mesi)
- `harmonic`: estrazione palette/curve/contrast da immagine di riferimento, validata su set di test curati (confronto percettivo A/B).
- `smartbatch`: classificatore scena on-device (modello quantizzato <5MB), pipeline di adattamento contestuale con guardrail configurabili.
- `xmp`: generatore preset Lightroom completo (tone curve, HSL, split-toning/color-grade, WB) + import XMP esistenti.
- UI: pannello "Sintesi Armonica" (selezione reference + preview live su thumbnail del batch), slider "Override Strength" per lo Smart-Batch, pulsante "Esporta come preset Lightroom (.xmp)".
- Benchmark: applicazione Look + adattamento contestuale su batch di **500 immagini in < 60 secondi** (preview-res, pipeline GPU-batched).

### Fase 4 — Polishing & Ottimizzazione (~2 mesi)
- Sharpening/Noise Reduction GPU (wavelet o bilateral avanzato).
- Ottimizzazione SIMD dei path CPU residui (istogrammi, descrittori smart-batch) via `pulp`.
- Affinità NUMA su workstation multi-socket (fase opzionale desktop).
- Hardening memoria su Android low-end (profili di degradazione automatica: riduzione risoluzione preview sotto soglie di RAM critiche).
- Localizzazione, accessibilità, telemetria opt-in per crash/performance regression.
- Security/license audit del binding LibRaw (LGPL — verifica linking dinamico su entrambe le piattaforme, o valutazione licenza commerciale LibRaw se necessario per la distribuzione Android via linking statico).

---

## 9. Rischi Tecnici da Monitorare

- **Licenza LibRaw (LGPL 2.1 / CDDL)**: il linking statico su Android complica la conformità LGPL (richiede meccanismo di re-linking, es. shared object separato). Valutare `rawler` come default su Android e LibRaw come opzione desktop, oppure licenza commerciale LibRaw per la distribuzione mobile.
- **Interop GPU zero-copy UI↔Engine**: il livello di difficoltà varia molto per GPU/driver Android (frammentazione Vulkan 1.1 su device low-end). Prevedere sempre un fallback a blit CPU-mediato testato fin da subito, non solo come piano B tardivo.
- **Deriva del mapping XMP** rispetto a versioni future di Adobe Camera Raw (nuovi campi Process Version): mantenere un corpus di fixture `.xmp` reali esportate da Lightroom per test di regressione ad ogni release ACR.
