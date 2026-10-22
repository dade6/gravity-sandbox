use avian2d::dynamics::integrator::{integrate_velocities, IntegrationSystems};
use avian2d::dynamics::solver::schedule::SubstepSchedule;
use avian2d::dynamics::solver::solver_body::SolverBody;
use avian2d::prelude::*;
use bevy::prelude::*;
use bevy_transform_interpolation::TranslationEasingState;

use crate::components::celestial::CelestialBody;
use crate::systems::persistence::{GravitationalConstant, SOFTENING};
use crate::systems::timeline::SimulationState;

/// Plugin gravità (v0.14.104): registra il sistema per-substep.
/// Un solo punto di registrazione per prod WASM (`lib.rs`), nativo
/// (`main.rs`), app di test (`lib.rs`) e harness (`timeline.rs`).
pub struct GravityPlugin;

impl Plugin for GravityPlugin {
    fn build(&self, app: &mut App) {
        // Niente sleeping per i corpi celesti (bug v0.14.105): Avian
        // addormentava la stella (velocità < 0.15 per > 0.5 s → `Sleeping` →
        // `SolverBody` rimosso → stella congelata nella sim vera), mentre il
        // ghost — che non ha il concetto di sonno — la integrava sempre.
        // Risultato: orbita reale periodica perfetta attorno a una stella
        // fissa, ghost in deriva crescente (stella ghost su traiettoria
        // quasi-rettilinea). Con `SleepingDisabled` come required component
        // ogni `CelestialBody` resta sempre integrato — presente, passato e
        // futuro — e sim vera e ghost vedono la stessa fisica.
        app.register_required_components::<CelestialBody, SleepingDisabled>();
        // Smussamento visivo del moto (v0.14.156).
        //
        // La fisica integra a passi fissi (1/64 s) e Avian scrive `Transform`
        // SOLO nel tick: a 120 Hz metà dei frame non muovono niente e gli altri
        // saltano di un tick intero → il pianeta "scatta" (si vede col follow
        // camera, nella parallasse delle stelle).
        // `TranslationInterpolation` scrive il `Transform` interpolato in
        // `RunFixedMainLoop::AfterFixedMainLoop`, PRIMA di `Update`, quindi ogni
        // lettore di posizione vede già la posizione smussata.
        //
        // GATE (pagato con la v0.14.154, rollback in v0.14.155): l'easing si
        // azzera a OGNI scrittura di `Transform` fuori dal fixed step
        // (`reset_easing_states_on_transform_change`). Per questo `sync_sprite_z`
        // — che scrive `z` sui corpi per lo z-sort di firefly — è stato spostato
        // DENTRO il tick (firefly_bridge.rs). Se domani si aggiunge un altro
        // scrittore per-frame di `Transform` sui corpi, lo smussamento sparisce
        // in silenzio: va spostato anche quello.
        app.register_required_components::<CelestialBody, TranslationInterpolation>();
        app.add_systems(
            RunFixedMainLoop,
            snap_eased_transforms_while_paused
                .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop)
                .after(TransformEasingSystems::Ease),
        );
        app.add_systems(
            SubstepSchedule,
            substep_gravity_system
                .in_set(IntegrationSystems::Velocity)
                .after(ForceSystems::ApplyLocalAcceleration)
                .before(integrate_velocities),
        );
    }
}

/// Forza gravitazionale su `e1` dovuta a `e2`: stessa formula ovunque
/// (substep-system, ghost). `delta` = pos2 − pos1.
pub fn pair_force(delta: Vec2, dist_sq: f32, m1: f32, m2: f32, g: f32) -> Vec2 {
    let force_magnitude = g * m1 * m2 / (dist_sq + SOFTENING * SOFTENING);
    let direction = delta / dist_sq.sqrt();
    direction * force_magnitude
}

