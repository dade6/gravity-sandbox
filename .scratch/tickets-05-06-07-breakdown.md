# Gravity Sandbox — Ticket 05/06/07 Micro Task Breakdown

## Ticket 05 — Selezione + Property Panel

### 5.1 — Raycast/Selezione corpo
- [ ] Componente `Selectable` marker
- [ ] Sistema `selection_system`: click sinistro → trova corpo più vicino sotto il cursore
- [ ] Resource `SelectedBody(Option<Entity>)`
- [ ] Deseleziona con click su sfondo
- [ ] Highlight visivo (bordo/cornice) sul corpo selezionato
- [ ] Verifica: click su corpo → `SelectedBody` popolato; click sfondo → cleared

### 5.2 — Property Panel UI
- [ ] Pannello Bevy UI laterale/destra che mostra proprietà corpo selezionato
- [ ] Campi: tipo, massa, raggio, colore, posizione (x,y), velocità (x,y)
- [ ] Read-only durante Play, editabile in Pausa
- [ ] Nascondi pannello se nessun corpo selezionato
- [ ] Verifica: click corpo → pannello mostra dati; deseleziona → pannello scompare

## Ticket 06 — Strumenti Add/Move/Delete

### 6.1 — Tool system
- [ ] Resource `CurrentTool { Select, Add, Move, Delete }`
- [ ] Toolbar buttons attuali già presenti (mancano marker component)
- [ ] Shortcut 1-4 per cambiare tool
- [ ] Disabilita Add/Move/Delete durante Play
- [ ] Verifica: cambio tool via tastiera aggiorna `CurrentTool`

### 6.2 — Add tool
- [ ] Click canvas → spawn corpo con parametri default nel punto click
- [ ] Apri Property Panel per modifica parametri (solo in pausa)
- [ ] Verifica: click su spazio vuoto con Add tool → nuovo corpo appare

### 6.3 — Move tool
- [ ] Click su corpo → seleziona
- [ ] Drag (mouse/touch) → aggiorna posizione
- [ ] Se in pausa: sposta corpo immediatamente
- [ ] Se in play: bloccato (torna a Select)
- [ ] Verifica: drag corpo in pausa → posizione aggiornata

### 6.4 — Delete tool
- [ ] Click su corpo → rimuove entità e componenti Avian
- [ ] Messaggio di conferma (opzionale)
- [ ] Verifica: click corpo con Delete tool → corpo scompare

## Ticket 07 — Minimap

### 7.1 — Render minimap
- [ ] Seconda camera ortografica in angolo basso-destro
- [ ] Mostra tutti i corpi come punti colorati
- [ ] Sfondo semitrasparente
- [ ] Viewport rect che mostra area camera principale
- [ ] Verifica: minimap visibile con punti colorati

### 7.2 — Interazione minimap
- [ ] Click su minimap → centra camera principale su quel punto
- [ ] Verifica: click minimap → camera si muove
