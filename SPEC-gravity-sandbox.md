# Gravity Sandbox — Specifica di Progetto

> Sandbox per level design di uno shooter 2D spaziale con simulazione gravitazionale N-body,
> illuminazione solare con normal map, e ombre proiettate.
> Basato su **Bevy Engine** + **Avian2D**, compilato in **WASM** per browser.

---

## 1. Stack tecnologico

| Componente | Scelta | Note |
|------------|--------|------|
| **Engine** | Bevy (ultima stable) | ECS puro, compile in WASM nativamente |
| **Fisica** | Avian2D | Puro Rust, nativo ECS, collisioni + forze custom |
| **Target** | WASM + Web browser | `wasm-pack`, asset incorporati nel binary |
| **Linguaggio** | Rust | — |
| **Shading** | WGSL (shader custom 2D) | Per illuminazione con normal map e ombre |
| **Persistenza** | JSON | Salvataggio/caricamento livelli |
- **Vincolo UI**: tutta l'interfaccia usa il sistema UI nativo di Bevy Engine (Node/Button/Text). Niente HTML overlay o librerie esterne.

---

## 2. Visuale

- **2D puro** — tutto è renderizzato su un piano 2D
- **Camera**: pan (click destro + drag / scroll 2 dita trackpad) e zoom (rotellina / pinch), compatibile mouse + trackpad
- **Parallasse**: 3 livelli di sfondo stellare procedurale
  1. Sfondo profondo — stelle fisse, piccole, bassa opacità, velocità 0
  2. Medio — stelle sparse, media grandezza, parallasse lento
  3. Primo piano — stelle brillanti, polvere spaziale, parallasse più veloce
- **Generazione procedurale delle stelle** di sfondo con seed casuale

---

## 3. Illuminazione

Sistema di luce 2D custom con shader WGSL su materiale Bevy.

### 3.1 Modello di luce

- **Ogni stella** è una `PointLight2D` che emette luce radialmente
- Un corpo celeste è illuminato dalla **stella più vicina**
- Corpi lontani da qualsiasi stella → luce ambient minima (10-15%)
- **Normal map**: sistema predisposto, da integrare quando arriveranno le texture
- **Ombre proiettate**: ogni pianeta proietta un cono d'ombra nella direzione opposta alla stella più vicina (poligono scuro semitrasparente)

### 3.2 Pipeline rendering

1. **Passo 1**: render corpo con shader materiale custom → legge normal map + posizione luce → calcola illuminazione per-pixel
2. **Passo 2**: overlay ombre → poligoni scuri semitrasparenti in base a posizione stella-corpo
3. **Passo 3**: UI e traiettorie in cima, non influenzate dalla luce

---

## 4. Fisica e simulazione

### 4.1 Gravitazione

- **N-body O(n²)** — ogni corpo risente della gravità di tutti gli altri
- Forza gravitazionale: F = G * m₁ * m₂ / (r² + ε²) (con softening ε per singolarità)
- Integrazione: **Velocity Verlet** (simile a quella di Avian)
- Gravità globale Avian disabilitata, sostituita da sistema custom `GravitySystem`

### 4.2 Avian2D integration

- Ogni corpo celeste ha `RigidBody` + `Collider` di Avian
- Component custom `CelestialBody { mass, body_type, radius }`
- `GravitySystem` applica `ExternalForce` su ogni corpo ogni frame
- Le collisioni tra corpi sono gestite da Avian (da definire: fusione, rimbalzo, distruzione)

### 4.3 Body types

| Tipo | Massa | Gravità subita | Note |
|------|-------|----------------|------|
| **Stella** | Molto grande (10⁴–10⁶) | Sì (ma può essere bloccata opzionalmente) | Emette luce, glow, non collassabile |
| **Pianeta** | Media (1–10³) | Sì | Texture + normal map |
| **Luna** | Piccola (0.1–1) | Sì | Orbita intorno a pianeta |
| **Asteroide** | Molto piccola (<0.1) | Sì | Tanti, forma irregolare |
| **Astronave** | Piccola (0.5–5) | Sì | Controllata dal giocatore nel runtime |

