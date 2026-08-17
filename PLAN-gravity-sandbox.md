# Gravity Sandbox — Piano d'Implementazione

> Basato su `SPEC-gravity-sandbox.md`
> Obiettivo: sandbox funzionante in browser via WASM

---

## Timeline & Stime

| Fase | Dipende da | Stima (grossolana) | Deliverable |
|------|-----------|-------------------|-------------|
| **M1** Scheletro | — | 3-4 sessioni | App Bevy + Avian in WASM con corpi e gravità |
| **M2** Sandbox UI | M1 | 4-5 sessioni | Strumenti, timeline, property panel, minimap, JSON |
| **M3** Illuminazione | M1 | 3-4 sessioni | Shader, luci, ombre |
| **M4** Sfondo & polish | M1 | 2 sessioni | Parallasse, shortcut, trackpad |
| **M5** Texture & normal map | M3 | 2 sessioni | Texture procedurali, normal map integrate |

Le fasi M3 e M4 sono parallele (entrambe dipendono solo da M1).

---

## M1 — Scheletro Bevy + Avian

**Obiettivo**: app che compila in WASM, mostra finestra, camera con pan/zoom, corpi con gravità N-body.

### Task

| # | Task | Dettagli |
|---|------|----------|
| 1.1 | **Progetto Cargo** | `cargo init --lib`, dipendenze bevy + avian2d + wasm-bindgen, configurazione target wasm32 |
| 1.2 | **Window + camera** | `DefaultPlugins`, `Camera2d`, risoluzione adattiva |
| 1.3 | **Pan & Zoom camera** | Sistema che trasforma eventi mouse/trackpad in movimento/zoom camera. Destro+drag per pan, rotellina per zoom centrato sul cursore |
| 1.4 | **CelestialBody component** | `{ body_type: enum, mass: f32, radius: f32, color: Color, luminous: bool }` + bundle di spawn |
| 1.5 | **Spawn corpi di prova** | Plugin `DebugSpawn` che crea 3-4 corpi (1 stella centrale, 2 pianeti, 1 asteroide) all'avvio |
| 1.6 | **GravitySystem** | Sistema Bevy che ogni frame: itera coppie con `CelestialBody` + `RigidBody`, calcola F = G*m₁*m₂/(r²+ε²), applica `ExternalForce` su Avian |
| 1.7 | **Verifica WASM** | Compilazione con `wasm-pack`, test su server HTTP locale. Niente thread, niente filesystem |
| 1.8 | **Integrazione Avian base** | Disabilitare gravità globale Avian. `Collider` circolare su ogni corpo. Damping minimo |

### Dipendenze Cargo

```toml
[dependencies]
bevy = "0.15"
avian2d = "0.2"  
serde = { version = "1", features = ["derive"] }
serde_json = "1"
wasm-bindgen = "0.2"
```

### Accettazione M1

- [ ] `cargo build --target wasm32-unknown-unknown` senza errori
- [ ] `wasm-pack build` produce bundle giocabile
- [ ] Aprire HTML → si vede finestra nera con stella + pianeti colorati
- [ ] Pan e zoom funzionano (mouse + trackpad)
- [ ] I pianeti orbitano intorno alla stella per gravità N-body
- [ ] Avian gestisce collisioni (merge/rimbalzo, da decidere dopo)

---

## M2 — Sandbox UI

**Obiettivo**: interfaccia completa per editare corpi e controllare la simulazione.

### Task

| # | Task | Dettagli |
|---|------|----------|
| 2.1 | **Tool system** | `Tool { Select, Add, Move, Delete }` — risorsa Bevy `CurrentTool`. Sistema che intercetta click/tastiera e delega al tool attivo |
| 2.2 | **Selection system** | Click su corpo in raggio di click → memorizza `Entity` in `SelectedBody` resource. Deseleziona con click su sfondo |
| 2.3 | **Add tool** | Click canvas → mostra parametri default in property panel → conferma → spawn corpo |
| 2.4 | **Move tool** | Seleziona corpo + drag → aggiorna `Position` + `LinearVelocity` di Avian (o resetta a corpo fermo se in pausa) |
| 2.5 | **Delete tool** | Click su corpo → rimuove entità e tutti i componenti associati. Conferma opzionale |
| 2.6 | **Timeline** | Play/Pause, Step, Speed slider (0.1×–10×). Controllo `SimulationState` resource + `Time.scale()` |
| 2.7 | **Property Panel** | Pannello laterale (o modale). Mostra: nome, tipo, massa, raggio, posizione, velocità, colore. Campi editabili solo se `SimulationState == Paused` |
| 2.8 | **Minimap** | Rendering alternativo della scena in piccolo angolo, telecamera ortografica separata che mostra tutti i corpi in miniatura |
| 2.9 | **Persistenza JSON** | `LevelSerializer` system. Salva: stato corrente corpi → JSON string. Carica: JSON → despawn corpi attuali → spawn nuovi. Download via browser File API |
| 2.10 | **File dialog browser** | Pulsanti "Salva" (scarica JSON) e "Carica" (file picker → deserializza) usando `wasm-bindgen` + web-sys |
| 2.11 | **Shortcut tastiera** | Spazio = Pause/Resume, 1-4 = tool Select/Add/Move/Delete, Ctrl+S = salva, Ctrl+O = carica |