/// In pausa la posizione renderizzata deve essere quella VERA di fine tick.
///
/// L'easing lascia il corpo a `lerp(start, end, overstep_fraction)`: con gli
/// orologi fermi quella frazione resta congelata, quindi un singolo Step
/// mostrerebbe il corpo ancora in `start` (visivamente "non si muove") e la
/// pausa lo lascerebbe fino a un tick indietro.
///
/// Si scrive SOLO al cambio reale: una scrittura per frame di `Transform` in
/// pausa tiene caldo `Changed<Transform>`, e `ghost_dirty_triggers` (che lo
/// guarda solo da fermo) riavvierebbe la previsione ogni frame — le curve
/// future resterebbero congelate a 1 chunk (stesso sintomo del bug v0.14.93).
///
/// Si usa `end` (stato dell'easing) e NON `Position`: se il corpo è stato
/// spostato fuori dal fixed loop (drag, pannello, reset)
/// `reset_easing_states_on_transform_change` ha già azzerato lo stato (`None`)
/// e questo sistema non tocca niente.
fn snap_eased_transforms_while_paused(
    sim_state: Option<Res<SimulationState>>,
    mut bodies: Query<(&mut Transform, &mut TranslationEasingState)>,
    mut was_paused: Local<bool>,
) {
    crate::mark_system("snap_eased_transforms_while_paused");
    // `Option`: gli harness headless aggiungono `GravityPlugin` senza
    // `TimelinePlugin`; senza il resource si assume "non in pausa".
    let paused = sim_state.map(|s| s.paused).unwrap_or(false);
    if !paused && !*was_paused {
        return;
    }
    *was_paused = paused;
    for (mut transform, mut easing) in &mut bodies {
        let Some(end) = easing.end else { continue };
        if transform.translation != end {
            transform.translation = end;
        }
        // Al ritorno in play `start` = `end` evita il saltino INDIETRO del
        // primo frame (mostrerebbe `lerp(start, end, frazione)`).
        if easing.start != Some(end) {
            easing.start = Some(end);
        }
    }
}

/// N-body gravity: kick di velocità per-substep (v0.14.104).
///
/// Sostituisce il vecchio sistema FixedUpdate + ConstantForce (rimosso):
/// quello scriveva la forza 1x/tick e Avian la congelava per i 6 substep →
/// pompaggio secolare di energia (+27 di raggio medio/orbita a r=200 sui
/// numeri del preset). Questo gira in `SubstepSchedule` prima di
/// `integrate_velocities` e ricalcola la forza con le posizioni correnti
/// dello step, dando il kick con il dt del substep.
///
/// Legge le posizioni da `Position` + `SolverBody.delta_position` (Position
/// è ferma a inizio tick, il delta accumula il moto nei substep — vedi
/// `writeback_solver_bodies` di Avian). Massa da `CelestialBody.mass`
/// (== `Mass` esplicita == `ComputedMass`, verifica v0.14.100).
///
/// I corpi tengono `ConstantForce(Vec2::ZERO)` inerte negli spawn (mai
/// scritta da nessuno: `apply_constant_forces` di Avian aggiunge zero).
pub fn substep_gravity_system(
    mut bodies: Query<(Entity, &CelestialBody, &Position, Option<&mut SolverBody>)>,
    grav: Res<GravitationalConstant>,
    sub_time: Res<Time<Substeps>>,
) {
    let g = grav.0;
    let dt_sub = sub_time.delta_secs_f64() as f32;
    if !(dt_sub > 0.0) {
        return;
    }
    // Posizioni live: Position è ferma a inizio tick, delta_position
    // accumula il moto dentro i substep (vedi writeback_solver_bodies).
    // La query NON richiede SolverBody: i corpi addormentati (Sleeping)
    // lo perdono ma gravano comunque da fermi con la loro Position.
    let snapshot: Vec<(Entity, f32, Vec2, bool)> = bodies
        .iter()
        .map(|(e, body, pos, solver)| {
            let (p, has_solver) = match solver {
                Some(s) => (pos.0 + s.delta_position, true),
                None => (pos.0, false),
            };
            (e, body.mass, p, has_solver)
        })
        .collect();
    if snapshot.len() < 2 {
        return;
    }
    // Kick per corpo: a = F/m_proprio * dt_sub. Solo i corpi con SolverBody
    // (svegli) integrano davvero; gli addormentati restano fermi ma tirano.
    let mut kicks: Vec<(Entity, Vec2, bool)> = snapshot
        .iter()
        .map(|(e, _, _, has)| (*e, Vec2::ZERO, *has))
        .collect();
    for i in 0..snapshot.len() {
        let (_, m1, t1, _) = snapshot[i];
        for j in (i + 1)..snapshot.len() {
            let (_, m2, t2, _) = snapshot[j];
            let delta = t2 - t1;
            let dist_sq = delta.length_squared();
            if dist_sq < 1.0 {
                continue;
            }
            let force_vec = pair_force(delta, dist_sq, m1, m2, g);
            kicks[i].1 += force_vec / m1 * dt_sub;
            kicks[j].1 -= force_vec / m2 * dt_sub;
        }
    }
    for (entity, kick, has_solver) in kicks {
        if !has_solver {
            continue;
        }
        if let Ok((_, _, _, Some(mut solver))) = bodies.get_mut(entity) {
            solver.linear_velocity += kick;
        }
    }
}

