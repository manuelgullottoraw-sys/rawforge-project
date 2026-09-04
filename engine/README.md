# RawForge Engine (Rust)

Workspace del motore nativo di RawForge, come descritto in `../docs/ARCHITECTURE.md`.

## Stato attuale

Crate reali, compilati e testati (72 test, tutti verdi — `color_science` 6, `core_types` 0,
`gpu_pipe` 3, `harmonic` 10, `look_render` 24, `metadata` 3, `raw_decode` 4, `ffi` 15, `smartbatch`
5, `xmp` 2):

| Crate | Cosa fa | Rif. architettura |
|---|---|---|
| `core-types` | Strutture dati condivise (`HarmonicLook` e affini) | §5.1 |
| `color-science` | Conversioni sRGB↔lineare, RGB↔Lab, RGB↔HSL | §3.2 |
| `harmonic` | Sintesi Armonica: estrae tone curve, palette (split toning), contrasto, WB e ora anche HSL per banda da un'immagine di riferimento | §4.1 |
| `smartbatch` | Smart-Batch Contestuale: descrittori di scena da istogramma + calcolo dei delta adattivi con guardrail | §4.2 |
| `metadata` | Sidecar JSON non distruttivo (schema versionato, history di operazioni) | §3.1 |
| `xmp` | Generatore di preset Lightroom `.xmp` dal `HarmonicLook` | §5 |
| `gpu-pipe` | Sorgenti WGSL degli stage di color grading, validati con `naga` (nessuna GPU richiesta per i test) | §3.2, §6.2 |
| `raw-decode` | Decodifica RAW vera (`rawler`, Rust puro): anteprima incorporata dalla fotocamera + metadati base | §2, §9 |
| `look-render` | Applica un `HarmonicLook` ai pixel su CPU (bilanciamento del bianco anche a gradiente, esposizione, tone curve, contrasto, highlights/shadows, HSL per banda, split toning, texture a bande di frequenza) più le frazioni di clipping per "slider sicuri" — l'anteprima "incolla impostazioni" e il pannello "Develop" | §3.2 |
| `ffi` | Superficie **UniFFI** che espone tutti i crate sopra a Kotlin, incluso l'oggetto stateful `PhotoEditSession` (vedi sotto) — è questo il crate che la pipeline CI compila per Android (via `cargo-ndk`) e Windows (nativo), generando anche i binding Kotlin usati da `shared/` | §1, §7 |

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
- `gpu-pipe` collegato alla UI per il rendering a piena risoluzione in tempo reale.
- `cache`, `catalog`, `job-scheduler` — non bloccanti per il flusso attuale (una foto campione +
  una foto target per volta, non un batch di centinaia di foto insieme).

## Nuovo: pannello di editing manuale ("Develop") lato motore

Per il pannello di editing manuale della UI (sliders su esposizione, contrasto,
highlights/shadows/whites/blacks, bilanciamento del bianco, vibrance/saturazione, split toning),
serviva un modo di renderizzare un `HarmonicLook` qualunque su una foto SENZA rifare
l'estrazione dalla foto campione né l'adattamento Smart-Batch — è il passo veloce richiamato a
ogni movimento di uno slider. Questo passo esiste oggi come metodo di `PhotoEditSession` (vedi la
sezione successiva, che ha sostituito la versione originaria a funzione libera descritta più sotto
nello storico di questo file).

## Nuovo (questo giro): `PhotoEditSession` — decodifica una sola volta, rendering dal vivo

Richiesta dell'utente dopo un uso reale: "far corrispondere le modifiche degli slider in tempo
reale sulla foto, perché così è veramente difficile da utilizzare". Causa: ogni singola chiamata
di rendering (una per ogni tick di uno slider trascinato) ripartiva da zero — ri-decodificava
l'intera foto target dai bytes originali (RAW compreso) e renderizzava a piena risoluzione
originale (potenzialmente 24+ megapixel), ritrasmettendo anche l'intera foto attraverso il confine
Kotlin/JNI ad ogni chiamata. Con quel costo per singolo tick, seguire uno slider mentre si
trascina non era praticabile.