### Regole sandbox

- Se `SimulationState == Playing`: i tool Add/Move/Delete sono disabilitati (grigi). Property panel in read-only.
- Se `SimulationState == Paused`: tutto abilitato.
- Il cambio tool si può fare sempre (anche in play, per Select).
- Quando si modifica massa/posizione/velocità di un corpo in pausa, al Resume Avian riparte con i nuovi valori.

### UI framework

**Decisione**: `bevy_egui` vs UI custom con nodi Bevy.

- `bevy_egui`: più rapido da sviluppare, ricco di widget, ma stile non nativo Bevy.
- UI Bevy nativa (Nodi/UI): più lavoro ma look integrato.

**Raccomandazione**: iniziare con **`bevy_egui`** per l'UI del sandbox (toolbar, pannelli, slider). È un sandbox, non il gioco finale — la velocità di sviluppo conta più della perfezione estetica. Se poi l'UI finale del gioco sarà diversa, la riscriviamo in UI Bevy nativa.

### Accettazione M2

- [ ] Toolbar con 4 strumenti, cliccabili, cambio con shortcut 1-4
- [ ] Select → click su corpo → property panel mostra dati
- [ ] Add → click canvas → spawn corpo personalizzabile
- [ ] Move → drag corpo fermo → aggiorna posizione
- [ ] Delete → click corpo → rimosso
- [ ] Play/Pause/Step/Speed funzionano
- [ ] Property panel editabile solo in pausa
- [ ] Minimap mostra tutti i corpi
- [ ] Salva JSON → download funziona
- [ ] Carica JSON → scena si ricostruisce

---

## M3 — Illuminazione e rendering

**Obiettivo**: stelle che emettono luce, corpi illuminati con normal map, ombre proiettate.

### Task

| # | Task | Dettagli |
|---|------|----------|
| 3.1 | **LightSource component** | `{ intensity: f32, falloff: f32 }` attaccato alle stelle. Una risorsa tiene conto di tutte le luci attive |
| 3.2 | **Custom 2D material WGSL** | `LightMaterial` — materiale Bevy che accetta: texture diffuse, normal map (opzionale), posizione luce. Calcola illuminazione per-pixel con normale + direzione luce. Per ora senza normal map (usa normale fittizia) |
| 3.3 | **Light system** | Per ogni corpo, trova la stella più vicina. Calcola vettore luce → corpo. Aggiorna uniformi del materiale (direzione luce, intensità) |
| 3.4 | **Ambient light** | Luce ambient globale. Corpi senza stella vicina → solo ambient (10-15%) |
| 3.5 | **Ombre proiettate** | Per ogni corpo non-luminoso, calcola cono d'ombra: raggio corpo + vettore luce → triangolo/poligono di ombra. Renderizzato come mesh semitrasparente scura sul layer sotto i corpi |
| 3.6 | **Culling ombre** | Non renderizzare ombre se il corpo è lontano dalla telecamera o se la luce è debole |
| 3.7 | **Traiettorie passate** | Component `TrajectoryHistory { positions: Vec<Vec2>, max_len: usize }`. Sistema che ogni frame registra posizione corrente (se SIM速度快, campiona ogni N frame). Render come punti connessi, opacità decrescente |
| 3.8 | **Traiettorie predittive** | Sistema che quando un corpo è selezionato, calcola N passi avanti con RK4 (copia lightweight della fisica, senza Avian). Mostra come linea punteggiata/sfumata |
| 3.9 | **Config traiettorie** | Property panel: slider per lunghezza past trails (100-1000) e predittive (50-500). Toggle visibilità |

### Normal map (predisposizione)

- Il materiale WGSL accetta `normal_map: Option<Handle<Image>>`
- Se assente, usa normale di default: `(0.0, 0.0, 1.0)` (frontale) → illuminazione piatta uniforme
- Quando arriveranno le texture normali (M5), basta caricare la texture nel materiale

