use bevy::prelude::*;
use avian2d::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::components::trajectory::{
    PredictionTrail, TrajectoryConfig, TrajectoryFrameCounter, TrajectoryHistory,
};
use crate::systems::selection::SelectedBody;
use crate::systems::timeline::SimulationState;

// ============================================================
// Constants (must match gravity.rs)
// ============================================================

/// Gravitational constant (must match gravity.rs).
const G: f32 = 5000.0;
/// Softening factor (must match gravity.rs).
const SOFTENING: f32 = 5.0;

// ============================================================
// RK4 N-body integrator (T11-B)
// ============================================================

/// Compute the N-body gravitational acceleration on a body at position `pos`
/// due to all other bodies. Other bodies' positions are treated as fixed.
fn nbody_acceleration(pos: Vec2, _vel: Vec2, bodies: &[(f32, Vec2, Vec2)], self_idx: usize) -> Vec2 {
    let mut acc = Vec2::ZERO;
    for (j, &(mj, pj, _)) in bodies.iter().enumerate() {
        if j == self_idx {
            continue;
        }
        let delta = pj - pos;
        let dist_sq = delta.length_squared();
        if dist_sq < 1.0 {
            continue;
        }
        let dist = dist_sq.sqrt();
        // Same formula as gravity.rs: a = G * m_j * direction / (dist_sq + SOFTENING^2)
        let force_mag = G * mj / (dist_sq + SOFTENING * SOFTENING);
        let direction = delta / dist;
        acc += direction * force_mag;
    }
    acc
}

/// Runge-Kutta 4th order integration for the target body's trajectory.
///
/// `bodies` is a snapshot of (mass, position, velocity) for ALL bodies.
/// Only the target body at `target_idx` is integrated; all other bodies
/// are assumed stationary for the prediction horizon.
///
/// Returns `steps` predicted positions of the target body.
pub fn rk4_integrate(
    bodies: &[(f32, Vec2, Vec2)],
    dt: f32,
    steps: usize,
    target_idx: usize,
) -> Vec<Vec2> {
    let mut positions = Vec::with_capacity(steps);
    let (mut pos, mut vel) = (bodies[target_idx].1, bodies[target_idx].2);

    for _ in 0..steps {
        // k1
        let k1_v = vel;
        let k1_a = nbody_acceleration(pos, vel, bodies, target_idx);

        // k2
        let k2_v = vel + k1_a * (dt / 2.0);
        let k2_a = nbody_acceleration(pos + k1_v * (dt / 2.0), k2_v, bodies, target_idx);

        // k3
        let k3_v = vel + k2_a * (dt / 2.0);
        let k3_a = nbody_acceleration(pos + k2_v * (dt / 2.0), k3_v, bodies, target_idx);

        // k4
        let k4_v = vel + k3_a * dt;
        let k4_a = nbody_acceleration(pos + k3_v * dt, k4_v, bodies, target_idx);

        // Weighted average (RK4)
        pos += (k1_v + k2_v * 2.0 + k3_v * 2.0 + k4_v) * (dt / 6.0);
        vel += (k1_a + k2_a * 2.0 + k3_a * 2.0 + k4_a) * (dt / 6.0);

        positions.push(pos);
    }

    positions
}

// ============================================================
// Prediction system (T11-B)
// ============================================================

/// System that computes the prediction trail for the selected body.
/// Runs in `Update` to refresh every frame.
pub fn prediction_system(
    selected: Res<SelectedBody>,
    config: Res<TrajectoryConfig>,
    bodies: Query<(Entity, &CelestialBody, &GlobalTransform, &LinearVelocity)>,
    mut trail: ResMut<PredictionTrail>,
) {
    if !config.enabled {
        trail.0.clear();
        return;
    }

    let target = match selected.0 {
        Some(e) => e,
        None => {
            trail.0.clear();
            return;
        }
    };

    // Collect all body states as a flat snapshot (mass, position, velocity)
    let body_states: Vec<(f32, Vec2, Vec2)> = bodies
        .iter()
        .map(|(_, body, xform, vel)| (body.mass, xform.translation().truncate(), vel.0))
        .collect();

    if body_states.len() < 2 {
        trail.0.clear();
        return;
    }

    // Find index of the selected body in the snapshot list
    let target_idx = bodies.iter().position(|(e, _, _, _)| e == target);

    let target_idx = match target_idx {
        Some(i) => i,
        None => {
            trail.0.clear();
            return;
        }
    };

    // RK4 integration with 4 sub-steps per physics frame (dt ≈ 0.004)
    let dt = 1.0 / 60.0 / 4.0;
    let predicted = rk4_integrate(&body_states, dt, config.prediction_steps, target_idx);
    trail.0 = predicted;
}

// ============================================================
// Prediction rendering system (T11-B)
// ============================================================

