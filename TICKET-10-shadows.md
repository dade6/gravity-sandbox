# Ticket 10 — Ombre proiettate

## Descrizione
Ogni corpo non-luminoso proietta un'ombra nella direzione opposta alla stella più vicina. L'ombra è renderizzata come poligono scuro semitrasparente.

## Cosa deve fare
- Calcolare cono d'ombra: raggio corpo + vettore luce → poligono (triangolo o quadrilatero)
- Renderizzare mesh ombra su layer sotto i corpi
- Opacità base 0.3, indipendente dalla distanza
- Culling: non renderizzare ombre per corpi troppo lontani (< 50px di raggio visivo)
- Le stelle (`luminous = true`) non proiettano ombra
- L'ombra segue il corpo mentre si muove

## Test di verifica
- [ ] Pianeta illuminato da stella → cono d'ombra visibile dietro il pianeta
- [ ] L'ombra si allunga/allarga in base alla distanza dalla stella
- [ ] Spostando il pianeta, l'ombra si muove con esso
- [ ] Stella non ha ombra

## Bloccanti
- Ticket 09 (sistema illuminazione, per avere la direzione della luce)
