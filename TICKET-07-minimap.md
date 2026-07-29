# Ticket 07 — Minimap

## Descrizione
Aggiungere una minimap che mostra una vista in miniatura di tutto il sistema solare in un angolo dello schermo, con indicazione della viewport corrente.

## Cosa deve fare
- Mini-camera ortografica separata che inquadra tutto il sistema
- Renderizzare su texture in angolo (basso-destro, piccolo)
- Mostrare tutti i corpi come punti colorati (dimensione proporzionale a massa/raggio)
- Rettangolo che indica l'area inquadrata dalla camera principale (viewport)
- Click sulla minimap → centra la camera principale su quel punto
- Aggiornamento ogni N frame (non ogni frame per performance)

## Test di verifica
- [ ] Minimap visibile in basso a destra, circa 150×150px
- [ ] Tutti i corpi appaiono come punti colorati
- [ ] Rettangolo viewport si muove quando fai pan/zoom
- [ ] Click sulla minimap → camera principale si sposta

## Bloccanti: nessuno
