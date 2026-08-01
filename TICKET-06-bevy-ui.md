# Ticket 06 — Interfaccia Utente nativa Bevy Engine

## Obiettivo
Sostituire l'attuale overlay HTML/CSS con una UI realizzata interamente con il sistema UI nativo di Bevy Engine (componenti `Node`, `Button`, `Text`, `ImageNode`). La UI deve includere toolbar, timeline, property panel e minimap, funzionando su tutti i target inclusi Safari + WebGL2.

## Contesto
- Il progetto usa Bevy 0.19 con feature flags: `bevy_ui`, `bevy_ui_render`, `bevy_ui_widgets`
- L'UI nativa Bevy NON funziona su Safari WebGL2 — questo ticket deve risolvere il problema o documentare la soluzione
- Il codice attuale ha un overlay HTML funzionante ma l'utente richiede UI nativa Bevy

## Architettura

### 1. Toolbar (in alto)
- Barra orizzontale semi-trasparente con bottoni: Select (1), Add (2), Move (3), Delete (4)
- Bottone attivo evidenziato con colore diverso
- Versione del progetto a destra

### 2. Timeline (in basso)
- Barra orizzontale con bottoni: Play/Pause (Spazio), Step (.)
- Display velocità corrente (es. "Speed: 1.0×")
- Stato Play/Pause riflesso nel bottone

### 3. Property Panel (laterale)
- Pannello che appare quando un corpo è selezionato
- Mostra: nome, tipo, massa, raggio, posizione (x,y), velocità (vx,vy), colore
- Campi editabili solo quando simulazione in pausa
- Scompare quando nessun corpo selezionato

### 4. Minimap
- Render target secondario 150×150
- Mostra tutti i corpi come punti colorati
- Rettangolo viewport che segue la camera principale
- Click → centra camera principale

## Sub-task previsti — STATO ATTUALE

### ST-01: Diagnostica rendering UI su Safari WebGL2 ✅ RISOLTO
- La diagnosi originale ("Safari-only") era **sbagliata**: la causa reale era un
  B0001 (due query `&mut Text` in `update_timeline_buttons` + conflitto in
  `update_parallax`), risolto con `ParamSet`. La UI Bevy renderizza
  correttamente su Safari iOS (verificato dall'utente, v0.13.x).

### ST-02: Creazione struttura UI di base ✅
- Overlay HTML rimosso (index.html = solo canvas + version badge)
- Toolbar + timeline + property panel + delete dialog in Bevy UI (`ui.rs`)

### ST-03: Toolbar funzionante ✅
- Bottoni Select/Add/Move/Delete con highlight del tool attivo
- Shortcut 1-4 riflesse visivamente; Select sempre attivo, gli altri solo in pausa

### ST-04: Timeline funzionante ✅
- Play/Pause/Step/Speed con `SimulationState` + `Time<Virtual>` + `Time<Physics>` (Avian)
- Display velocità aggiornato in tempo reale

### ST-05: Property Panel ✅
- Campi editabili solo in pausa, grigi (readonly) in play
- Scrittura modifiche al corpo via `sync_property_input_to_body`
- Scompare quando deselezionato

### ST-06: Minimap integrata ✅
- Camera secondaria con render target + container UI Bevy con `ImageNode`
- Viewport rect + click-to-center

### ST-07: Verifica multipiattaforma ⚠️ PARZIALE
- ✅ iPhone Safari iOS (test T05 + UI verificati dall'utente)
- ⏳ Chrome desktop, Firefox desktop, Safari macOS — da verificare

## Test di verifica (per ogni sub-task)
Ogni ST deve avere:
- [x] Compilazione senza errori (native + WASM)
- [x] Test visivo su browser di riferimento — iPhone Safari ✅, altri da fare (ST-07)
- [x] Test di regressione: keyboard shortcut funzionano ancora
- [x] Test di regressione: simulazione gravità funziona
- [x] WASM build produce output giocabile

## Priorità
1. ST-01 (diagnostica Safari) — sblocca tutto il resto
2. ST-02 + ST-03 (struttura + toolbar)
3. ST-04 (timeline)
4. ST-05 (property panel)
5. ST-06 (minimap)
6. ST-07 (verifica)

## Bloccanti
- Nessuno: il progetto ha già tutti i sistemi (tool, selezione, timeline, minimap) implementati con HTML overlay. Questo ticket li migra a Bevy UI nativa.
