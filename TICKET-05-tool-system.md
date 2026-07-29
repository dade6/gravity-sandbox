# Ticket 05 — Tool system + Selezione + Property Panel

## Descrizione
Implementare il sistema di selezione dei corpi celesti e la visualizzazione delle loro proprietà. Quando l'utente clicca su un corpo, deve essere evidenziato e le sue proprietà devono apparire in un pannello. I tool (Select/Add/Move/Delete) devono essere commutabili via tastiera e via bottoni HTML overlay.

## Cosa deve fare
### 1. Tool system (Rust/ECS)
- Creare risorsa `CurrentTool` con enum: `{ Select, Add, Move, Delete }`
- Sistema che intercetta tasti 1/2/3/4 e cambia il tool attivo
- Sistema che intercetta click sinistro su un corpo (hit test basato su distanza dal centro del corpo)
- Risorsa `SelectedBody(Option<Entity>)` per tracciare il corpo selezionato

### 2. Highlight visivo (Rust/ECS)
- Quando un corpo è selezionato, disegnare un bordo/cerchio attorno ad esso
- Si può usare un `Gizmos` o un `Sprite` aggiuntivo come child dell'entità

### 3. Property Panel (HTML overlay + wasm-bindgen)
- Esporre funzione Rust via `#[wasm_bindgen]` che restituisce le proprietà del corpo selezionato come JSON
- JS HTML overlay aggiorna un pannello laterale con: nome, tipo, massa, raggio, posizione (x,y), velocità (vx,vy), colore
- I campi sono editabili solo quando `SimulationState.paused == true`
- Esporre una funzione `set_body_property(key, value)` che aggiorna una proprietà nel corpo selezionato

### 4. Comunicazione JS ↔ Rust
- Esportare le funzioni: `select_tool(tool_index)`, `get_selected_body_json() -> String`, `set_body_property(key, value)`
- Usare `wasm-bindgen` per collegare i bottoni HTML alle funzioni Rust

## Test di verifica
- [ ] Tasto 1 → tool Select attivo, bottone HTML "Select (1)" si illumina di blu
- [ ] Click su un corpo → highlight visibile (cerchio attorno), property panel mostra i dati
- [ ] Click su sfondo → deseleziona, panel si nasconde
- [ ] In play mode, i campi property panel sono readonly (grigi)
- [ ] In pausa, modificare "mass" nel panel → corpo cambia massa
- [ ] `get_selected_body_json()` chiamata da console browser → restituisce JSON valido con tutte le proprietà

## Bloccanti: nessuno
