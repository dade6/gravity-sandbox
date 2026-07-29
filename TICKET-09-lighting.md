# Ticket 09 — Illuminazione: shader WGSL + PointLight2D

## Descrizione
Implementare un sistema di illuminazione 2D con shader WGSL custom. Le stelle emettono luce, ogni corpo è illuminato dalla stella più vicina. La luce decade con la distanza. I corpi senza stella vicina ricevono solo luce ambient.

## Cosa deve fare
### 1. Componente LightSource
- Aggiungere componente `LightSource { intensity: f32, falloff: f32 }` attaccato alle stelle (`luminous = true`)
- Sistema che raccoglie tutte le LightSource attive

### 2. Materiale custom WGSL
- Creare shader WGSL `light_material.wgsl` con Material2d
- Input: posizione luce, intensità, colore luce, normal map (opzionale)
- Calcolo: direzione luce → corpo → intensità normalizzata per distanza
- Se normal map assente → normale frontale (0,0,1) → illuminazione piatta gradiente
- Ambient light minima (10-15%) per corpi non illuminati

### 3. Sistema illuminazione
- Per ogni corpo non-luminoso, trova la stella più vicina
- Calcola direzione luce e intensità in base alla distanza e al falloff
- Aggiorna le uniform del materiale ogni frame

### 4. Predisposizione normal map
- Il materiale deve accettare `normal_map: Option<Handle<Image>>`
- Se assente → normale frontale → illuminazione uniforme
- Pronto per quando arriveranno le texture (Ticket 14)

## Test di verifica
- [ ] Stella gialla al centro, pianeta blu illuminato più sul lato verso la stella
- [ ] Pianeta lontano da stelle → scuro (solo ambient)
- [ ] Muovendo un pianeta, l'illuminazione cambia in tempo reale
- [ ] Due stelle → corpi illuminati dalla più vicina
- [ ] FPS > 30 con 20 corpi e shader attivo

## Bloccanti: nessuno (parallelo ad altri ticket)
