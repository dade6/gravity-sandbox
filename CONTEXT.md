# CONTEXT.md — Gravity Sandbox

Progetto: sandbox di gravità N-body in Bevy 0.19 + Avian 0.7, WASM su porta
8081. Vincoli generali in `HERMES.md`.

## Glossario

- **Trail storico** — posizioni passate reali, campionate ogni N tick fisici
  (`TrajectoryHistory`, ring buffer O(1)). Esistente.
- **Previsione ghost** — simulazione clonata del futuro di TUTTI i corpi,
  calcolata progressivamente (un pezzo per frame). Lo stato clonato ("ghost")
  vive in una `Resource` separata: NON sono entità ECS con componenti Avian.
- **Ghost fedele** — la previsione replica l'integratore della sim reale
  (Euler semi-implicito, 6 substep, forza congelata per tick, dt con velocità
  sim). Prevede la sim, errori numerici inclusi — non la fisica ideale.
- **Dirty flag** — segnalazione "stato cambiato" → la previsione riparte da
  zero (invalidazione totale) e ricresce frame dopo frame.
- **Merge anelastico** — alla collisione nel ghost, i due corpi si fondono:
  massa sommata, baricentro, quantità di moto conservata. Coerente con
  Restitution 0.0 della sim reale.
- **Orizzonte** — quantità di futuro calcolato (unità: secondi di sim,
  configurabile nel settings, default 300s). Conversione interna in step.
- **Finestra scorrevole** — a sim in Run i punti percorsi si cancellano in
  testa e si appendono punti nuovi in coda (lunghezza costante, zero
  ricalcoli; l'ancora resta lo snapshot al Play così la divergenza resta
  visibile).
- **Futuro tratteggiato** — le curve future sono tratteggiate, il trail
  storico è continuo; la curva del selezionato è evidenziata.
- **Marcatore ✕** — punto di collisione previsto, dove le curve coinvolte si
  troncano; il calcolo prosegue oltre.
- **Correzione lenta** — in Run, ogni trail vivo + corpo ghost viene traslato
  di una frazione del divario testa-pianeta (1% per tick): la testa resta
  incollata senza salti, le divergenze grandi restano visibili a lungo.
- **Orbita circolare** — orbita a distanza costante dalla stella di
  riferimento (nel senso di questa sandbox: la traiettoria che il motore
  N-body + il ghost producono partendo dalla velocità circolare).
- **Velocità circolare** — `v = sqrt(G·M·r / (r²+s²))`, con `G` = costante
  gravitazionale corrente, `M` = massa della stella di riferimento,
  `r` = distanza pianeta–stella, `s` = softening (5.0). Per `r >> s`
  coincide con la formula scolastica `sqrt(G·M/r)`.
- **Stella di riferimento** — la stella (`BodyType::Star`) più vicina al corpo
  selezionato. Centro rispetto al quale si calcola la velocità circolare.
  (Scelta manuale del centro — es. luna attorno a pianeta — rimandata a
  sviluppo futuro.)

## Decisioni registrate (ADR)

- [ADR 0001 — Previsione traiettorie lunghe](docs/adr/0001-previsione-traiettorie-lunghe.md):
  modello (tutti i corpi, progressivo, ghost in Resource), invalidazione totale
  con dirty flag, ghost fedele all'integratore della sim, collisioni rilevate
  con marcatore ✕ e continuazione, merge anelastico post-collisione.
- [ADR 0002 — Bottone "orbita circolare"](docs/adr/0002-orbita-circolare.md):
  bottone nel pannello Properties, centro = stella più vicina, verso attuale
  conservato, solo in pausa, formula esatta con softening, disabilitato con
  motivo nei casi limite.
