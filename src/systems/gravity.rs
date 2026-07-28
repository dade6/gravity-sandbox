use bevy::prelude::*;
use avian2d::prelude::*;

use crate::components::celestial::CelestialBody;

/// Gravitational constant (tunable)
const G: f32 = 5000.0;
/// Softening factor to avoid singularities at close distances
const SOFTENING: f32 = 5.0;

/// N-body gravity system.
/// Runs in FixedUpdate to sync with Avian's physics solver.
pub fn gravity_system(
    query: Query<(Entity, &CelestialBody, &GlobalTransform)>,
    mut force_query: Query<&mut ConstantForce>,
) {
    // Collect all bodies
    let bodies: Vec<(Entity, f32, Vec2)> = query
        .iter()
        .map(|(e, body, xform)| (e, body.mass, xform.translation().truncate()))
        .collect();

    if bodies.len() < 2 {
        return;
    }

    // Zero all forces
    for mut cf in force_query.iter_mut() {
        cf.0 = Vec2::ZERO;
    }

    // Compute N-body forces
    for i in 0..bodies.len() {
        let (e1, m1, t1) = bodies[i];
        for j in (i + 1)..bodies.len() {
            let (e2, m2, t2) = bodies[j];
            let delta = t2 - t1;
            let dist_sq = delta.length_squared();
            if dist_sq < 1.0 {
                continue;
            }
            let force_magnitude = G * m1 * m2 / (dist_sq + SOFTENING * SOFTENING);
            let direction = delta / dist_sq.sqrt();
            let force_vec = direction * force_magnitude;

            if let Ok(mut cf) = force_query.get_mut(e1) {
                cf.0 += force_vec;
            }
            if let Ok(mut cf) = force_query.get_mut(e2) {
                cf.0 -= force_vec;
            }
        }
    }
}
