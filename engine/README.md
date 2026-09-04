# RawForge Engine (Rust)

Workspace del motore nativo di RawForge, come descritto in `../docs/ARCHITECTURE.md`.

## Stato attuale

Crate reali, compilati e testati (48 test, tutti verdi):

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
| `look-render` | Applica un `HarmonicLook` ai pixel su CPU (esposizione, tone curve, contrasto, highlights/shadows, HSL, split toning) — l'anteprima "incolla impostazioni" | §3.2 |
| `ffi` | Superficie **UniFFI** che espone tutti i crate sopra a Kotlin — è questo il crate che la pipeline CI compila per Android (via `cargo-ndk`) e Windows (nativo), generando anche i binding Kotlin usati da `shared/` | §1, §7 |

**Novità di questo giro**: lo Smart-Batch Contestuale (`smartbatch`) era già scritto e testato ma
irraggiungibile dalla UI — l'unico modo di "usare" un Look era esportarlo come `.xmp`. Il nuovo
crate `look-render` chiude il cerchio: applica un `HarmonicLook` ai pixel di un'immagine su CPU
(niente GPU necessaria, quindi testabile in questo ambiente), e la nuova funzione FFI
`paste_look_onto_target_photo` mette in fila estrazione del Look dalla foto campione, calcolo dei
descrittori di scena di campione e target, calcolo dei delta adattivi (`smartbatch`), applicazione
del Look adattato e rendering — restituendo un'anteprima PNG pronta da mostrare in app.

Perché CPU e non `gpu-pipe` (già scritto con `wgpu`/WGSL): collegare `wgpu` a Kotlin via
UniFFI/JNA su entrambe le piattaforme è un lavoro sostanzialmente più grande, rimandato di
proposito — dettagli e semplificazioni dichiarate nel commento di testa di
`look-render/src/lib.rs` (in particolare: il bilanciamento del bianco assoluto non è applicato,
servirebbe un profilo colore camera che questo motore non ha ancora).

**Correzione di fedeltà in `HarmonicLookFfi`**: il tipo che attraversa il confine Rust↔Kotlin
portava originariamente solo 9 dei ~18 campi di un `HarmonicLook` (scelta della primissima demo).
highlights, shadows, whites, blacks, saturation, tone curve, HSL, il "balance" dello split-toning e
il tint del bilanciamento del bianco venivano quindi silenziosamente azzerati ad ogni giro
Kotlin→Rust→Kotlin — **compreso l'export `.xmp` già esistente prima di questo giro** (bug
pre-esistente, scoperto costruendo questa funzionalità, non introdotto da essa). Ora porta tutti i
campi; `TonePointFfi` sostituisce la tupla `(u8, u8)` (non rappresentabile da UniFFI), le bande
HSL passano come `Vec<i32>` invece di array fissi. Test dedicato:
`harmonic_look_ffi_round_trip_preserves_all_fields`.

