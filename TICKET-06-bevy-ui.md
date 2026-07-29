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

## Sub-task previsti

### ST-01: Diagnostica rendering UI su Safari WebGL2
- Identificare perché Bevy UI non renderizza su Safari WebGL2
- Testare con feature flags minimi: `bevy_ui` + `bevy_ui_render` + `bevy_ui_widgets`
- Provare configurazioni alternative: rimozione `ZIndex`, diverso ordine rendering, camera separata UI
- Documentare la soluzione trovata

### ST-02: Creazione struttura UI di base
- Rimuovere dipendenze HTML overlay non necessarie
- Creare struttura Node principale con toolbar + timeline + panel
- Usare esclusivamente componenti Bevy UI (Button, Node, Text, BackgroundColor)
- Verificare compilazione incrociata (native + WASM)

### ST-03: Toolbar funzionante
- Bottoni Select/Add/Move/Delete con highlight del tool attivo
- Collegamento al sistema CurrentTool (già esistente)
- Keyboard shortcut 1-4 riflessa visualmente
- Tooltip con nome tool

### ST-04: Timeline funzionante
- Bottone Play/Pause che cambia icona/testo
- Bottone Step
- Display velocità aggiornato in tempo reale
- Collegamento a SimulationState e Time<Virtual>

### ST-05: Property Panel
- Pannello laterale con campi editabili
- Read-only quando in play
- Scrittura modifiche al corpo selezionato
- Scomparsa quando deselezionato

### ST-06: Minimap integrata
- Render target + camera secondaria
- UI container con ImageNode
- Aggiornamento bounding box corpi
- Click per centrare camera principale

### ST-07: Verifica multipiattaforma
- Test su Chrome (desktop)
- Test su Firefox (desktop)
- Test su Safari macOS
- Test su Safari iOS
- Test su WASM build

## Test di verifica (per ogni sub-task)
Ogni ST deve avere:
- [ ] Compilazione senza errori
- [ ] Test visivo su browser di riferimento
- [ ] Test di regressione: keyboard shortcut funzionano ancora
- [ ] Test di regressione: simulazione gravità funziona
- [ ] WASM build produce output giocabile

## Priorità
1. ST-01 (diagnostica Safari) — sblocca tutto il resto
2. ST-02 + ST-03 (struttura + toolbar)
3. ST-04 (timeline)
4. ST-05 (property panel)
5. ST-06 (minimap)
6. ST-07 (verifica)

## Bloccanti
- Nessuno: il progetto ha già tutti i sistemi (tool, selezione, timeline, minimap) implementati con HTML overlay. Questo ticket li migra a Bevy UI nativa.
