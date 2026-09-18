# ADR 0002 — Bottone "orbita periodica" nel pannello Properties

**Status:** Accepted (v0.14.102) + modifica direzione (set 2026, sotto)
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
5. **Formula esatta con softening**: modulo `v = sqrt(G·M·r / (r²+s²))` con
   la `G` corrente. Coincide con motore e previsione ghost; per `r >> s` è
   identica alla scolastica `sqrt(G·M/r)`. Il modulo è sempre legato
   (`v < v_fuga`) quindi l'orbita è periodica.
6. **Bottone sempre visibile, disabilitato con motivo**: se il selezionato è
   una stella / non c'è nessuna stella in scena / `r < raggio stella + raggio
   pianeta` (anti divisione-per-zero) → disabilitato con motivo ("nessuna
   stella", "troppo vicino alla stella", ...).

## Modifica set 2026 — da "circolare" a "periodica"

Scopo chiarito da Davide: ogni rivoluzione deve essere uguale alla
precedente, NON necessariamente circolare. La v0.14.102 raddrizzava sempre a
perpendicolare (e = 0); ora il bottone ("Orbita periodica") conserva la
DIREZIONE attuale e corregge solo il MODULO a `v_circ`:

3bis. **Direzione = quella attuale** (non più raddrizzamento a
   perpendicolare): cerchio se perpendicolare, ellisse se obliqua. Il
   fallback perpendicolare (senso attuale, default antiorario) resta solo
   quando la direzione non dà un'orbita valida: velocità zero, radiale pura,
   o periastro stimato `rp = r·(1−|sinφ|) < r_stella + r_corpo` (con
   `v = v_circ` si ha `a = r` via vis-viva, `e = |sinφ|`).
   Funzione: `periodic_velocity` in `src/systems/orbit.rs`
   (`circular_velocity` resta come fallback/test).

## Tentativo shooting col ghost (set 2026) — ESITO NEGATIVO, scartato

Problema (report Davide su v0.14.103): l'orbita calcolata si allarga di giro
in giro invece di ripetersi. Diagnosi con sonda headless (Avian vero,
numeri del preset Sole M=5000 + Alpha m=50 a r=200, G=5000, dt=1/64):

- Il ghost replica il motore entro **0.02 unità su 456 tick**: la previsione
  è fedele, il drift è nel motore, non nella formula.
- La formula continua `v_circ` nel discreto (forza congelata 1x/tick + 6
  substep Eulero ≈ Eulero esplicito) **pompa energia ogni orbita**: raggio
  medio +27/orbita (~+13%), già 200→230 al primo giro.
- Paesaggio (orbite segmentate per angolo vero, non finestre fisse):
  D(f) sempre > 0 per f ∈ [0.8, 1.2] (minimo +26.9 a f≈1.0), swing minimo
  a f=1.0. **Nessun modulo azzera il drift**: 1 parametro non può
  soddisfare chiusura radiale + velocità insieme (2 condizioni), e il
  pompaggio è secolare, non un offset iniziale.
- Due ottimizzatori provati (sezione aurea su orizzonte fisso; griglia +
  rifinitura su chiusura a 2π): il primo cade in un falso minimo che
  precipita (f=0.70), il secondo chiude il giro 1 ma il giro 2 pompa come
  prima. Scartati entrambi, codice rimosso (mai deployato).

Conseguenza: `v_circ` + direzione attuale RESTA l'ottimo least-bad
(swing minimo, drift minimo). Il bottone dà la velocità giusta al primo
giro; il ghost mostra onestamente l'allargamento.

Fix strutturale (NON tentato, decisione di Davide): il pompaggio sparisce
solo riducendo il dt fisico (tick più frequenti) o rinfrescando la forza
per-substep — tocca tick rate globale, ghost, preset e velocità sim.
Lavoro grosso, da valutare a parte (orbite larghe pompano molto meno).

## Fix v0.14.104 — gravità per-substep (pompaggio ELIMINATO)

Il "fix strutturale" sopra si è rivelato NON grosso: Avian espone
`SubstepSchedule` + `SolverBody` (posizioni live = `Position` +
`delta_position`). Nuovo `substep_gravity_system` (`GravityPlugin`,
`src/systems/gravity.rs`): kick di velocità con forza ricalcolata a ogni
substep invece di ConstantForce congelata 1x/tick. Risultati headless
(numeri del preset, Alpha a r=200):

- drift orbita su 2 giri: **+21.6 → −0.00**, swing 30+ → 4.0;
- ghost fedele entro 0.25 unità su 200 tick anche in regime estremo
  (M=500000) — il ghost ora replica 6 eval/tick (`ghost_step_tick`).

Lezione: i corpi addormentati (Sleeping) PERDONO `SolverBody` → la query
con `&mut SolverBody` richiesto li escludeva e con <2 corpi il sistema
faceva early return (gravità spenta per tutti!). Ora `Option<&mut
SolverBody>`: gli addormentati tirano da fermi, i kick vanno solo agli
svegli. Vecchio `gravity_system` (FixedUpdate) rimosso; `pair_force`
formula unica per sim + ghost.

## Conseguenze

- Impostare la velocità deve alzare il dirty flag delle traiettorie (come
  ogni edit a sim ferma) così il ghost riparte.
- Test: caso velocità iniziale zero (vincolo di progetto), caso radiale pura,
  caso `r` minima, nessuna stella in scena.
