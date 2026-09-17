use avian2d::prelude::*;
use bevy::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::systems::persistence::{GravitationalConstant, SOFTENING};

/// N-body gravity system.
/// Runs in FixedUpdate to sync with Avian's physics solver.
///
/// Reads positions from Avian `Position` (the physics state of record):
/// the transform tree is only propagated in PostUpdate, so at FixedUpdate
/// time `GlobalTransform` may still hold the previous tick's positions.
/// Every body has `Position` (`RigidBody` requires it), so no spawn path is
/// skipped by this query.
///
/// Mass note (bug v0.14.100 investigation): Avian integrates ConstantForce
/// as acceleration via `ComputedMass.inverse()`. Verified headless
/// (`ghost_total_mass_matches_avian_computed_mass`): with explicit `Mass`
/// present, `ComputedMass == Mass` EXACTLY (the explicit mass REPLACES the
/// collider auto-mass — Avian's own test asserts this too). Our bodies
/// always spawn with explicit `Mass` (== `CelestialBody.mass`, synced on
/// edit in ui.rs), so `a = F / m_body` on both sides and the ghost's
/// `acc / body.mass` is already the identical computation. No compensation
/// needed — and none applied (an earlier revision pre-scaled forces by
/// ComputedMass on a wrong auto-mass assumption; reverted).
pub fn gravity_system(
    query: Query<(Entity, &CelestialBody, &Position)>,
    mut force_query: Query<&mut ConstantForce>,
    grav: Res<GravitationalConstant>,
) {
    crate::mark_system("gravity_system");

    let g = grav.0;

    // Collect all bodies
    let bodies: Vec<(Entity, f32, Vec2)> = query
        .iter()
        .map(|(e, body, pos)| (e, body.mass, pos.0))
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
            let force_magnitude = g * m1 * m2 / (dist_sq + SOFTENING * SOFTENING);
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
