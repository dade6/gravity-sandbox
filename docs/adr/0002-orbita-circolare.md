# ADR 0002 — Bottone "orbita circolare" nel pannello Properties

**Status:** Proposed (definito via grill-with-docs, set 2026)
**Relates-to:** feature "velocità per orbite costanti attorno alla stella"

## Contesto

Oggi per mettere un pianeta in orbita stabile l'utente deve indovinare a mano
`Vel X` / `Vel Y` nel pannello Properties. Fatti verificati dai sorgenti:

- `gravity.rs`: `F = G·m1·m2 / (r²+s²)` con direzione normalizzata,
  `G` = resource `GravitationalConstant` (default 5000.0, modificabile),
  `s` = `SOFTENING` = 5.0 condiviso col predittore traiettorie.
- I corpi sono Avian `RigidBody::Dynamic` + `LinearVelocity`: impostare
  l'orbita = scrivere un `Vec2` lì.
- Editing esistente vale solo in pausa (`tools.rs`, hint "Edit mode" /
  "Pause to edit" in `ui.rs`).
- Selezione esistente: `SelectedBody(Option<Entity>)`, pannello Properties a
  destra con campi editabili (`ui.rs`).

## Decisioni

1. **Bottone nel pannello Properties del corpo selezionato** ("seleziono il
   pianeta e avvio il calcolo da interfaccia"). Niente helper orfano: la
   funzione pura Rust (`circular_orbit_velocity`) vive sotto il bottone ed è
   riusabile da spawn/tool/preset/test.
2. **Centro = stella più vicina** (`BodyType::Star`) al pianeta selezionato.
   Alternativa scartata: corpo più massiccio (sorprendente con due stelle).
   Scelta manuale del centro (es. lune attorno a pianeti) rimandata a futuro.
3. **Verso = quello attuale** (segno del momento angolare rispetto alla
   stella); se velocità zero o radiale pura → default antiorario. Così non si
   ribalta mai un sistema esistente per sbaglio.
4. **Solo in pausa**, coerente con tutto l'editing (evita di toccare
   `LinearVelocity` mentre Avian integra).
5. **Formula esatta con softening**: `v = sqrt(G·M·r / (r²+s²))` con la `G`
   corrente. Coincide con motore e previsione ghost; per `r >> s` è identica
   alla scolastica `sqrt(G·M/r)`. Direzione perpendicolare al raggio
   stella→pianeta.
6. **Bottone sempre visibile, disabilitato con motivo**: se il selezionato è
   una stella / non c'è nessuna stella in scena / `r < raggio stella + raggio
   pianeta` (anti divisione-per-zero) → disabilitato con motivo ("nessuna
   stella", "troppo vicino alla stella", ...).

## Conseguenze

- Impostare la velocità deve alzare il dirty flag delle traiettorie (come
  ogni edit a sim ferma) così il ghost riparte.
- Test: caso velocità iniziale zero (vincolo di progetto), caso radiale pura,
  caso `r` minima, nessuna stella in scena.