---

## 5. Sandbox — Strumenti e UI

### 5.1 Interazione

| Azione | Mouse | Trackpad |
|--------|-------|----------|
| **Pan** | Click destro + drag | Scroll 2 dita |
| **Zoom** | Rotellina (centrato sul cursore) | Pinch |
| **Select** | Click sinistro | Tap |
| **Move** | Click sinistro + drag | Tap + drag |
| **Add** | Tool Add + click canvas | Tool Add + tap |
| **Delete** | Tool Delete → click corpo | Tool Delete → tap corpo |

### 5.2 Tool modalità

1. **Select** — clicca un corpo, mostra proprietà
2. **Add** — clicca canvas, posiziona un nuovo corpo (form parametri)
3. **Move** — trascina un corpo, aggiorna posizione
4. **Delete** — clicca un corpo per rimuoverlo

**Regola**: l'editing (Add/Move/Delete/modify parametri) è abilitato **solo a simulazione in pausa**.

### 5.3 Timeline

- **Play / Pause** (toggle, shortcut Space)
- **Step avanti** (un frame di simulazione)
- **Velocità simulazione** slider 0.1× – 10×

### 5.4 UI elements

- **Toolbar** — orizzontale in alto o verticale a sinistra, con icone strumenti
- **Property Panel** — laterale, mostra parametri del corpo selezionato (massa, posizione, velocità, raggio, tipo, colore)
- **Minimap** — in basso a destra, mostra la posizione di tutti i corpi su scala ridotta
- **Traiettorie** — toggle per mostrarle/nasconderle
- **Shortcuts**: Spazio = Pause/Resume, 1–4 = strumenti

### 5.5 Traiettorie

Ogni corpo può visualizzare:
1. **Traccia passata** — storia delle ultime N posizioni (configurabile: 100–1000 campioni)
2. **Traccia predittiva** — proiezione futura calcolata con integrazione rapida (RK4, configurabile: 50–500 passi)

Lunghezza configurabile dall'utente per entrambi.

### 5.6 Pan e Zoom

- **Zoom**: rotellina mouse o pinch trackpad, centrato sulla posizione del cursore
- **Pan**: click destro + drag o scroll 2 dita trackpad
- Il browser traduce pinch-to-zoom in eventi `wheel` con `ctrlKey`

---

## 6. Persistenza

### 6.1 Formato file (JSON)

```json
{
  "name": "livello-001",
  "gravity_constant": 1000.0,
  "bodies": [
    {
      "id": "sole-1",
      "body_type": "star",
      "mass": 100000.0,
      "radius": 50.0,
      "position": [0.0, 0.0],
      "velocity": [0.0, 0.0],
      "color": "#ffdd44",
      "luminous": true
    },
    {
      "id": "pianeta-1",
      "body_type": "planet",
      "mass": 100.0,
      "radius": 20.0,
      "position": [300.0, 0.0],
      "velocity": [0.0, 100.0],
      "color": "#4488ff",
      "luminous": false
    }
  ]
}
```

### 6.2 Operazioni

- **Salva** → serializza scena corrente in JSON → download nel browser (o File API)
- **Carica** → file picker → deserializza → sostituisce scena corrente

---

## 7. Architettura Bevy (moduli)

