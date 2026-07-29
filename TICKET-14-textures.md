# Ticket 14 — Texture + Normal Map procedurali

## Descrizione
Sostituire i placeholder geometrici con texture diffuse e normal map generate proceduralmente per ogni tipo di pianeta, integrate nello shader lighting.

## Cosa deve fare
### 1. Generatore texture
- Funzione Rust che genera immagini PNG per ogni tipo di corpo:
  - **Roccioso**: rumore Perlin con colori terra/marte (marrone, rosso, ocra)
  - **Gassoso**: gradienti orizzontali con bande (simile a Giove/Saturno)
  - **Ghiaccio**: bianco/blu con vene chiare
  - **Stella**: gradiente radiale caldo (giallo/arancione/bianco)
- Salvare come `Handle<Image>` in Bevy (creare immagini via `Image` API o caricare file generati)

### 2. Generatore normal map
- Per ogni texture diffusa, calcolare la normal map dalla derivata dell'altezza (height map)
- La height map si ricava dal rumore usato per la texture
- Normal map codificata in RGB: R = normale X, G = normale Y, B = normale Z

### 3. Integrazione shader
- Aggiornare lo shader WGSL del Ticket 09 per usare la normal map
- Se normal map presente → illuminazione per-pixel con volume 3D
- Se assente → normale frontale (fallback)

### 4. Asset pipeline
- Le texture sono generate in un sistema di startup e inserite in `Assets<Image>`
- Opzionale: ruotare il corpo su sé stesso per mostrare la texture da diverse angolazioni

## Test di verifica
- [ ] Pianeta roccioso → texture con dettagli superficiali visibili
- [ ] Pianeta gassoso → bande colorate orizzontali
- [ ] Stella → glow caldo (giallo-arancione)
- [ ] Con luce attiva, la normal map produce ombreggiatura volumetrica (parte in ombra e parte illuminata)
- [ ] Ruotando il corpo, la texture si muove con esso

## Bloccanti
- Ticket 09 (shader lighting, per integrare normal map nel materiale)