Sostituito il vecchio schema a funzioni libere (`paste_look_onto_target_photo`,
`render_look_on_photo`, prese rispettivamente da bytes grezzi ogni volta) con un nuovo oggetto
UniFFI **stateful**, `PhotoEditSession` (`#[derive(uniffi::Object)]`, esposto a Kotlin come classe
vera con lifecycle `Disposable`/`AutoCloseable`):

- **`PhotoEditSession::new(target_bytes, target_file_name)`** — decodifica il target UNA SOLA
  VOLTA (RAW-aware come prima) e la mantiene cacheiata in memoria in due copie: `full_res` (la
  decodifica originale) e `interactive_preview`, una copia ridotta apposta (lato più lungo max
  `INTERACTIVE_PREVIEW_MAX_DIM = 1024` px, `image::FilterType::Triangle`) pensata per essere
  veloce da renderizzare e leggera da ritrasmettere.
- **`render_preview(look)`** — renderizza SOLO sulla copia ridotta cacheiata: è il metodo
  richiamato ad ogni singolo movimento di uno slider. Niente ri-decodifica, niente pixel a piena
  risoluzione, niente ri-trasmissione della foto (solo i parametri leggeri del Look).
- **`render_full_resolution(look)`** — renderizza sulla copia a piena risoluzione: più lento apposta,
  usato solo dal pulsante "Esporta foto…", non ad ogni modifica.
- **`paste_look_from_sample(sample_bytes, sample_file_name, look_name, override_strength)`** —
  stesso algoritmo di "incolla impostazioni" di prima (estrazione dal campione, descrittori di
  scena, delta Smart-Batch guardrailati, interpolazione dell'esposizione assoluta descritta più
  sotto), ma renderizza sulla copia ridotta già cacheiata invece di ri-decodificare il target.

