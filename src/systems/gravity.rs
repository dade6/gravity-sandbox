use bevy::prelude::*;
use avian2d::prelude::*;

use crate::components::celestial::CelestialBody;

/// Gravitational constant (tunable)
const G: f32 = 1000.0;
/// Softening factor to avoid singularities at close distances
const SOFTENING: f32 = 10.0;

/// N-body gravity system.
/// For every pair of CelestialBody entities, computes F = G * m1 * m2 / (r² + ε²)
/// and applies it via Forces QueryData on Avian rigid bodies.
pub fn gravity_system(
    query: Query<(Entity, &CelestialBody, &GlobalTransform)>,
    mut force_query: Query<Forces>,
) {
    let bodies: Vec<(Entity, CelestialBody, Vec2)> = query
        .iter()
        .map(|(e, body, xform)| (e, body.clone(), xform.translation().truncate()))
        .collect();

    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            let (e1, b1, t1) = &bodies[i];
            let (e2, b2, t2) = &bodies[j];

            let delta = *t2 - *t1;
            let dist_sq = delta.length_squared();
            let force_magnitude = G * b1.mass * b2.mass / (dist_sq + SOFTENING * SOFTENING);
            let direction = if dist_sq > 0.0 {
                delta / dist_sq.sqrt()
            } else {
                Vec2::ZERO
            };
            let force = direction * force_magnitude;

            // Apply forces using Forces QueryData
            if let Ok(mut f1) = force_query.get_mut(*e1) {
                f1.apply_force(force);
            }
            if let Ok(mut f2) = force_query.get_mut(*e2) {
                f2.apply_force(-force);
            }
        }
    }
}
