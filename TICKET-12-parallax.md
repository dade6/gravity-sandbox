# Ticket 12 — Sfondo stellare procedurale + Parallasse

## Descrizione
Generare uno sfondo con stelle generate proceduralmente su 3 layer con parallasse, per dare profondità alla scena.

## Cosa deve fare
### 1. Sistema parallasse 3 layer
- Layer 1 (sfondo): stelle fisse, piccole (raggio 0.5-1.5), grigie, opacità 0.3-0.6
- Layer 2 (medio): stelle medie (raggio 1-3), colori caldi/freddi, opacità 0.5-0.8, parallasse 0.2×
- Layer 3 (primo piano): stelle brillanti (raggio 2-4), bianche/gialle, opacità 0.7-1.0, parallasse 0.5×

### 2. Generazione procedurale
- All'avvio, generare N stelle per ogni layer (500/200/50)
- Posizioni random in un'area grande (es. 5000×5000 unità)
- Seed casuale (o fisso per riproducibilità)
- Le stelle sono sprite circolari semplici (non mesh)

### 3. Movimento parallasse
- La posizione di ogni layer è legata alla camera principale
- Layer 1 non si muove (parallasse 0)
- Layer 2 si muove al 20% della velocità della camera
- Layer 3 si muove al 50% della velocità della camera

## Test di verifica
- [ ] All'avvio si vedono stelle sullo sfondo nero
- [ ] Facendo pan, i layer si muovono a velocità diverse (effetto parallasse)
- [ ] Facendo zoom, le stelle appaiono più grandi/più piccole (ma non cambiano dimensione reale)
- [ ) Frame rate > 30fps con 750 stelle totali

## Bloccanti: nessuno
