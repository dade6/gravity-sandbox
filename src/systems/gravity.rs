use avian2d::dynamics::integrator::{integrate_velocities, IntegrationSystems};
use avian2d::dynamics::solver::schedule::SubstepSchedule;
use avian2d::dynamics::solver::solver_body::SolverBody;
use avian2d::prelude::*;
use bevy::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::systems::persistence::{GravitationalConstant, SOFTENING};

/// Plugin gravità (v0.14.104): registra il sistema per-substep.
/// Un solo punto di registrazione per prod WASM (`lib.rs`), nativo
/// (`main.rs`), app di test (`lib.rs`) e harness (`timeline.rs`).
pub struct GravityPlugin;

impl Plugin for GravityPlugin {
    fn build(&self, app: &mut App) {
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
}
