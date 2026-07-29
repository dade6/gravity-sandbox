# Ticket 08 — JSON Persistence (Save/Load)

## Descrizione
Permettere di salvare e caricare i livelli in formato JSON tramite browser File API (download/upload).

## Cosa deve fare
### 1. Serializzazione
- Struttura JSON: `{ name, gravity_constant, bodies: [{ id, body_type, mass, radius, position, velocity, color, luminous }] }`
- Sistema `LevelSerializer` che itera tutti i corpi con `CelestialBody` e produce JSON
- Usa `serde_json` per serializzare

### 2. Salvataggio (Download)
- Pulsante "Salva" nell'HTML overlay → chiamata a funzione Rust `save_level()`
- `save_level()` produce JSON string, la passa a JS che crea un download via Blob + URL.createObjectURL
- Il file si chiama `livello-<timestamp>.json`

### 3. Caricamento (Upload)
- Pulsante "Carica" nell'HTML overlay → file picker (input type=file)
- JS legge il file come text, passa a Rust `load_level(json_string)`
- `load_level()` deserializza, despawna tutti i corpi attuali, spawna i nuovi
- La simulazione si ferma (pausa) dopo il caricamento

### 4. Gestione errori
- Se il JSON è malformato, mostra messaggio di errore nel badge versione
- Se mancano campi obbligatori, usa valori di default

## Test di verifica
- [ ] Pulsante Salva → download file `.json` con tutti i corpi
- [ ] File scaricato è JSON valido e contiene tutti i campi
- [ ] Pulsante Carica → seleziona file → corpi attuali spariscono, nuovi appaiono
- [ ] Dopo caricamento, la simulazione è in pausa
- [ ] Caricare un file invalido → messaggio di errore (non crash)

## Bloccanti
- Ticket 05 (selezione, per feedback visivo)
- Ticket 06 (add/delete, per consistenza)
