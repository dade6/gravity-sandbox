# Ticket 11 — Traiettorie: passate e predittive

## Descrizione
Ogni corpo mostra una scia della sua posizione passata e una linea predittiva del percorso futuro. La lunghezza di entrambe è configurabile dall'utente.

## Cosa deve fare
### 1. Traccia passata (history trail)
- Componente `TrajectoryHistory { positions: Vec<Vec2>, max_len: usize }`
- Sistema che ogni N frame campiona la posizione corrente e la inserisce in coda
- Render come linea connessa, opacità decrescente (più vecchio = più trasparente)
- Lunghezza default: 500 campioni, configurabile 100-2000

### 2. Traccia predittiva (prediction trail)
- Sistema che per il corpo selezionato (solo se c'è una selezione), calcola N passi futuri
- Integrazione rapida con RK4 (Runge-Kutta 4° ordine, senza Avian, solo fisica N-body)
- Render come linea punteggiata/sfumata in colore diverso dalla history (es. verde)
- Lunghezza default: 200 passi, configurabile 50-1000

### 3. Configurazione
- Slider/controlli nell'HTML overlay: "Past trail length", "Prediction steps"
- Toggle visibilità traiettorie (on/off)
- I valori di configurazione sono esposti via wasm-bindgen

## Test di verifica
- [ ] Corpo in movimento → scia passata visibile dietro di esso
- [ ] Corpo selezionato → linea predittiva visibile davanti
- [ ] Modificando lunghezza past trails → la scia si allunga/accorcia
- [ ] Toggle off → traiettorie scompaiono
- [ ] La linea predittiva segue approssimativamente l'orbita reale (errore < 10% dopo 5 orbite)

## Bloccanti
- Ticket 05 (selezione corpo, per traccia predittiva su corpo selezionato)
- Ticket 09 (illuminazione, per layer di rendering)