/// Renders the prediction trail as green fading dots in `PostUpdate`.
pub fn prediction_render_system(
    selected: Res<SelectedBody>,
    config: Res<TrajectoryConfig>,
    trail: Res<PredictionTrail>,
    mut gizmos: Gizmos,
) {
    if !config.enabled || selected.0.is_none() || trail.0.is_empty() {
        return;
    }

    let total = trail.0.len();
    // Draw at most ~80 dots for performance
    let spacing = (total / 80).max(1);

    for i in (0..total).step_by(spacing) {
        let t = i as f32 / total as f32;
        // Fade from opaque (near) to transparent (far)
        let alpha = (1.0 - t) * 0.7 + 0.05;
        let color = Color::srgba(0.3, 1.0, 0.3, alpha);
        gizmos.circle_2d(trail.0[i], 2.0, color);
    }
}

// ============================================================
// History sampling system (T11-A)
// ============================================================

/// Samples body positions every N frames and stores them in TrajectoryHistory.
fn sample_trajectory(
    mut counter: ResMut<TrajectoryFrameCounter>,
    config: Res<TrajectoryConfig>,
    sim_state: Option<Res<SimulationState>>,
    mut bodies: Query<(&GlobalTransform, &mut TrajectoryHistory), With<CelestialBody>>,
) {
    // Don't sample when paused
    if let Some(sim) = sim_state {
        if sim.paused {
            return;
        }
    }

    counter.0 += 1;
    if counter.0 % config.sample_interval as u64 != 0 {
        return;
    }

    for (transform, mut history) in bodies.iter_mut() {
        // Sync per-entity max_len from the global config
        if history.max_len != config.history_length && config.history_length > 0 {
            history.max_len = config.history_length;
            // Trim if the new limit is smaller than current size
            while history.positions.len() > history.max_len {
                history.positions.remove(0);
            }
        }

        let pos = transform.translation().truncate();
        history.positions.push(pos);

        // Enforce max length
        while history.positions.len() > history.max_len {
            history.positions.remove(0);
        }
    }
}

// ============================================================
// History rendering system (T11-A)
// ============================================================

/// Renders trajectory trails using Gizmos with fading opacity.
fn render_trajectories(
    config: Res<TrajectoryConfig>,
    bodies: Query<(&CelestialBody, &TrajectoryHistory)>,
    mut gizmos: Gizmos,
) {
    if !config.enabled {
        return;
    }

    for (body, history) in bodies.iter() {
        let positions = &history.positions;
        let total = positions.len();
        if total < 2 {
            continue;
        }

        let [r, g, b] = body.color;

        // Draw segments oldest -> newest with interpolated alpha
        for i in 0..(total - 1) {
            let from = positions[i];
            let to = positions[i + 1];
            // Normalized position of the newer endpoint of this segment
            let t = (i + 1) as f32 / (total - 1) as f32;
            let alpha = 0.05 + t * 0.55; // ranges 0.05 .. 0.6
            gizmos.line_2d(from, to, Color::srgba(r, g, b, alpha));
        }
    }
}

// ============================================================
// JS sync system (T11-A)
// ============================================================

/// Syncs the in-Rust config to the JS-accessible snapshot whenever it changes.
fn sync_trajectory_config_to_js(
    config: Res<TrajectoryConfig>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        if config.is_changed() {
            // The JS side uses these field names:
            //   trail_length     -> history_length
            //   prediction_steps -> prediction_steps
            //   trails_visible   -> enabled
            let json = format!(
                r#"{{"trail_length":{},"prediction_steps":{},"trails_visible":{}}}"#,
                config.history_length,
                config.prediction_steps,
                config.enabled,
            );
            if let Ok(mut shared) = crate::js_bridge::TRAJECTORY_CONFIG_SNAPSHOT.lock() {
                *shared = json;
            }
        }
    }
}

/// Applies config changes sent from JavaScript via set_trajectory_config().
fn apply_js_trajectory_config(
    mut config: ResMut<TrajectoryConfig>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let cmd = if let Ok(mut c) = crate::js_bridge::TRAJECTORY_CONFIG_CMD.lock() {
            c.take()
        } else {
            None
        };

        if let Some(json_str) = cmd {
            // Parse the JSON and apply recognised fields
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(val) = parsed.get("trail_length").and_then(|v| v.as_u64()) {
                    config.history_length = (val as usize).clamp(100, 2000);
                }
                if let Some(val) = parsed.get("prediction_steps").and_then(|v| v.as_u64()) {
                    config.prediction_steps = (val as usize).clamp(50, 1000);
                }
                if let Some(val) = parsed.get("trails_visible").and_then(|v| v.as_bool()) {
                    config.enabled = val;
                }
            }
        }
    }
}

// ============================================================
// Plugin
// ============================================================

/// Plugin for all trajectory systems (history + prediction).
///
/// Registers resources and systems for:
/// - History trail sampling & rendering (T11-A)
/// - RK4 prediction trail for selected body (T11-B)
pub struct TrajectoryPlugin;

impl Plugin for TrajectoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrajectoryConfig>()
            .init_resource::<TrajectoryFrameCounter>()
            .init_resource::<PredictionTrail>()
            .add_systems(Update, (sample_trajectory, prediction_system, apply_js_trajectory_config))
            .add_systems(
                PostUpdate,
                (
                    render_trajectories,
                    prediction_render_system,
                    sync_trajectory_config_to_js,
                ),
            );
    }
}
