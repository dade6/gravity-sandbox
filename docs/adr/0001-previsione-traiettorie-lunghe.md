# ADR 0001 — Previsione traiettorie lunghe (long-horizon prediction)

**Status:** Proposed (in definizione via grill-with-docs, set 2026)
**Relates-to:** feature "previsione traiettorie per sistemi stabili"

## Contesto

Scopo: poter **progettare sistemi stabili** vedendo il futuro dell'intero sistema
N-body (orbite complete, perturbazioni reciproche), non solo ~15s del corpo
selezionato con gli altri fermi.

Stato attuale del codice (`src/systems/trajectory.rs`, T11-B):
- `prediction_system` integra RK4 il **solo corpo selezionato** con gli altri
  corpi assunti FERMI → fedele solo nei primissimi istanti di un sistema
  multi-corpo.
- `prediction_steps` default 200, clamp 50–1000 → a 64 Hz max ~15s di sim,
  spesso meno di un'orbita.

Fatti verificati dai sorgenti Avian 0.7.0 (registry cargo):
- Integratore: **solo semi-implicit (symplectic) Euler** è supportato
  ("Currently, only the semi-implicit Euler integration scheme is supported",
  `dynamics/integrator/mod.rs`).
- `SubstepCount` default = **6** substep per tick.
- `Restitution` default coefficient = **0.0** (urto anelastico totale).
- `gravity_system` (nostro, FixedUpdate) ricalcola le forze **una volta per
  tick** dalle posizioni correnti; Avian avanza 6 substep con quella forza
  congelata.
- Modifica massa dal property editor tiene sincronizzate `CelestialBody.mass`
  e `Mass` Avian (`ui.rs:809`).
- Editing (Add/Move/Delete) attivo solo a sim in pausa (`tools.rs`).

## Decisioni

1. **Modello: simulazione completa del futuro di TUTTI i corpi.** Il ghost è un
   clone (pos/vel/massa/raggio) di ogni corpo; ognuno tira tutti gli altri.
   Calcolo **progressivo**: un sistema in `Update` integra un "pezzo" di futuro
   per frame e appende i punti — il frame rate (60–120 Hz) è più frequente del
   tick fisico (64 Hz) e il carico si spalma sui frame.
2. **Stato ghost FUORI dall'ECS fisico.** Niente componenti Avian
   (`Position`/`LinearVelocity`/`Collider`) sulle copie: vivono in una
   `Resource` dedicata, integrate dalla nostra RK4/Euler. Motivi: Avian le
   muoverebbe davvero col suo integratore (divergenza), e i sistemi
   luce/ombra/glow le tratterebbero come pianeti reali.
3. **Invalidazione totale con dirty flag.** Qualsiasi modifica a sim ferma
   (drag corpo, edit proprietà, add/delete corpo, cambio G, cambio velocità
   sim) → flag dirty → la previsione riparte da zero e ricresce
   progressivamente. Niente riuso parziale dei punti vecchi.
4. **Ghost fedele alla sim, non alla fisica ideale.** La previsione replica
   l'integratore della sim reale: Euler semi-implicito, forza congelata per
   tick, 6 substep, `dt = timestep × relative_speed` (include la velocità
   corrente), stesso softening e stesso ordine di iterazione dei corpi.
   Conseguenze: se la sim reale drifta per errore numerico, la previsione lo
   MOSTRA — è un ghost run deterministico della simulazione, non la verità
   fisica. Costo: 1 valutazione forze/tick (vs 4 di RK4) → ~4× più economico.
5. **Collisioni: rilevate + marcatore + continuazione.** Nel ghost, a ogni step
   controllo distanza < somma raggi. Alla prima collisione: la previsione di
   quei corpi si tronca lì con **marcatore evidente (✕)** sul punto, e il
   calcolo CONTINUA da quel punto (richiesta esplicita di Davide: non fermarsi).
6. **Post-collisione: merge perfettamente anelastico.** I due ghost si fondono
   in uno: massa sommata, posizione = baricentro, velocità = conservazione
   della quantità di moto. Da lì una sola curva prosegue. Coerente con
   `Restitution` 0.0 della sim reale (i corpi restano appiccicati).
7. **Orizzonte configurabile in secondi di sim** (set 2026): campo nel pannello
   settings, default 300s, clamp 10–3600s. Conversione interna in step:
   secondi × 64 tick/s (dt include già la velocità sim).
8. **Finestra scorrevole durante il Run (proposta Davide).** A sim in Run la
   previsione NON si congela né si ricalcola: i punti già percorsi si
   cancellano in testa e si appendono nuovi punti in coda integrando avanti
   dallo stato fantasma di frontiera — lunghezza costante, nessun dato
   precedente perso. Il ricalcolo totale scatta solo se le condizioni
   cambiano (edit corpo, add/delete, cambio G). Nota tecnica: la finestra
   scorre per conteggio tick dalla previsione originale (ancora = snapshot al
   Play), NON ri-ancorata allo stato reale — così l'eventuale divergenza
   reale-vs-previsto resta visibile (test di conformità). Dettaglio da
   definire in implementazione: tolleranza alla deriva numerica tra S reale e
   P previsto dopo molti tick.
9. **Rendering: tutte le curve + selezionato evidenziato + futuro tratteggiato.**
   Ogni corpo ha la sua curva futura nel suo colore (come il trail storico);
   la curva del corpo selezionato è più spessa/opaca, le altre attenuate.
   Futuro vs passato: **curve future TRATTEGGIATE**, trail storico continuo.
   Marcatore ✕ alle collisioni previste (Dec. 5). Densità di disegno ridotta
   sul futuro lontano (campionamento ogni N step, non un punto per tick).
10. **La vecchia previsione RK4 viene SOSTITUITA, non affiancata.** I punti
    verdi del `prediction_system` attuale spariscono, rimpiazzati dalle curve
    tratteggiate fedeli. Motivazione: due modelli diversi sullo stesso schermo
    mostrerebbero due futuri contraddittori (RK4/altri-fermi vs ghost fedele).
    Non è rimozione di feature (stessa UX, modello corretto). Il campo
    settings esistente `set_traj_prediction` viene riconvertito in secondi di
    sim (orizzonte, Dec. 7).

## Conseguenze

- La vecchia `prediction_system` RK4 (punti verdi sul selezionato, altri
  fermi) è **superata**: la nuova previsione è un superset fedele. La
  sostituzione è da confermare (vincolo progetto: non rimuovere feature, ma
  qui la feature viene ri-fatta meglio — stessa UX, modello corretto).
- L'orizzonte in step dipende dalla velocità sim (`dt` per tick); esprimerlo in
  secondi di sim è l'unità intuitiva per l'utente.
- Orizzonte configurabile in secondi di sim (Decisione 6): campo settings,
  default 300s, clamp 10–3600s. Conversione interna: secondi × 64 tick/s.

## Domande ancora aperte

- Integrazione settings panel + persistenza (`TrajectoryConfig` è serde nel
  livello; nuove impostazioni con `#[serde(default)]`).
- Budget per frame del calcolo progressivo (chunk size adattivo).
- Tolleranza alla deriva numerica reale-vs-previsto dopo molti tick di Run
  (nota in Dec. 8).
