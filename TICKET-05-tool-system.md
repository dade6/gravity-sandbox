# Ticket 05 — Tool system + Selezione + Property Panel

## Descrizione
Implementare il sistema di selezione dei corpi celesti e la visualizzazione delle loro proprietà. Quando l'utente clicca su un corpo, deve essere evidenziato e le sue proprietà devono apparire in un pannello. I tool (Select/Add/Move/Delete) devono essere commutabili via tastiera e via bottoni della UI nativa Bevy.

> **Vincolo progettuale**: tutta la UI è **Bevy UI** (o altra UI Rust compatibile Bevy). **Nessun overlay HTML** — vedi doc header di `src/systems/ui.rs`.

## Cosa deve fare
### 1. Tool system (Rust/ECS)
- Creare risorsa `CurrentTool` con enum: `{ Select, Add, Move, Delete }`
- Sistema che intercetta tasti 1/2/3/4 e cambia il tool attivo
- Sistema che intercetta click sinistro su un corpo (hit test basato su distanza dal centro del corpo)
- Risorsa `SelectedBody(Option<Entity>)` per tracciare il corpo selezionato

### 2. Highlight visivo (Rust/ECS)
- Quando un corpo è selezionato, disegnare un bordo/cerchio attorno ad esso
- Implementato con `Gizmos` (cerchio sul corpo selezionato)

### 3. Property Panel (Bevy UI)
- Pannello Bevy UI a destra che mostra: nome, tipo, massa, raggio, posizione (x,y), velocità (vx,vy), colore
- Il pannello si mostra quando un corpo è selezionato, si nasconde quando la selezione è vuota
- I campi sono editabili solo quando `SimulationState.paused == true`:
  - In play mode i campi appaiono "grigi" (readonly visivo via `TextColor`) e le modifiche non vengono applicate
  - In pausa i campi tornano attivi e le modifiche vengono sincronizzate al corpo
- Campo tipo (`_type`) è sempre read-only
- Hint di stato nel pannello: "✏️ Edit mode" (pausa) / "⏸ Pause to edit" (play)

### 4. Comunicazione UI ↔ ECS
- Tutta la comunicazione avviene in Rust tra i sistemi Bevy (nessun bridge wasm-bindgen richiesto):
  - `update_property_panel` — scrive i valori correnti del corpo nei campi
  - `sync_property_input_to_body` — legge le modifiche dai campi e le applica al corpo (solo in pausa)
  - `handle_ui_buttons` — gestisce click/hover sui bottoni toolbar/timeline
  - `sync_tool_buttons` — evidenzia il tool attivo nella toolbar

## File di riferimento
- `src/systems/tools.rs` — CurrentTool, shortcut tastiera, add/move/delete
- `src/systems/selection.rs` — SelectedBody, hit test click, highlight Gizmos
- `src/systems/ui.rs` — spawn toolbar/timeline/property panel, sync input→body, readonly in play

## Test di verifica (da eseguire in browser, native e WASM)
- [ ] Tasto 1 → tool Select attivo, bottone "Select (1)" nella toolbar si illumina
- [ ] Click su un corpo → highlight visibile (cerchio attorno), property panel mostra i dati
- [ ] Click su sfondo → deseleziona, panel si nasconde
- [ ] In play mode, i campi property panel sono grigi (readonly visivo) e le modifiche non si applicano
- [ ] In pausa, modificare "mass" nel panel → corpo cambia massa
- [ ] Property panel mostra: nome, tipo, massa, raggio, pos (x,y), vel (vx,vy), colore hex

## Bloccanti: nessuno
