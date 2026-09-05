# RawForge Engine (Rust)

Workspace del motore nativo di RawForge, come descritto in `../docs/ARCHITECTURE.md`.

## Stato attuale

Crate reali, compilati e testati (91 test, tutti verdi — `color_science` 6, `core_types` 0,
`gpu_pipe` 3, `harmonic` 17, `look_render` 29, `metadata` 3, `raw_decode` 4, `ffi` 22, `smartbatch`
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

## Corretto (questo giro): "Incolla impostazioni" desaturava il target ben oltre il campione stesso

L'utente ha fornito due foto vere e chiesto una verifica precisa: dopo "Incolla impostazioni" la
foto target diventava troppo desaturata, con "artefatti" — non solo diversa dal target originale,
ma molto più desaturata della STESSA foto campione che doveva riprodurre. Misurato con la chroma
Lab media (indipendente dalla luminosità, a differenza della saturazione HSL grezza): campione
~4.46, target originale ~8.39, target dopo il paste **~1.45** — un terzo della chroma del campione
stesso. Isolando ogni stadio della pipeline sulle foto vere (script `cargo run --example` dedicati,
non ipotesi), sono emerse due cause reali distinte:

**1. `hsl_sat` ancora ancorato a una costante esterna, incoerente con `hsl_lum`/`hsl_hue`.** La
correzione della sessione precedente aveva solo ristretto il range (±100 → ±50) senza cambiare il
confronto: `band_mean_sat` restava confrontato con `BASELINE_HSL_SATURATION` (costante fissa),
mentre `hsl_lum` confronta ogni banda con `overall_hsl_lum` (media dell'INTERA foto campione) e
`hsl_hue` con `band_center_hue` — entrambi confronti INTERNI/relativi alla stessa foto, non esterni.
Su una foto uniformemente poco satura questo spingeva quasi tutte le bande verso l'estremo negativo
(misurato: 6 bande su 8 al -50 di clamp su una foto vera), e quel bias si sommava
moltiplicativamente a `vibrance` (bias globale, calcolato dalla STESSA caratteristica "poca chroma
media") — la foto veniva penalizzata due volte per lo stesso fatto invece che una volta sola più
un'informazione nuova. **Corretto**: aggiunto un accumulatore `sum_hsl_sat` (su TUTTI i pixel, non
solo quelli cromatici come `bucket.sum_sat`) e `overall_hsl_sat = sum_hsl_sat / n`; la formula
diventa `((band_mean_sat - overall_hsl_sat) * 150.0).clamp(-50.0, 50.0)` — additiva e relativa alla
stessa foto, esattamente come `hsl_lum`. Anche `BASELINE_CHROMA` (18.0, mai verificata) è stata
ricalibrata a 10.0, misurata sulle due foto vere (chroma effettiva ~4.5 e ~8.5 in ciascuna).
Il test `saturated_band_gets_a_positive_saturation_bias_others_stay_at_zero` è stato riscritto (la
vecchia foto-quasi-tutta-un-colore non aveva più senso col confronto relativo: serve un "resto della
foto" con cui confrontarsi) e un nuovo test
`uniformly_saturated_photo_gives_no_per_band_bias_only_global_vibrance` verifica esplicitamente che
una foto uniformemente satura non riceva PIÙ bias per banda, solo il bias globale di `vibrance`.

**2. Tone curve e contrasto applicati per canale in `look-render`, non alla luminosità — la causa
più grande.** `render_preview_with_look` applicava `sample_lut(&tone_curve_lut, *c)` e
`(*c - 0.5) * contrast_amount + 0.5` a ciascun canale R/G/B indipendentemente. Applicare la stessa
curva/riscalamento a ogni canale separatamente comprime le DIFFERENZE fra i canali — cioè la
chroma/saturazione — come effetto collaterale, anche con valori non estremi: misurato isolando ogni
stadio, la sola tone curve tagliava la chroma media di ~40%, il contrasto da solo di un altro ~25%.
La lift ombre/luci preesistente non soffriva di questo perché è additiva e identica sui tre canali
(sposta la luma senza toccare le differenze fra canali — la luma è una combinazione lineare con pesi
che sommano a 1, quindi uno shift uguale su R/G/B sposta la luma di esattamente quello shift).
**Corretto** applicando lo stesso principio a tone curve e contrasto: si calcola la luma del pixel,
si applica la curva/il contrasto alla SOLA luma, si ricava il delta, e si sposta ogni canale di
quel delta (ricalcolando la luma da zero dopo ogni stadio, non riusando il valore teorico pre-clamp,
per restare corretti anche quando un canale satura a 0 o 255). Nuovo test
`negative_contrast_and_a_real_tone_curve_do_not_collapse_saturation`: un pixel arancione
moderatamente saturo, contrasto -40 e una tone curve reale (non identità) non devono far crollare
la sua saturazione HSL sotto l'80% dell'originale.

**Risultato combinato**, misurato sulle stesse foto vere: chroma del rendering finale da ~1.45 a
**~6.24** — sopra la chroma del campione stesso (4.46), vicina all'originale del target (8.39).
Ispezione visiva: sedili rossi vividi (non più smorti), pavimentazione con la tonalità calda del
campione invece di un grigio spento, nessun artefatto. **Onestà**: due foto reali non sono un
corpus — `BASELINE_CHROMA = 10.0` e il moltiplicatore `150.0` di `hsl_sat` restano stime, da
ritarare con più foto di riferimento; il principio strutturale (curve sulla luminosità, non per
canale; bias per-banda relativo alla stessa foto) è però una correzione solida, non solo un numero.
Workspace completo: 74 test, tutti verdi.

## Corretto (questo giro): "Incolla impostazioni" desaturava ANCORA i sedili rossi sotto il livello del target originale

Il giro precedente (sezione sopra) aveva verificato con uno script di debug isolato che la chroma
MEDIA dell'intera foto migliorava (1.45 → 6.24). L'utente ha però rifatto la build (dopo un fix di
compilazione separato, vedi sotto) e riportato che il problema persisteva sui sedili rossi
specificamente. Rieseguito lo stesso script di debug end-to-end (`paste_look_from_sample` reale,
non solo la formula isolata) e misurata la chroma Lab SOLO sulla regione dei sedili (una maschera
sui pixel con saturazione HSL > 0.3, non tutta la foto): campione 36.7, target originale 35.0
(quasi identici — il colore della pelle è simile in entrambe le foto, cambia soprattutto lo sfondo),
render dopo "Incolla impostazioni" **18.2** — molto sotto ENTRAMBI, l'opposto dell'obiettivo.

**Causa**: `vibrance` globale (-55 su questa foto, dominato da un ampio asfalto grigio quasi neutro,
vedi sezione precedente) veniva applicato in `look-render` come moltiplicatore PIATTO
(`global_sat_mul`) uguale per ogni pixel, qualunque fosse la sua saturazione di partenza. Un
moltiplicatore piatto riduce la saturazione in valore ASSOLUTO più sui pixel già molto saturi (i
sedili rossi) che su quelli quasi grigi (che di saturazione ne hanno poca da perdere) — l'esatto
opposto di cosa significa "vibrance" in un editor fotografico reale, a differenza di "saturation":
la vibrance è pensata per proteggere i colori già vividi (tipicamente soggetti intenzionali come
pelle, tessuti, fiori) e agire soprattutto sui colori spenti/di sfondo. Il vecchio guardrail (clamp
0.35..2.5 sul moltiplicatore) attutiva l'effetto ma restava piatto: stessa percentuale di riduzione
per ogni pixel.

**Corretto** sostituendo il moltiplicatore piatto con la formula standard di vibrance non lineare:
`protezione = (1 - saturazione_attuale)²`, `moltiplicatore = 1 + vibrance × protezione`. A
saturazione già alta (→1.0) il moltiplicatore tende a 1.0 qualunque sia `vibrance` (pixel già pieno
di colore, protetto); a saturazione quasi nulla il moltiplicatore riceve il pieno effetto di
`vibrance` (dove comunque non è percepibile). Il quadrato (non lineare) invece che una protezione
lineare semplice è stato scelto DOPO aver misurato che quella lineare proteggeva a sufficienza solo i
pixel vicinissimi alla saturazione massima, lasciando un calo ancora percepibile sulla fascia
medio-alta (~0.6-0.7) dove ricade la pelle dei sedili in ombra. `saturation` (lo slider esplicito
"Saturazione", diverso da "Vivacità") resta invece un moltiplicatore piatto, invariato: è un intento
diretto dell'utente, non una statistica della foto campione da correggere.

Risultato misurato sulle stesse foto vere (chroma Lab, maschera solo-sedili): 18.2 → **35.5** —
ora praticamente identica al target originale (35.0) e al campione (36.7), invece che ben sotto
entrambi. Nuovo test `negative_global_vibrance_protects_a_very_saturated_pixel_more_than_a_moderately_saturated_one`
in `look-render`: con lo stesso `vibrance` fortemente negativo, un pixel molto saturo deve perdere
una frazione relativa della propria saturazione minore di uno moderatamente saturo (mai il
contrario), e non deve comunque perderne più del 15% — prima della correzione un pixel così ne
perdeva oltre il 30%. Workspace completo: 83 test, tutti verdi.

## Corretto (questo giro): la tinta del target non convergeva verso quella del campione dopo "Incolla impostazioni", anche a contrasto/chroma ormai corretti

Segnalato dall'utente dopo il fix del contrasto sopra: "resta un'ultima cosa da sistemare, ovvero i
colori non corrispondono del tutto... la tinta anche è parecchio diversa". Misurato sui sedili rossi
delle due foto vere (spazio Lab, zona sedili isolata via template matching): la chroma era già ben
recuperata (94% dell'originale, 34.53 contro 36.39, vicina anche a quella del campione, 36.68) — ma
la TONALITÀ restava sostanzialmente quella di partenza del target (invariata: 11.0° prima del paste,
11.5° dopo), lontanissima dai 25.3° del campione che l'utente voleva copiare.

Causa: `hsl_hue`, calcolato da `harmonic::extract_look_from_reference`, è per costruzione uno scarto
RELATIVO — quanto la banda di tonalità del CAMPIONE si discosta dal proprio centro-banda canonico
(un "vezzo stilistico" del campione), non un valore assoluto verso cui il target deve convergere.
Applicare lo stesso piccolo scarto assoluto al target (che parte da una tonalità di partenza diversa)
non fa convergere le due tinte: due foto dello stesso soggetto sotto luce diversa, entrambe già
vicine al proprio hue canonico "a modo loro", restano diverse fra loro dopo il paste esattamente
quanto lo erano prima — l'intensità dello slider "Adattamento" non aveva alcuna leva su questo, a
differenza di esposizione/luci/ombre (già adattate da Smart-Batch) e contrasto/tone-curve (corretto
nel fix precedente).

**Corretto** con un meccanismo di hue-matching nuovo e complementare, non un rimpiazzo di `hsl_hue`:
`harmonic::analyze_hue_bands` (generalizzazione pubblica dell'accumulo per banda già usato
internamente dall'estrazione) misura la tonalità media per ciascuna delle 8 bande su un'immagine
QUALUNQUE, non solo sul campione; `harmonic::hue_matching_deltas` confronta poi le bande di campione
e target — richiedendo popolazione sufficiente in ENTRAMBE (stesso guardrail `MIN_BAND_PIXELS` già
usato per `hsl_hue`, altrimenti la media di pochi pixel è rumore, non un colore da inseguire) — e
calcola quanto la tonalità MISURATA del campione si discosta da quella MISURATA del target, nella
stessa banda, con differenza circolare corretta (gestisce il wraparound 350°→10°) e un tetto
(`MAX_HUE_MATCH_DELTA = 45°`, la larghezza di un'intera banda) contro shift innaturali quando le due
foto non condividono davvero lo stesso soggetto in quella banda. `ffi::apply_hue_matching` applica
questo delta pesato da `override_strength`, esattamente come gli altri delta adattivi di Smart-Batch:
a 0% nessun hue-matching (Look letterale invariato), a 100% il massimo consentito dal guardrail.

Sei nuovi test in `harmonic` (misura su immagine sintetica, guardrail di popolazione minima,
riproduzione del gap reale misurato sulla foto dell'utente, clamping del guardrail, differenza
circolare) e tre in `ffi` (nessun cambiamento a intensità 0%, shift non nullo a intensità 100%,
verifica end-to-end che il Look applicato porti un delta di hue-matching).

Rimisurato sulla stessa foto vera (tonalità HSL media della zona sedili, media circolare per gestire
correttamente il wraparound — a differenza della chroma/hue in Lab usata sopra, qui la misura è
nello stesso spazio in cui `look-render` applica davvero l'aggiustamento): campione 333.0°, target
originale 340.2° (scarto 7.2°), target dopo "Incolla impostazioni" PRIMA di questo fix 339.7° (scarto
6.7° — praticamente invariato, bug confermato), target dopo questo fix 334.3° (scarto **1.3°** dal
campione — una riduzione dell'81% del divario). La saturazione HSL della stessa zona resta invariata
prima/dopo (0.622 in entrambi i casi): questo fix tocca solo la tonalità, non introduce né corregge
nulla sulla vividezza, che restava già ben recuperata dal fix precedente.

**Onestà sui limiti**: questo è un confronto GLOBALE per banda fra le due foto intere, non un
riconoscimento del soggetto (non sa che "i sedili" sono l'oggetto da abbinare) — funziona perché, per
queste due foto, la banda di tonalità dei sedili (Magenta, 315-360°) è anche la banda dove entrambe
le foto hanno popolazione sufficiente e comparabile. Se due foto condividessero un colore-soggetto
sparso in una banda dove è una minoranza trascurabile del fotogramma in una delle due, il confronto
per banda resta comunque un confronto "quello che c'è in questa banda in entrambe le foto", non
necessariamente "lo stesso oggetto": un limite architetturale onesto, non nascosto — un vero
abbinamento per soggetto richiederebbe segmentazione, fuori scope da questo fix.

## Corretto (questo giro, giro precedente): "Intensità adattamento" non aveva ALCUN effetto su contrasto e tone curve — copiati letteralmente dal campione a qualunque valore dello slider

Segnalato dall'utente con tre foto vere (una pulita, una fortemente "granulosa"): dopo aver
verificato che la foto pulita non aveva alcun rumore di sensore vero (Sony A7IV, ISO 100), e dopo
un chiarimento dell'utente su quale build stesse testando, il problema reale non era rumore ma
— testuale — "i rettangoli grigi che si creano e la mancanza totale di contrasto": i lastroni
rettangolari della pavimentazione (texture/variazione tonale reale, ben visibile nella foto
originale) diventavano piatti e privi di dettaglio dopo "Incolla impostazioni".

Misurato: il contrasto locale della pavimentazione (deviazione standard in una finestra 15×15,
foto vera a piena risoluzione) crollava da 8.23 a 4.56 — **il 55% del contrasto originale, quindi
un calo reale del 45%** — a QUALUNQUE valore dello slider "Intensità adattamento" (0%, 50%, 100%,
nessuna differenza). Causa: `contrast` e `tone_curve` — a differenza di `exposure_ev`, `highlights`
e `shadows`, tutti e tre già tarati in base a questo slider — venivano presi sempre e solo dal
valore LETTERALE estratto dalla foto campione, per intero, quale che fosse la posizione dello
slider. Lo slider prometteva "0% = impostazioni identiche alla foto campione, 100% = massimo
adattamento intelligente alla scena", ma per questi due campi non faceva letteralmente nulla — a
100% (il valore usato dall'utente in tutti i test) l'utente si aspettava MENO copiatura letterale
del campione, non la stessa identica copia di uno slider a 0%. La foto campione aveva una tone
curve che alza le ombre e abbassa le luci più un contrasto già negativo (-23): una piattezza
volutamente scelta per quella foto, trasferita in blocco sul target senza che l'utente avesse alcun
modo di attenuarla.

**Corretto** in `ffi::taper_contrast_and_tone_curve_toward_neutral` (nuova funzione, chiamata da
`paste_look_from_sample`): `contrast` e ogni punto di `tone_curve` vengono ora sfumati verso il
loro valore neutro (0, e curva identità x=y) in proporzione allo stesso `override_strength` già
usato per `exposure_ev` — stesso principio, stessa direzione. A intensità 0% il comportamento resta
identico a prima (copia letterale, come promesso); a intensità 100% contrasto e tone curve tornano
completamente neutri, lasciando il target con la propria tonalità originale invece di quella
(potenzialmente molto piatta) del campione. Quattro nuovi test: tre unitari sulla funzione di
sfumatura (intensità 0 = invariato, intensità 1.0 = completamente neutro, intensità 0.5 = a metà
strada, verificato punto per punto sulla tone curve) e uno end-to-end
(`paste_look_from_sample_flattens_less_at_full_adaptation_strength_than_at_zero`) che verifica che
il contrasto applicato a intensità massima non superi mai, in valore assoluto, quello a intensità
zero.

Misurato di nuovo sulla stessa foto vera con lo stesso "Incolla impostazioni" (intensità 100%,
come nei test dell'utente): contrasto locale della pavimentazione 4.56 → **7.76**, cioè dal 55% al
94% del contrasto originale (8.23) — praticamente ripristinato. Ispezionato visivamente a piena
risoluzione: i lastroni della pavimentazione mostrano di nuovo la loro texture naturale invece di
un grigio piatto uniforme.

## Corretto (questo giro, giro precedente): aggiustamento HSL per banda applicato a piena forza anche su pixel quasi neri, dove la tonalità è inaffidabile

Segnalato dall'utente dopo tutti i fix sopra, con due screenshot: "non ci siamo per niente...
trova la causa di questo schifo". Indagando pixel per pixel una zona molto scura di una foto vera
(la presa d'aria/griglia sotto il paraurti) è emerso un **quarto bug reale**, distinto dal "salto
ripido fra bande" già corretto: l'aggiustamento hue-selettivo per banda (`interpolate_hsl_band`)
viene scelto in base alla TONALITÀ del pixel, ma per un pixel quasi grigio (poco o nulla colorato)
la tonalità è numericamente instabile — quando R, G e B sono tutti vicini fra loro e vicini a zero,
il minimo rumore di sensore/JPEG (presente in QUALUNQUE foto reale) fa oscillare selvaggiamente
quale canale risulti max/min, quindi la tonalità calcolata può saltare di decine o centinaia di
gradi da un pixel al successivo pur essendo i due pixel visivamente identici.

Un primo tentativo di correzione — pesare l'aggiustamento in base alla SATURAZIONE HSL del pixel
(bassa saturazione = poco effetto) — è stato scartato dopo aver misurato che non funzionava:
la formula classica della saturazione HSL, `s = d / (1 - |2L-1|)`, ha un polo esattamente a L=0 e
L=1. Vicino al nero il denominatore tende a zero, quindi anche una croma assoluta minuscola (rumore
vero, pochi millesimi) produce una saturazione HSL riportata vicina a 1.0 — l'OPPOSTO di "poco
saturo". Pesare su quella saturazione avrebbe lasciato questi pixel a piena forza proprio dove
serviva proteggerli di più. **Corretto** pesando invece sulla CROMA ASSOLUTA (`d = max(R,G,B) -
min(R,G,B)`, sempre in 0..1, senza poli), ricavata algebricamente da saturazione e luminosità già
disponibili (`d = s · (1 - |2L-1|)`) invece di ricalcolarla da capo. Sotto una soglia di croma
(0.05) il peso dell'aggiustamento per banda sale con uno smoothstep invece di un taglio netto,
esattamente come già fatto per i confini fra bande. Due nuovi test in `look-render`:
`hue_band_weight_is_low_for_a_near_black_pixel_even_when_its_hsl_saturation_reads_high` (verifica
diretta del motivo per cui il tentativo basato sulla saturazione HSL non bastava) e
`near_black_pixels_are_shielded_from_per_band_hsl_noise_even_across_opposite_hue_bands`
(end-to-end: due pixel quasi neri con la stessa saturazione HSL "ingannevole" ma tonalità agli
antipodi, con un Look che ha un bias di banda molto diverso da un lato all'altro, non devono più
divergere in croma finale).

**Onestà su cosa questo fix cambia e cosa no**: è una correzione reale e verificata (un pixel quasi
nero non riceve più un bias di saturazione arbitrario e diverso da quello del pixel accanto solo
per rumore di tonalità), ma indagando a fondo la STESSA zona scura mostrata dall'utente
(paraurti/griglia) è emerso che la "grana"/speckle visibile lì non è causata né amplificata da
questo bug né da nessun'altra parte della pipeline: renderizzando la stessa foto vera con il Look
completo di "Incolla impostazioni" e, per confronto, con un Look reso quasi nullo (solo il lift
minimo, esposizione/bilanciamento del bianco/tone curve/contrasto/vibrance/bande HSL tutti
azzerati o a identità), la grana in quella zona risulta IDENTICA nei due render — ed è già presente
anche nell'anteprima ridotta SENZA alcun Look applicato. È rumore di sensore/compressione JPEG già
presente nella foto originale, reso più visibile solo dallo zoom elevato usato per ispezionarlo, non
qualcosa che questo motore introduce o amplifica. Una vera riduzione del rumore (già segnalata come
limite noto e pianificata per una fase successiva della roadmap, vedi il commento di modulo in
`look-render/src/lib.rs`) resta l'unico modo per attenuare ATTIVAMENTE quella grana specifica — non
ancora implementata in questo giro.

## Corretto (questo giro, giro precedente): rumore/"glitch" a chiazze sulla saturazione per un salto ripido fra bande HSL adiacenti

Segnalato dall'utente dopo il fix sopra: "glitch non risolti". Il fix precedente (vibrance non
lineare) risolveva la desaturazione media ma NON il glitch visibile a occhio — quindi si trattava
di un bug diverso, ancora presente. L'utente ha anche ipotizzato quattro cause specifiche (spazio
colore/profilo ICC, spazio di lavoro non a 32 bit in virgola mobile, shader GPU che satura, curva di
transfer non normalizzata): invece di applicare alla cieca le sue ipotesi, ognuna è stata verificata
direttamente sul codice e sulle due foto vere:

- **Profilo colore/ICC**: ispezionate entrambe le foto vere con PIL/ImageCms. `photoA.jpg`
  (target) non ha profilo incorporato; `photoB.jpg` (campione) ha un profilo sRGB esplicito
  ("IEC 61966-2.1 Default RGB"). Sono quindi entrambe sRGB — **nessun mismatch reale per queste
  due foto**. Detto ciò, è stato verificato con una ricerca su tutto il workspace che il motore
  non ha ALCUNA gestione dei profili ICC: è un limite architetturale reale (foto con profili
  diversi da sRGB verrebbero trattate come se non lo fossero), separato dal bug qui sotto e non
  la sua causa.
- **Spazio di lavoro a 32 bit in virgola mobile**: già vero. L'intera pipeline per-pixel in
  `look-render` lavora in `f32` dall'inizio alla fine di ogni stage (WB, esposizione, tone curve,
  contrasto, HSL per banda, saturazione/vibrance); solo lettura e scrittura toccano `u8`. Nessuna
  conversione intermedia a 8 bit che potesse introdurre banding.
- **"Matrice di trasformazione cromatica"**: questa architettura non ne usa una. La Sintesi
  Armonica Automatica funziona per bande di tonalità HSL (8 bande, interpolazione circolare), non
  per matrice 3x3 di color science come in un profilo ICC/LUT 3D. Chiarito per evitare di far
  credere che sia stata aggiunta una matrice che in realtà non esiste in questo motore.
- **Shader GPU**: il percorso di rendering live (`look-render`, usato da `PhotoEditSession`) è
  interamente CPU (`rayon`). Il crate `gpu-pipe` contiene sorgenti WGSL validate con `naga` ma
  **non è collegato** a questa funzionalità — quindi uno shader GPU che satura non può essere la
  causa qui.

La causa reale, trovata misurando i valori HSL per banda estratti dalla foto campione vera: alcune
bande adiacenti (circolarmente, es. Purple→Magenta) avevano un salto fino a **45 punti** su
`hsl_sat` (e 25 punti su Magenta→Red). `interpolate_hsl_band` interpola in modo continuo (nessun
salto netto, bug già corretto in un giro precedente), ma un salto così ripido nei VALORI di
partenza fa sì che il normale, minuscolo jitter di tonalità pixel-per-pixel (subsampling cromatico
JPEG, texture della pelle dei sedili, rumore del sensore — presente in qualsiasi foto reale) venga
amplificato in un'oscillazione di saturazione molto più ampia quando il pixel attraversa quella
regione di tonalità a pendenza ripida — producendo la chiazza/speckle visibile ("glitch") distinta
dalla desaturazione piatta già corretta.

**Corretto** in `harmonic::extract_look_from_reference` con una nuova funzione
`smooth_circular_bands`: una media mobile circolare a 3 prese (60% banda corrente + 20% ciascuna
banda adiacente) applicata a `hsl_hue`, `hsl_sat` e `hsl_lum` prima di restituire la
`HarmonicLook` estratta — appiattisce i salti ripidi senza spostare quale banda resta dominante.
Nuovo test `smooth_circular_bands_softens_a_steep_cliff_but_keeps_the_dominant_band_dominant`:
verifica su un caso preso dai dati reali (salto di 45 punti) che dopo lo smoothing il salto massimo
fra bande adiacenti scenda sotto i 30 punti e che la banda dominante resti la stessa.

Misurato sulle stesse foto vere (maschera solo-sedili): salto massimo fra bande adiacenti
45 → **17** punti sui valori estratti; chroma Lab della regione sedili 35.5 (post-fix vibrance,
pre-smoothing) → **31.9** (leggermente sotto, un compromesso accettato in cambio della riduzione
del rumore — resta comunque vicino al target 35.0 e al campione 36.7); metrica di "rugosità"
(passa-alto, proxy autoprodotta e non uno standard di rumore percettivo rigoroso) 2.13
(pre-smoothing) → **1.96** (post-smoothing), entrambe sotto il valore della foto originale stessa
(2.29) — a conferma che si tratta di una reale riduzione del rumore introdotto dall'elaborazione, e
non solo di un'impressione visiva. Come per `BASELINE_CHROMA` e la curva di protezione della
vibrance, anche i pesi 60/20/20 dello smoothing sono una scelta ragionevole ma calibrata solo su
queste due foto vere, non verificata su un corpus più ampio.

## Corretto (questo giro): build Windows falliva con "Unresolved reference: graphicsLayer"

Riportato separatamente dall'utente prima di poter anche solo testare il fix sopra: la build
falliva su `:shared:compileKotlinDesktop` per un import dal pacchetto sbagliato in `App.kt`
(`androidx.compose.ui.draw.graphicsLayer` invece di `androidx.compose.ui.graphics.graphicsLayer` —
l'estensione `Modifier.graphicsLayer` vive nel modulo `ui-graphics`, non `ui` base, a differenza di
`clipToBounds` che invece è davvero in `draw`, da cui la confusione). Non è specifico di Windows: lo
stesso errore avrebbe bloccato anche la build Android, essendo `App.kt` in `commonMain`.

## Corretto (questo giro): layout Android illeggibile — foto compresse in verticale, impossibile vedere campione e target insieme

Segnalato insieme al bug di desaturazione. Causa: sia la schermata principale di confronto sia la
modalità "Modifica a schermo intero" usano un `Row` con un pannello Develop a larghezza FISSA (320dp
e 360dp rispettivamente) accanto al resto del contenuto — un layout pensato per una finestra desktop
larga, senza nessun adattamento per uno schermo stretto. Su un telefono largo 360-400dp il solo
pannello Develop consuma quasi tutta la larghezza disponibile, lasciando alle foto pochissimi pixel
e costringendo Compose a schiacciare in verticale tutto il resto (pulsanti, slider) pur di farlo
stare nello spazio residuo.

**Corretto** in entrambe le schermate con `BoxWithConstraints`: sotto i 700dp di larghezza
disponibile si passa da un layout affiancato (`Row`) a uno impilato in verticale (`Column`) — stesso
identico contenuto (stessi componenti, stesse azioni, definiti una sola volta ed estratti in lambda
locali per evitare di duplicarli con il rischio che le due versioni divergano), disposizione diversa.
Nella schermata di confronto le due foto restano affiancate anche nel layout stretto (il confronto
campione/target è il punto della schermata, e un telefono ha comunque larghezza sufficiente per due
miniature verticali — le foto usate per scoprire il bug erano proprio verticali) ma con un'altezza
fissa (240dp) invece di condividere lo spazio con il resto della pagina; il pannello Develop passa da
barra laterale a blocco a piena larghezza sotto le foto, con il suo scroll verticale interno
(preesistente) invariato. Nella modalità a schermo intero, dove non c'è un blocco a contenuto fisso
da preservare, la foto e il pannello Develop si dividono lo spazio verticale a metà. **Non verificato
da un compilatore** (limite noto di questo ambiente per tutta la parte Kotlin/Compose): verifica
prevista alla prossima build su GitHub Actions.

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