### Accettazione M3

- [ ] Stelle emettono luce visibile (corpi più vicini alla stella sono più illuminati)
- [ ] Corpi lontani da stelle sono scuri (solo ambient)
- [ ] Ombre proiettate visibili e corrette geometricamente
- [ ] Traiettorie passate visibili e configurabili
- [ ] Traiettorie predittive visibili su corpo selezionato
- [ ] Con 20 corpi + traiettorie, frame rate > 30fps in WASM

---

## M4 — Sfondo & Polish

**Obiettivo**: background stellare procedurale, scorciatoie, rifiniture UX.

### Task

| # | Task | Dettagli |
|---|------|----------|
| 4.1 | **ParallaxPlugin** | 3 layer con fattori di parallasse configurabili. Ogni layer ha un proprio transform moltiplicato per il fattore |
| 4.2 | **Generatore stelle procedurale** | Sistema che all'avvio genera N stelle per ogni layer. Posizioni random in un'area molto più grande della viewport. Seed fisso per riproducibilità (o random ogni volta) |
| 4.3 | **Rendering stelle** | Stelle come punti/luminosità variabile. Layer 1: piccole opache. Layer 2: medie con glow. Layer 3: brillanti + polvere spaziale |
| 4.4 | **Compatibilità trackpad** | Assicurarsi che pan (scroll 2 dita) e zoom (pinch) funzionino sui browser Safari/Chrome/Firefox macOS |
| 4.5 | **Shortcut rimanenti** | Completare mappatura shortcut mancanti dal task 2.11. Tooltip sui pulsanti |
| 4.6 | **Cursor feedback** | Icone cursore diverse per tool: 🔍 per Select, ✚ per Add, ✋ per Move, 🗑 per Delete |

### Accettazione M4

- [ ] 3 layer di parallasse con stelle visibili e movimento differenziato
- [ ] Scrolling trackpad funziona in pan (no zoom involontario)
- [ ] Shortcut tastiera completi
- [ ] Cursori cambiano con il tool attivo

---

## M5 — Texture & Normal Map

**Obiettivo**: sprite con texture diffuse e normal map per volume 3D apparente.

### Task

| # | Task | Dettagli |
|---|------|----------|
| 5.1 | **Texture generator** | Script/funzione Rust (o Python CLI) che genera PNG + normal map per ogni tipo di pianeta. Usa rumore di Perlin/simplex per superfici rocciose, gradienti per gassosi, glow per stelle |
| 5.2 | **Normal map da rumore** | Per ogni texture diffusa, calcola normal map: derivata dell'altezza (dal rumore) → vettore normale normalizzato codificato in RGB |
| 5.3 | **Asset pipeline** | Carica le texture generate in Bevy come `Handle<Image>`. Opzionale: embed negli asset WASM |
| 5.4 | **Shader update** | Collega la normal map al materiale custom. Se presente, usa per illuminazione per-pixel. Se assente, normale frontale |
| 5.5 | **Varianti** | Almeno 4 varianti: roccioso, gassoso, ghiaccio, terrestre. Stelle con glow texture |

### Accettazione M5

- [ ] I pianeti mostrano texture con dettagli superficiali
- [ ] La luce della stella interagisce con la normal map → volume percepito
- [ ] I pianeti ruotano su sé stessi (opzionale, effetto visivo)
- [ ] Cambiando posizione della stella, l'illuminazione sulla normal map cambia

---

## Ordine consigliato

```
M1 ──┬── M2 ──→ fine sandbox (editing + simulazione)
     ├── M3 ──→ fine illuminazione
     └── M4 ──→ fine background
M5 (dopo M3) ──→ fine texture
```

M2 ha priorità dopo M1 perché dà l'interattività. M3 e M4 in parallelo. M5 è l'ultima.

---

## Rischi

| Rischio | Impatto | Mitigazione |
|---------|---------|-------------|
| Performance WASM con N-body + shader | Medio | Testare presto con 20+ corpi. Ottimizzare softening e passo |
| `bevy_egui` in WASM ha problemi di input | Medio | Verificare subito dopo M1. Se problematico → UI Bevy nativa |
| Avian2D non supporta bene WASM | Basso | Verificare compile. Se blocchi → Rapier2D |
| Shader WGSL non compatibile con tutti i browser | Medio | Testare su Chrome, Firefox, Safari (WebGPU) |
| File API browser per JSON | Basso | `wasm-bindgen` ha supporto maturo per download/upload |