**Corretto un secondo bug, questo segnalato dall'utente dopo un test reale**: "incolla
impostazioni" con una foto scattata ed editata in basso-chiave (molto asfalto/ombre scure) come
campione produceva un'anteprima innaturalmente scura e desaturata — nei log dell'app, "esposizione
-1.09 EV" applicata a una foto che avrebbe dovuto ricevere al più ±0.5 EV di correzione (il
guardrail di `smartbatch::AdaptationParams::max_exposure_delta_ev`, già testato). Causa reale,
confermata leggendo `harmonic::extract_look_from_reference`: `exposure_ev` vi è calcolato come uno
scostamento ASSOLUTO tra la mediana di luminanza della foto campione e un pivot di grigio neutro
(`NEUTRAL_L = 50.0`), clampato a ±2.0 EV — cioè "quanto è scura/chiara *quella specifica foto*", non
"quanta correzione di esposizione replicare su un'altra scena" (impossibile da sapere con certezza
da una sola immagine finale, senza l'originale non editato). Questo valore assoluto e senza freni
veniva sommato per intero al delta di Smart-Batch, che invece è correttamente guardrailato — quindi
anche a "intensità adattamento" 100% il risultato restava dominato dalla luminosità assoluta della
foto campione, non dall'adattamento contestuale. Due correzioni, entrambe testate:

1. `paste_look_onto_target_photo` ora interpola la componente assoluta di `exposure_ev` con
   `(1 - intensità_adattamento)` prima di sommare il delta di Smart-Batch: a intensità 0.0 resta "Look
   letterale" (comportamento invariato), a intensità 1.0 lascia il campo per intero al delta
   guardrailato (±0.5 EV di default), a valori intermedi interpola tra i due. Test dedicato,
   che riproduce lo scenario segnalato (campione scuro, stessa scena come target, intensità 100%):
   `paste_look_onto_target_photo_does_not_force_large_exposure_shift_when_target_matches_reference_scene`.
2. `harmonic::extract_look_from_reference` costruiva anche la tone curve dai percentili ASSOLUTI di
   luminanza del campione — stesso problema in forma diversa: una foto campione scura trascinava il
   midtone (128) di *qualsiasi* target verso il basso, sommandosi silenziosamente all'esposizione e
   aggravando lo scurimento anche dopo la correzione (1). Ora i punti di controllo sono calcolati
   RELATIVI alla mediana del campione stesso (il midpoint resta sempre ancorato a 128 = pivot
   neutro): la curva trasporta solo la *forma* del contrasto/roll-off ombre-luci, non la luminosità
   assoluta della scena campione. Test dedicati: `dark_reference_tone_curve_midpoint_stays_neutral`,
   `contrasty_reference_still_produces_asymmetric_curve_shape`.

Aggiunto anche un guardrail difensivo in `look-render`: il moltiplicatore globale di
saturazione/vibrance è ora clampato a un intervallo sicuro (`[0.35, 2.5]`) invece che solo
`.max(0.0)`, per evitare che una stima di vibrance molto negativa (es. da una foto campione con
ampie zone quasi neutre come asfalto o cielo uniforme) desaturi quasi del tutto il target.

`ffi` espone adesso, oltre alle funzioni già esistenti:

- `decode_raw_file_preview(bytes)` — anteprima + metadati da un file RAW vero.
- `extract_look_from_raw_reference(bytes, look_name)` — Sintesi Armonica direttamente da un file
  RAW, senza passare da una ri-codifica intermedia.
- `is_known_raw_file_name(file_name)` — riconoscimento rapido dell'estensione, usato dalla UI.
- `paste_look_onto_target_photo(sample_bytes, sample_file_name, look_name, target_bytes,
  target_file_name, override_strength)` — il nuovo flusso "incolla impostazioni" completo in una
  chiamata. Prende solo bytes/stringhe primitive (non un `HarmonicLookFfi`) apposta: così la UI
  Kotlin comune (`commonMain`) può richiamarlo senza dover far attraversare il confine
  `expect`/`actual` a un tipo generato da UniFFI, che esiste solo nelle copie platform-specific dei
  binding.

Verificato in locale prima di essere consegnato: build e test dell'intero workspace (48 test,
tutti verdi — inclusi i test aggiornati su `harmonic`/`ffi`/`look-render` che riproducono e
verificano la correzione del bug di esposizione/tone-curve descritto sopra), generazione reale dei
binding Kotlin dal `.so` compilato, ispezione del Kotlin generato (nessuna collisione di nomi come
quella già risolta in un giro precedente; `pasteLookOntoTargetPhoto` ha la firma attesa, solo tipi
primitivi — questo giro non ha toccato la superficie Kotlin, solo la logica Rust sotto, quindi i
binding non sono nemmeno cambiati di forma).

**Non verificabile in locale** (richiede i runner reali di GitHub Actions, e questo ambiente non
può scaricare un NDK Android per policy di rete): la build Gradle completa con la nuova UI (due
import, uno slider, il rendering dell'anteprima incollata) — è la parte Kotlin più estesa
consegnata finora in un colpo solo, mai compilata per davvero prima d'ora.

Non ancora presente:

- **Demosaic completo**: `raw-decode` estrae solo l'anteprima incorporata dalla fotocamera
  (istantanea, nessun calcolo pesante), non l'immagine RAW "sviluppata" pixel per pixel a piena
  risoluzione (§3.2) — `look-render` lavora quindi sull'anteprima, non sul RAW pieno.
- Bilanciamento del bianco assoluto nel rendering (richiede un profilo colore camera).
- `gpu-pipe` collegato alla UI per il rendering a piena risoluzione in tempo reale.
- `cache`, `catalog`, `job-scheduler` — non bloccanti per il flusso attuale (una foto campione +
  una foto target per volta, non un batch di centinaia di foto insieme).

## Comandi

```bash
cd engine
cargo build --workspace   # compila tutti i crate
cargo test --workspace    # esegue tutti i test (validazione shader, raw-decode, look-render, ffi)

# Genera i binding Kotlin (stesso comando usato dalla CI):
cargo build -p rawforge-ffi
cargo run --bin uniffi-bindgen -- generate --library target/debug/librawforge_ffi.so \
  --language kotlin --out-dir /tmp/kotlin-bindings
```