```
gravity-sandbox/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point, app builder
│   ├── components/
│   │   ├── mod.rs
│   │   ├── celestial.rs     # CelestialBody component
│   │   └── trajectory.rs    # TrajectoryHistory, TrajectoryConfig
│   ├── systems/
│   │   ├── mod.rs
│   │   ├── gravity.rs       # N-body gravity system
│   │   ├── collision.rs     # Collision handler (Avian events)
│   │   └── cleanup.rs       # Remove bodies safely
│   ├── sandbox/
│   │   ├── mod.rs
│   │   ├── camera.rs        # Pan & Zoom
│   │   ├── tools.rs         # Select, Add, Move, Delete
│   │   ├── ui.rs            # Toolbar, PropertyPanel, Minimap
│   │   ├── timeline.rs      # Play/Pause/Step/Speed
│   │   └── trajectory.rs    # Trajectory rendering
│   ├── rendering/
│   │   ├── mod.rs
│   │   ├── light.rs         # PointLight2D component, light system
│   │   ├── shadow.rs        # Projected shadow polygons
│   │   └── parallax.rs      # 3-layer procedural star background
│   ├── shaders/
│   │   └── light_material.wgsl  # Custom 2D material with normal map lighting
│   ├── persistence/
│   │   ├── mod.rs
│   │   └── level.rs         # Save/Load JSON logic
│   └── assets/              # Sprite placeholder generator
```

---

## 8. Milestone implementative

### M1 — Scheletro Bevy + Avian
- [ ] Progetto Cargo con Bevy + Avian2D
- [ ] Window + camera + sistema di pan/zoom
- [ ] CelestialBody component + spawn corpi di prova
- [ ] Sistema N-body gravity + integrazione Avian
- [ ] WASM compile test (`wasm-pack`)

### M2 — Interazione sandbox
- [ ] Tool: Select, Add, Move, Delete
- [ ] Timeline: Play/Pause/Step/Speed
- [ ] Property Panel (visualizzazione parametri)
- [ ] Modifica corpi solo in pausa
- [ ] Persistenza JSON (salva/carica)
- [ ] Minimap

### M3 — Illuminazione e rendering
- [ ] Custom WGSL shader per luce 2D
- [ ] PointLight2D dal sistema stellare
- [ ] Ombre proiettate dai pianeti
- [ ] Traiettorie passate e predittive (configurabili)

### M4 — Sfondo e polish
- [ ] Parallasse procedurale 3 livelli
- [ ] Generatore di stelle di sfondo
- [ ] Generazione placeholder geometrici
- [ ] Shortcut tastiera
- [ ] Compatibilità trackpad

### M5 — Texture e normal map
- [ ] Asset loader per texture diffuse
- [ ] Normal map integration nello shader
- [ ] Varianti di texture per tipo corpo

---

## 9. Decisioni aperte (rimandate)

| Decisione | Da decidere |
|-----------|-------------|
| Cosa succede quando due corpi collidono | Merge, rimbalzo, distruzione? |
| UI framework Bevy | `bevy_egui` o UI custom con nodi Bevy |
| Astronave nel sandbox | Solo parametri statici o AI-path test? |
| Multiplayer/co-op | Fuori scope iniziale |

---

## 10. Vincoli tecnici

- **WASM**: niente thread (`std::thread`), niente file system nativo (usare browser File API via `wasm-bindgen`)
- **Bevy WASM**: asset incorporati via `embed` o caricati a runtime con richieste HTTP
- **Performance N-body**: O(n²) con 20 corpi è trascurabile; con 100+ valutare ottimizzazioni (Barnes-Hut o passo variabile)
- **Normal map**: shader WGSL predisposto ma testabile solo con texture di prova finché non generiamo le vere

---

## 11. Glossario

| Termine | Significato |
|---------|-------------|
| **N-body** | Simulazione gravitazionale dove ogni corpo interagisce con tutti gli altri |
| **Softening (ε)** | Costante che evita la singolarità gravitazionale a distanza zero |
| **Velocity Verlet** | Metodo di integrazione numerica conservativo per sistemi fisici |
| **PointLight2D** | Luce che si irradia da un punto nello spazio 2D |
| **Normal map** | Texture che codifica la direzione della superficie per simulare volume |
| **Parallasse** | Tecnica di profondità simulata dove gli oggetti più lontani si muovono più lentamente |
| **WGSL** | WebGPU Shading Language — linguaggio di shader per WebGPU |