#[cfg(test)]
mod spike_tests {
    use super::*;
    use avian2d::dynamics::integrator::{integrate_velocities, IntegrationSystems};
    use avian2d::dynamics::solver::schedule::SubstepSchedule;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    use crate::components::celestial::BodyType;

    /// Regression test (pompaggio secolare, set 2026): gravità nel substep
    /// loop vs ConstantForce congelata.
    ///
    /// Stessi numeri del preset (Sole M=5000 + Alpha m=50 a r=200, G=5000,
    /// dt=1/64): il vecchio path pompava +21.6/orbita (misurato headless).
    /// Il kick per-substep deve tenere il drift vicino a zero su 2 giri.
    #[test]
    fn spike_substep_gravity_halves_drift() {
        let g = 5000.0f32;
        let m_star = 5000.0f32;
        let m_planet = 50.0f32;
        let r = 200.0f32;
        let v_circ = (g * m_star * r / (r * r + 25.0)).sqrt();
        let t0 = ((2.0 * std::f32::consts::PI * r / v_circ) / (1.0 / 64.0)).round() as usize;
        eprintln!("SPIKE v_circ={v_circ:.3} T0={t0}");

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(GravitationalConstant(g))
        // Niente gravity_system: solo kick per-substep. Niente ConstantForce
        // sui corpi (apply_constant_forces li salta).
        .add_systems(
            SubstepSchedule,
            substep_gravity_system
                .in_set(IntegrationSystems::Velocity)
                .after(ForceSystems::ApplyLocalAcceleration)
                .before(integrate_velocities),
        )
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 64.0,
        )));
        app.finish();
        let spawn = |world: &mut World, pos: Vec2, vel: Vec2, mass: f32, radius: f32| {
            world
                .spawn((
                    CelestialBody {
                        name: "b".into(),
                        body_type: BodyType::Planet,
                        mass,
                        radius,
                        color: [0.5, 0.5, 0.8],
                        luminous: false,
                    },
                    Transform::from_xyz(pos.x, pos.y, 0.0),
                    Position::from_xy(pos.x, pos.y),
                    RigidBody::Dynamic,
                    Collider::circle(radius),
                    Mass(mass),
                    LinearVelocity(vel),
                ))
                .id()
        };
        let star = spawn(app.world_mut(), Vec2::ZERO, Vec2::ZERO, m_star, 30.0);
        let planet = spawn(
            app.world_mut(),
            Vec2::new(r, 0.0),
            Vec2::new(0.0, v_circ),
            m_planet,
            12.0,
        );
        app.update(); // prime
        let mut rs = Vec::with_capacity(2 * t0);
        for _ in 0..2 * t0 {
            app.update();
            let w = app.world();
            let sp = w.entity(star).get::<Position>().unwrap().0;
            let pp = w.entity(planet).get::<Position>().unwrap().0;
            rs.push((pp - sp).length());
        }
        let (o1, o2) = rs.split_at(t0);
        let mean = |w: &[f32]| w.iter().sum::<f32>() / w.len() as f32;
        let swing = |w: &[f32]| {
            w.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
                - w.iter().cloned().fold(f32::INFINITY, f32::min)
        };
        let d = mean(o2) - mean(o1);
        let sw = swing(o1).max(swing(o2));
        eprintln!("SPIKE substep D={d:.2} swing={sw:.2} (vecchio path D=+21.6)");
        assert!(
            d.abs() < 10.0,
            "drift dimezzato vs +21.6 del vecchio path: D={d:.2}"
        );
    }

    /// Regression test (stella congelata, bug v0.14.105): senza
    /// `SleepingDisabled` Avian addormentava la stella (velocità < 0.15 per
    /// oltre 0.5 s di sim → `Sleeping` → `SolverBody` rimosso → stella ferma
    /// nella sim vera), mentre il ghost la integrava sempre → deriva
    /// crescente della previsione (orbita reale periodica perfetta, ghost
    /// alla deriva, trail ghost della stella quasi-rettilineo).
    ///
    /// Numeri utente (Sole 10000 + Alpha 8, G=5000): 3 s di sim (> 6x il
    /// TimeToSleep di 0.5 s). La stella non deve mai dormire e deve muoversi.
    #[test]
    fn star_never_sleeps_with_gravity_plugin() {
        let g = 5000.0f32;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(GravitationalConstant(g))
        .add_plugins(GravityPlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 64.0,
        )));
        app.finish();
        let spawn = |world: &mut World, pos: Vec2, vel: Vec2, mass: f32, radius: f32| {
            world
                .spawn((
                    CelestialBody {
                        name: "b".into(),
                        body_type: BodyType::Planet,
                        mass,
                        radius,
                        color: [0.5, 0.5, 0.8],
                        luminous: false,
                    },
                    Transform::from_xyz(pos.x, pos.y, 0.0),
                    Position::from_xy(pos.x, pos.y),
                    RigidBody::Dynamic,
                    Collider::circle(radius),
                    Mass(mass),
                    LinearVelocity(vel),
                ))
                .id()
        };
        let star_pos = Vec2::new(63.372482, 57.54164);
        let star = spawn(app.world_mut(), star_pos, Vec2::ZERO, 10_000.0, 30.0);
        let _planet = spawn(
            app.world_mut(),
            Vec2::new(215.95473, -1000.0),
            Vec2::new(-250.0, -15.0),
            8.0,
            12.0,
        );
        // Required components: ogni corpo nasce già con SleepingDisabled.
        assert!(
            app.world().entity(star).contains::<SleepingDisabled>(),
            "GravityPlugin deve dare SleepingDisabled ad ogni CelestialBody"
        );
        // 3 s di sim: oltre 6x il TimeToSleep (0.5 s). Prima del fix la
        // stella si addormentava qui e restava inchiodata.
        for _ in 0..192 {
            app.update();
        }
        let w = app.world();
        assert!(
            w.entity(star).get::<Sleeping>().is_none(),
            "la stella non deve mai addormentarsi"
        );
        assert!(
            w.entity(star).get::<SolverBody>().is_some(),
            "la stella deve tenere il SolverBody ( resta integrata )"
        );
        let moved = (w.entity(star).get::<Position>().unwrap().0 - star_pos).length();
        assert!(
            moved > 1e-4,
            "la stella deve muoversi sotto l'attrazione del pianeta (mossa={moved:.6})"
        );
    }
}

