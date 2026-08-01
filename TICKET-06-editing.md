# Ticket 06 — Editing: Add, Move, Delete

## Descrizione
Permettere all'utente di aggiungere nuovi corpi celesti, spostare quelli esistenti e cancellarli, attraverso gli strumenti Add/Move/Delete. Tutte le operazioni sono permesse solo a simulazione in pausa.

## Cosa deve fare
### 1. Add tool
- Click sinistro sul canvas in tool Add → spawna un nuovo corpo nella posizione cliccata
- Il corpo ha parametri default (massa=100, raggio=15, colore=grigio, tipo=Planet)
- Subito dopo lo spawn, il corpo viene selezionato automaticamente e il property panel si apre per permettere la modifica dei parametri
- Aggiungere `ConstantForce(Vec2::ZERO)`, `Mass(valore)`, `Collider::circle(raggio)`, `RigidBody::Dynamic`
- Usare un colore casuale/di default per distinguerlo

### 2. Move tool
- Click + drag su un corpo in tool Move → segue il cursore
- Rilasciando il mouse, la posizione viene aggiornata
- Se la simulazione è in pausa, la velocità lineare viene resettata a zero
- Mostrare un feedback visivo (il corpo diventa leggermente trasparente durante il drag)

### 3. Delete tool
- Click su un corpo in tool Delete → mostra conferma (dialog **Bevy UI** "Eliminare [nome]?")
- Conferma → despawn entity + cleanup (rimuovi mesh, material, collider, rigidbody)
- Annulla → deseleziona

### 4. Regole
- Tutti i tool sono disabilitati quando `SimulationState.paused == false`
- I bottoni Add/Move/Delete mostrano stato disabilitato (opacity ridotta via `BTN_DISABLED`) quando in play — implementato in `handle_ui_buttons`
- Select resta sempre attivo

## Test di verifica
- [ ] Metti in pausa, tool Add → click canvas → nuovo corpo appare, property panel mostra i suoi dati
- [ ] Tool Move → drag corpo → si sposta, posizione aggiornata nel panel
- [ ] Tool Delete → click corpo → dialog conferma (Bevy UI) → conferma → corpo scompare
- [ ] In play mode → click su canvas non fa nulla, bottoni Add/Move/Delete sembrano disabilitati (opacity ridotta)

## Bloccanti
- Ticket 05 (serve il tool system e la selezione) — ✅ completato