La sessione va aperta una volta quando l'utente sceglie/cambia la foto da modificare
(`Engine.openPhotoForEditing`, lato Kotlin) e chiusa esplicitamente (`close()`/`destroy()`) quando
non serve più, per liberare la memoria allocata lato Rust — non c'è un finalizer automatico.
Nuovi test dedicati in `ffi`: `photo_edit_session_open_reports_error_on_bad_bytes`,
`paste_look_from_sample_renders_and_reports_positive_recovery_on_darker_target`,
`paste_look_from_sample_does_not_force_large_exposure_shift_when_target_matches_reference_scene`
(la stessa regressione del bug storico di -1.09 EV, ora verificata passando dalla sessione),
`paste_look_from_sample_reports_error_on_bad_sample_bytes`,
`render_preview_applies_manual_exposure_without_reextraction`,
`render_full_resolution_uses_the_uncropped_original_size` (verifica che l'export usi davvero le
dimensioni originali mentre l'anteprima interattiva resta entro `INTERACTIVE_PREVIEW_MAX_DIM`).

## Nuovo (questo giro): bilanciamento del bianco reso davvero nel renderer

Prima il bilanciamento del bianco (temp/tint) era dichiarato esplicitamente come non implementato
nel rendering ("servirebbe un profilo colore camera"). Ora `look-render` applica temp/tint come un
guadagno per canale in spazio lineare RGB (`WB_STRENGTH = 0.35`, temp più alta = più caldo/giallo,
stessa convenzione dello slider di Lightroom) — un'approssimazione dichiarata da color grading,
non colorimetricamente accurata (non deriva da un vero profilo camera), ma sufficiente a far
corrispondere meglio la temperatura colore quando si copia lo stile da una foto di riferimento.
Il default (temp 5500K, tint 0) produce guadagni tutti 1.0, quindi non introduce nessuna regressione
sul comportamento "look neutro = immagine invariata" già testato. Nuovi test:
`warm_white_balance_raises_red_relative_to_blue`, `cool_white_balance_raises_blue_relative_to_red`,
`magenta_tint_lowers_green_relative_to_neutral`.

## Nuovo (questo giro): estrazione HSL per banda nella Sintesi Armonica

`HarmonicLook.hsl` (le regolazioni hue/saturazione/luminanza per 8 bande di colore, già supportate
dal renderer da tempo) restava sempre a zero: la Sintesi Armonica non le calcolava mai in
estrazione. Ora `harmonic::extract_look_from_reference` accumula, per ogni pixel non
quasi-neutro (saturazione HSL > 0.02), un bucket per una delle 8 bande di tonalità (stesso schema
a 8 bande già usato in applicazione da `look-render`), con una soglia minima di popolazione per
banda (`MIN_BAND_PIXELS = 40`) per evitare che bande poco rappresentate producano regolazioni
rumorose. Le tre grandezze sono calcolate con attenzione a restare nello spazio colore corretto
(confrontando statistiche in spazio HSL con basi/medie anch'esse in spazio HSL, non mescolando con
i valori Lab già usati altrove nella stessa funzione) e sempre relative alla scena stessa (mai un
pivot assoluto fisso che potrebbe dominare in modo imprevedibile — stesso principio già applicato
correggendo il bug storico dell'esposizione): la saturazione di banda è relativa a una baseline
dichiarata (`BASELINE_HSL_SATURATION = 0.35`), la luminanza di banda è relativa alla luminanza HSL
media dell'intera immagine, la tonalità di banda è relativa al centro nominale della banda stessa.
Nuovi test: `saturated_band_gets_a_positive_saturation_bias_others_stay_at_zero`,
`near_neutral_gray_leaves_all_hsl_bands_at_zero`.

Insieme, bilanciamento del bianco e HSL per banda sono le due leve principali rimaste per
migliorare la fedeltà della copia di stile dalla foto di riferimento: prima di questo giro
venivano estratte/applicate solo esposizione, contrasto, tone curve, highlights/shadows e split
toning.

## Nuovo (questo giro): texture a bande di frequenza, clipping per "slider sicuri", WB a gradiente

Tre aggiunte a `core_types::HarmonicLook` (e al suo specchio `HarmonicLookFfi` in `ffi`, con
entrambi i `From` aggiornati e testati dal round-trip in `harmonic_look_ffi_round_trip_preserves_all_fields`),
tutte e tre nuovi campi di `look-render`:

- **`texture_fine`/`texture_medium`/`texture_coarse`** (-100..100 ciascuno). Separazione di
  frequenza gaussiana vera: `apply_texture_bands` sfoca l'immagine già color-gradata a tre raggi
  crescenti (`image::imageops::blur`, sigma 1.2/4/10), ricava le bande di dettaglio per differenza
  tra sfocature successive (la più sfocata di tutte è il "residuo" a bassa frequenza, mai toccato:
  colore e tono di base), poi ricompone scalando ogni banda di `1 + amount/100`. Con amount a 0 la
  ricostruzione è esatta (residuo + somma delle differenze = l'originale), quindi un'immagine a
  tinta piatta resta invariata qualunque sia l'amount — verificato da
  `zero_texture_amounts_leave_a_solid_color_image_unchanged` e, più a fondo, da
  `fully_negative_texture_smooths_an_isolated_bright_point_toward_its_surroundings` (un singolo
  pixel chiaro isolato, texture -100 su tutte e tre le bande, il pixel centrale deve avvicinarsi
  allo sfondo). È un passo SEPARATO dal loop per-pixel principale di
  `render_preview_with_look` (un'operazione spaziale — serve leggere pixel vicini — non può vivere
  in un ciclo che processa un pixel alla volta), eseguito solo se almeno un amount è diverso da 0.
- **`clipping_fractions(image) -> (f32, f32)`** (shadow, highlight): frazione di pixel con luma
  ≤ 2 e ≥ 253 nell'immagine GIÀ RENDERIZZATA — non un'analisi dell'originale. `ffi::render_preview`
  la calcola subito dopo il rendering e la restituisce in un nuovo record, `RenderedPreviewFfi
  { preview_png_bytes, shadow_clip_fraction, highlight_clip_fraction }`, sostituendo il precedente
  `Vec<u8>` semplice come tipo di ritorno (tutti i call site Rust aggiornati, inclusi i test
  esistenti che ora leggono `.preview_png_bytes`). Deliberatamente calcolato solo per il rendering
  CORRENTE, non per l'intero range di uno slider — vedi il README di primo livello per il
  ragionamento sul costo. `render_full_resolution` resta `Vec<u8>` semplice: l'esportazione finale
  non ha bisogno del feedback dal vivo. Test: `clipping_fractions_detects_pure_black_and_white_images`,
  `clipping_fractions_reports_zero_for_a_midtone_image`,
  `render_preview_reports_high_shadow_clip_fraction_for_a_crushed_black_image` (quest'ultimo in
  `ffi`, end-to-end attraverso `PhotoEditSession`).
- **Bilanciamento del bianco a gradiente**: `white_balance_b: WhiteBalance` (seconda zona) più
  `wb_gradient_enabled`, `wb_gradient_vertical`, `wb_gradient_position` (0..100),
  `wb_gradient_spread` (0..100). Il loop per-pixel principale ora traccia la posizione `(x, y)` di
  ogni pixel (prima assente: bastava scorrere righe/pixel senza sapere dove fossero — aggiunto un
  `.enumerate()` sulle righe e uno sui pixel di ogni riga) e, quando il gradiente è attivo,
  interpola il guadagno WB fra `compute_wb_gain(&white_balance)` (zona A) e
  `compute_wb_gain(&white_balance_b)` (zona B) tramite `gradient_blend_factor`: una transizione
  lineare lungo l'asse scelto, centrata su `wb_gradient_position` (percentuale lungo l'asse) e
  larga `wb_gradient_spread` (0 = bordo netto, 100 = sfumatura sull'intero fotogramma). Con
  `wb_gradient_enabled = false` (il default) il comportamento è identico a prima — un solo
  guadagno globale — verificato da `gradient_white_balance_is_ignored_when_disabled`. Test:
  `gradient_white_balance_differs_between_left_and_right_zones_when_enabled`.

**Scelte di design dichiarate** (le stesse discusse con l'utente prima di implementare): il
bilanciamento del bianco a gradiente usa due zone lungo un asse, non punti liberi piazzabili a
piacere sulla foto — una UI di posizionamento 2D e un'interpolazione multi-punto sarebbero un
lavoro sostanzialmente più grande per lo stesso caso d'uso reale (cielo freddo in alto, terreno
caldo in basso); "slider sicuri" mostra solo il clipping del valore ATTUALE, non un'anteprima
dipinta sull'intero binario dello slider.

## Corretto (questo giro): quattro bug reali di dominanti/blocchi di colore segnalati dall'utente

Tre segnalazioni consecutive dell'utente hanno portato a quattro correzioni reali nel motore (non
semplici ritocchi estetici):

**1. Confine netto ogni 45° nell'applicazione HSL per banda (`look-render`).**
`render_preview_with_look` applicava gli 8 aggiustamenti HSL per banda
(`HarmonicLook.hsl.{hue,sat,lum}`) assegnando ogni pixel a un'UNICA banda in base alla sua
tonalità (`floor(hue / 45) % 8` implicito) e applicandone l'aggiustamento per intero — nessuna
transizione fra bande adiacenti. Su tonalità che variano con continuità (fogliame, cielo) questo
produceva bordi artificiali netti a forma di blocco ovunque la tonalità di due pixel vicini
cadesse ai due lati di un confine banda — il difetto "immagine a blocchi/posterizzata" segnalato
per primo. **Corretto** con `interpolate_hsl_band(values: &[i32; 8], hue: f32) -> f32`:
interpolazione lineare circolare (wrap a 360°) fra i valori delle due bande più vicine al centro
del proprio intervallo di 45° — ogni banda ha pieno effetto solo al proprio centro, ai bordi
l'effetto sfuma 50/50, la somma dei pesi resta sempre 1. Cinque test dedicati (incluso uno
end-to-end che riproduce lo scenario: due tinte a tonalità quasi identica ai due lati di un
confine banda, verificando che la differenza di saturazione risultante resti piccola invece che
radicale).

**2. Range troppo ampio per `hsl_sat` in estrazione (`harmonic`).** Nella Sintesi Armonica,
`hsl_sat[banda]` (scarto percentuale della saturazione media di banda rispetto alla soglia fissa
`BASELINE_HSL_SATURATION = 0.35`) era limitato a `.clamp(-100.0, 100.0)`, a differenza dei suoi
"fratelli" `hsl_lum` (±30) e `hsl_hue` (±15), i cui commenti nel codice già dichiaravano
esplicitamente range più stretti "perché è il ritocco più visibile/rischioso". Meccanismo:
`band_mean_sat` è una MEDIA ARITMETICA di valori di saturazione HSL sempre non-negativi (nessuna
cancellazione vettoriale, a differenza per esempio del centroide Lab a/b usato dallo split
toning, che invece cancella naturalmente rumore in direzioni opposte) — quasi ogni banda popolata
prevalentemente da pixel quasi neutri/poco saturi (la stragrande maggioranza delle bande nella
maggior parte delle foto reali) calcola una `band_mean_sat` ben sotto 0.35, spingendo il bias di
quella banda verso l'estremo -100 ("desatura completamente questa tonalità"); al contrario una
banda che cattura anche poco colore incidentale (verificato: un "rosso" test (200,30,40) ha hue
effettivo ≈356.5°, quindi cade nella banda "Magenta" 315-360° invece che "Rosso", per la sua
lieve componente blu) può schizzare all'estremo opposto +100. Combinato con la correzione al
punto 1 (che DIFFONDE l'influenza di un valore estremo su un range di tonalità più ampio tramite
l'interpolazione), questo spiega plausibilmente la dominante di colore diffusa segnalata come
"peggiorata" dopo la prima correzione. **Corretto**: range ristretto a `.clamp(-50.0, 50.0)`. Lo
slider HSL MANUALE nella UI (±100, esposto in `look-render`) resta deliberatamente invariato: è
una scelta creativa esplicita dell'utente, non un artefatto dell'estrazione automatica.

**3. `look.whites`/`look.blacks` mai letti dal renderer (`look-render`).** I campi
`HarmonicLook.whites`/`.blacks` (slider UI "Bianchi"/"Neri") esistono nel modello dati,
attraversano FFI e l'export `.xmp`, ma — verificato con `grep` diretto, zero riscontri — non
venivano mai usati in `render_preview_with_look`: non avevano alcun effetto sull'immagine
renderizzata. Difetto preesistente (non introdotto in questo giro), ma rilevante perché l'utente
aveva provato a rispondere all'avviso "ombre schiacciate" delle slider sicure impostando
Neri=-60, senza alcun effetto visibile. **Corretto**: aggiunte `blacks_mask(luma)`/
`whites_mask(luma)`, sullo stesso schema di `shadow_mask`/`highlight_mask` ma con zone più
STRETTE (`(1.0 - luma / 0.12).clamp(0,1)` e `((luma - 0.88) / 0.12).clamp(0,1)`, contro le zone
larghe di ombre/luci sotto 0.4/sopra 0.6) — mirate ai soli estremi tonali veri, non all'ampia
metà inferiore/superiore del range. Stesso segno di ombre/luci (positivo = schiarisce quella
zona): per correggere "ombre schiacciate" la risposta corretta è Neri POSITIVO, non negativo.
Due nuovi test: `positive_blacks_lifts_near_black_pixels_more_than_midtones`,
`negative_whites_pulls_near_white_pixels_down_more_than_midtones`.

**Verificato ma escluso come causa della dominante segnalata**: il bilanciamento del bianco a
gradiente, testato riproducendo esattamente i valori slider dello screenshot dell'utente
(`white_balance={5389,0}`, `white_balance_b={11038,0}`, gradiente verticale, posizione 47,
ampiezza 9) tramite uno script di debug dedicato — produce una transizione fredda/calda morbida e
ragionevole, non una dominante innaturale. Anche `smartbatch::apply_deltas` è stato escluso
leggendone il codice per intero: modifica solo `exposure_ev`/`highlights`/`shadows`, mai i campi
del bilanciamento del bianco (singolo o a gradiente).

**4. `split_toning.shadow_sat`/`highlight_sat` senza alcuna baseline (`harmonic`).** Dopo la
consegna dei primi tre punti, l'utente ha confermato un indizio decisivo: il problema si
presentava SOLO usando "Incolla impostazioni", mai con l'editing manuale. Questo confina la causa
alla sola estrazione automatica — split toning manuale parte sempre da 0/0, quindi il difetto non
può essere lì. A differenza di `hsl_sat`/`vibrance` (entrambi già uno scarto RELATIVO a una
baseline), lo split toning usava la chroma Lab GREZZA della zona ombre/luci (`shadow_chroma`/
`highlight_chroma` da `lab_ab_to_hue_chroma`), limitata solo a `.clamp(0.0, 100.0)` — nessun
confronto con quanto sia "tipicamente colorata" quella zona in una foto qualunque. Verificato con
uno script di debug dedicato: anche una foto campione scattata alla luce del giorno, SENZA alcuna
intenzione di grading (solo la normale, lieve differenza di colore fra cielo/ombra e sole diretto
che ha qualunque scatto — non un test pattern estremo), produceva `shadow_sat`/`highlight_sat` non
trascurabili (es. 3-4 su una scala 0-100), copiati per intero sul target con "Incolla
impostazioni" e applicati su zone tonali ampie (ombre sotto luma 0.4, luci sopra 0.6 — in molte
foto la maggioranza dei pixel). **Corretto**: sottratta `BASELINE_SPLIT_CHROMA = 6.0` prima del
clamp (`.max(0.0)` per restare comunque non-negativo, poi `.clamp(0.0, 50.0)` — range dimezzato,
stessa proporzione già applicata al punto 2), lasciando intatto lo split toning genuinamente
graduato (es. "teal & orange" deliberato — verificato che il test esistente
`teal_and_orange_split_produces_distinct_shadow_and_highlight_hues` continui a passare). Nuovo
test: `mild_incidental_color_variation_does_not_produce_split_toning`. Verificato anche
end-to-end (estrazione + Smart-Batch + rendering, non solo la sola estrazione isolata) su una
scena sintetica con contenuto vario — cielo, fogliame, carrozzeria, asfalto, non bande piatte —
che il risultato dopo "Incolla impostazioni" resti un'interpretazione moderata dello stile: ogni
zona mantiene la propria tinta caratteristica (cielo bluastro, fogliame verdastro) senza
dominante estranea sovrapposta.

Non è stato possibile riprodurre al 100% l'esatta dominante magenta/rosa dello screenshot
originale dell'utente partendo dai soli valori slider visibili in foto — ma con questi quattro
punti corretti, ogni meccanismo di estrazione automatica individuato nel codice che poteva
produrre una dominante sproporzionata rispetto allo stile reale della foto campione è stato
guardrailato e verificato, sia isolatamente sia end-to-end. Se il problema dovesse ripresentarsi,
condividere la foto campione e quella target permetterebbe di individuare la causa esatta invece
di continuare a ipotizzare da uno screenshot.

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