#[cfg(test)]
mod smoothing_tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    use crate::components::celestial::BodyType;
    use crate::systems::timeline::SimulationState;

    fn paused_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
            GravityPlugin,
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(GravitationalConstant(0.0))
        .insert_resource(SimulationState::default())
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 120.0,
        )));
        app.finish();
        let body = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "Terra".into(),
                    body_type: BodyType::Planet,
                    mass: 10.0,
                    radius: 5.0,
                    color: [0.5, 0.5, 0.8],
                    luminous: false,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
                Position::from_xy(0.0, 0.0),
                RigidBody::Dynamic,
                Collider::circle(5.0),
                Mass(10.0),
                LinearVelocity(Vec2::new(120.0, 0.0)),
            ))
            .id();
        app.update();
        app.update();
        app.world_mut().resource_mut::<SimulationState>().paused = true;
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.world_mut().resource_mut::<Time<Physics>>().pause();
        (app, body)
    }

    /// L'interpolazione è SOLO visiva: le `Position` della fisica devono essere
    /// identiche a quelle di una run senza easing (`NoTranslationEasing`).
    ///
    /// Serve perché "la traiettoria è cambiata" è il sintomo che ha fatto
    /// sospettare (a torto) il motore: con e senza easing le posizioni fisiche
    /// coincidono, quindi se le orbite SEMBRANO diverse il colpevole è un
    /// consumatore della posizione (previsione ghost, camera, z-sort).
    #[test]
    fn interpolation_does_not_perturb_the_physics() {
        fn track(no_easing: bool, ticks: usize) -> Vec<Vec2> {
            let g = 5000.0f32;
            let (m_star, m_planet, r) = (5000.0f32, 50.0f32, 200.0f32);
            let v_circ = (g * m_star * r / (r * r + 25.0)).sqrt();
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                bevy::transform::TransformPlugin,
                PhysicsPlugins::default(),
                GravityPlugin,
            ))
            .insert_resource(Gravity::ZERO)
            .insert_resource(GravitationalConstant(g))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 64.0,
            )));
            app.finish();
            let spawn = |world: &mut World, pos: Vec2, vel: Vec2, mass: f32, radius: f32| {
                let mut e = world.spawn((
                    CelestialBody {
                        name: "b".into(),
                        body_type: BodyType::Planet,
                        mass,
                        radius,
                        color: [0.5, 0.5, 0.8],
                        luminous: false,
                    },
                    Transform::from_xyz(pos.x, pos.y, 0.0),
                    Position::from_xy(pos.x, pos.y),
                    RigidBody::Dynamic,
                    Collider::circle(radius),
                    Mass(mass),
                    LinearVelocity(vel),
                ));
                if no_easing {
                    e.insert(NoTranslationEasing);
                }
                e.id()
            };
            let star = spawn(app.world_mut(), Vec2::ZERO, Vec2::ZERO, m_star, 30.0);
            let planet = spawn(
                app.world_mut(),
                Vec2::new(r, 0.0),
                Vec2::new(0.0, v_circ),
                m_planet,
                12.0,
            );
            let mut out = Vec::with_capacity(ticks);
            for _ in 0..ticks {
                app.update();
                let w = app.world();
                let sp = w.entity(star).get::<Position>().unwrap().0;
                let pp = w.entity(planet).get::<Position>().unwrap().0;
                out.push(pp - sp);
            }
            out
        }

        let with_easing = track(false, 200);
        let without = track(true, 200);
        let mut max_d = 0.0f32;
        let mut at = 0;
        for (i, (a, b)) in with_easing.iter().zip(without.iter()).enumerate() {
            let d = (*a - *b).length();
            if d > max_d {
                max_d = d;
                at = i;
            }
        }
        eprintln!("SPIKE easing: max delta orbita = {max_d:.4} al tick {at} (r=200)");
        assert!(
            max_d < 1e-3,
            "l'easing non deve toccare la fisica: delta max {max_d:.4} unità al tick {at}"
        );
    }

    /// Lo snap in pausa deve scrivere `Transform` SOLO quando serve.
    ///
    /// Esperienza v0.14.154: scrivendolo a ogni frame, `Changed<Transform>`
    /// restava caldo e `ghost_dirty_triggers` (che lo guarda solo da fermo)
    /// riavviava la previsione ogni frame → curve future congelate a 1 chunk e
    /// "traiettoria cambiata" a schermo (stesso sintomo del bug v0.14.93).
    #[test]
    fn paused_snap_writes_only_on_change() {
        let (mut app, _body) = paused_app();
        // Primo frame di pausa: lo snap chiude l'easing (una scrittura vera).
        app.update();
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, (With<CelestialBody>, Changed<Transform>)>();
        let mut frames_with_write = 0;
        for _ in 0..5 {
            app.update();
            if q.iter(app.world()).count() > 0 {
                frames_with_write += 1;
            }
        }
        assert_eq!(
            frames_with_write, 0,
            "in pausa, con gli orologi fermi, il corpo non si muove: nessuna \
             scrittura di Transform dopo lo snap (frame con scritture: {frames_with_write}/5)"
        );
    }

    /// In pausa il `Transform` mostrato deve essere la posizione VERA di fine
    /// tick, e un singolo Step deve spostare il corpo di un tick intero.
    #[test]
    fn paused_body_shows_true_position_and_step_moves_it() {
        let (mut app, body) = paused_app();
        app.update();
        let w = app.world();
        let tf = w.entity(body).get::<Transform>().unwrap().translation.x;
        let pos = w.entity(body).get::<Position>().unwrap().0.x;
        assert!(
            (tf - pos).abs() < 1e-3,
            "in pausa Transform deve coincidere con Position: tf={tf:.4} pos={pos:.4}"
        );

        let timestep = app.world().resource::<Time<Fixed>>().timestep();
        app.world_mut()
            .resource_mut::<Time<Fixed>>()
            .accumulate_overstep(timestep);
        app.world_mut().resource_mut::<Time<Physics>>().unpause();
        app.update();
        app.world_mut().resource_mut::<Time<Physics>>().pause();
        let w = app.world();
        let tf2 = w.entity(body).get::<Transform>().unwrap().translation.x;
        let pos2 = w.entity(body).get::<Position>().unwrap().0.x;
        assert!(
            (tf2 - pos2).abs() < 1e-3,
            "dopo lo Step Transform deve coincidere con Position: tf={tf2:.4} pos={pos2:.4}"
        );
        assert!(
            tf2 - tf > 1.0,
            "lo Step deve VEDERSI: prima={tf:.4} dopo={tf2:.4} (un tick = 1.875 unità)"
        );
    }
}
