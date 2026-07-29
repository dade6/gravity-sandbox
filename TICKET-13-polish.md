# Ticket 13 — Polish: shortcut, cursori, input scheme

## Descrizione
Rifiniture UX finali: completare la mappatura delle scorciatoie, feedback visivo dei cursori, tooltip, verifica compatibilità trackpad/mouse.

## Cosa deve fare
### 1. Shortcut completi
- 1-4: Select/Add/Move/Delete (già implementato in parte)
- Ctrl+S: Salva livello
- Ctrl+O: Carica livello
- Spazio: Play/Pause
- . o Freccia Destra: Step
- +/-: velocità
- Mostrare le scorciatoie nei tooltip/bottoni HTML

### 2. Cursori personalizzati
- Select → default/pointer
- Add → crosshair
- Move → grab (hand)
- Delete → not-allowed o icona
- Usare CSS sull'HTML overlay o `cursor` property sul canvas

### 3. Tooltip
- Hover su bottoni → mostrano nome + shortcut
- Esempio: "Select (1) — Click to select a body"

### 4. Verifica compatibilità
- Pan con 2 dita trackpad
- Zoom con pinch
- Click destro per pan su mouse
- Rotellina per zoom

## Test di verifica
- [ ] Ogni shortcut funziona come descritto
- [ ] Cursore cambia con il tool attivo
- [ ] Pan/Zoom funziona sia con mouse che trackpad
- [ ] Tooltip visibili su hover dei bottoni

## Bloccanti: nessuno
