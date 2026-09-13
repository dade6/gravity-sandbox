use avian2d::prelude::*;
use bevy::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::components::trajectory::{
    PredictionTrail, TrajectoryConfig, TrajectoryHistory, TrajectoryTickCounter,
};
use crate::systems::persistence::{GravitationalConstant, SOFTENING};
use crate::systems::selection::SelectedBody;

// ============================================================
// RK4 N-body integrator (T11-B)
// ============================================================

/// Compute the N-body gravitational acceleration on a body at position `pos`
/// due to all other bodies. Other bodies' positions are treated as fixed.
///
/// `g` is the live gravitational constant (from `GravitationalConstant`,
/// editable in the settings panel) so the prediction always matches the
/// real simulation.
fn nbody_acceleration(
    pos: Vec2,
    _vel: Vec2,
    g: f32,
    bodies: &[(f32, Vec2, Vec2)],
    self_idx: usize,
) -> Vec2 {
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
        let acc_mag = g * mj / (dist_sq + SOFTENING * SOFTENING);
        let direction = delta / dist;
        acc += direction * acc_mag;
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
///
/// `g` is the live gravitational constant and `dt` the physics timestep
/// actually used by the simulation (see `prediction_system`).
pub fn rk4_integrate(
    bodies: &[(f32, Vec2, Vec2)],
    dt: f32,
    steps: usize,
    target_idx: usize,
    g: f32,
) -> Vec<Vec2> {
    let mut positions = Vec::with_capacity(steps);
    let (mut pos, mut vel) = (bodies[target_idx].1, bodies[target_idx].2);

    for _ in 0..steps {
        // k1
        let k1_v = vel;
        let k1_a = nbody_acceleration(pos, vel, g, bodies, target_idx);

        // k2
        let k2_v = vel + k1_a * (dt / 2.0);
        let k2_a = nbody_acceleration(pos + k1_v * (dt / 2.0), k2_v, g, bodies, target_idx);

        // k3
        let k3_v = vel + k2_a * (dt / 2.0);
        let k3_a = nbody_acceleration(pos + k2_v * (dt / 2.0), k3_v, g, bodies, target_idx);

        // k4
        let k4_v = vel + k3_a * dt;
        let k4_a = nbody_acceleration(pos + k3_v * dt, k4_v, g, bodies, target_idx);

        // Weighted average (RK4)
        pos += (k1_v + k2_v * 2.0 + k3_v * 2.0 + k4_v) * (dt / 6.0);
        vel += (k1_a + k2_a * 2.0 + k3_a * 2.0 + k4_a) * (dt / 6.0);

        positions.push(pos);
    }

    positions
}

// ============================================================
// Physics dt (testable helper)
// ============================================================

/// The dt Avian actually advances the simulation by each physics tick:
/// Bevy's fixed timestep scaled by the physics relative speed (the same
/// formula as `run_physics_schedule` in Avian's source).
///
/// Extracted as a pure function so tests can verify it.
pub fn physics_dt(fixed_timestep_secs: f64, physics_relative_speed: f64) -> f32 {
    (fixed_timestep_secs * physics_relative_speed) as f32
}

// ============================================================
// Prediction system (T11-B, fix A)
// ============================================================

/// System that computes the prediction trail for the selected body.
/// Runs in `Update` to refresh every frame.
///
/// Fix A: the gravitational constant now comes from the live
/// `GravitationalConstant` resource (editable in settings), and the
/// integration dt matches the real physics timestep: Bevy's fixed
/// timestep (64 Hz) scaled by the physics relative speed — the same
/// dt Avian advances the simulation by each tick.
pub fn prediction_system(
    selected: Res<SelectedBody>,
    config: Res<TrajectoryConfig>,
    grav: Res<GravitationalConstant>,
    fixed_time: Res<Time<Fixed>>,
    physics_time: Res<Time<Physics>>,
    bodies: Query<(Entity, &CelestialBody, &GlobalTransform, &LinearVelocity)>,
    mut trail: ResMut<PredictionTrail>,
) {
    crate::mark_system("prediction_system");

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

    // Real physics dt: Bevy fixed timestep (default 64 Hz) scaled by the
    // physics relative speed. This is exactly the dt Avian advances by
    // each physics tick (`run_physics_schedule` in Avian's source).
    let dt = physics_dt(
        fixed_time.timestep().as_secs_f64(),
        physics_time.relative_speed_f64(),
    );

    let predicted = rk4_integrate(
        &body_states,
        dt,
        config.prediction_steps,
        target_idx,
        grav.0,
    );
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
// History sampling system (T11-A, fix B + C)
// ============================================================

/// Samples body positions every N *physics ticks* and stores them in
/// `TrajectoryHistory`.
///
/// Fix C: the system runs in `PhysicsSystems::Last` (i.e. once per real
/// physics tick, after Avian has written back positions). Sampling is
/// therefore proportional to simulated time, not to render frames:
/// at speed 4x the trail spans 4x more sim-time per rendered frame
/// consistently, and pausing physics stops sampling automatically
/// (Avian does not run the PhysicsSchedule when `Time<Physics>` is paused).
fn sample_trajectory(
    mut counter: ResMut<TrajectoryTickCounter>,
    config: Res<TrajectoryConfig>,
    mut bodies: Query<(&Position, &mut TrajectoryHistory), With<CelestialBody>>,
) {
    crate::mark_system("sample_trajectory");

    counter.0 += 1;
    if counter.0 % config.sample_interval as u64 != 0 {
        return;
    }

    for (position, mut history) in bodies.iter_mut() {
        // Sync per-entity max_len from the global config
        if history.max_len != config.history_length && config.history_length > 0 {
            history.set_max_len(config.history_length);
        }

        history.push_sample(position.0);
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
fn sync_trajectory_config_to_js(config: Res<TrajectoryConfig>) {
    #[cfg(target_arch = "wasm32")]
    {
        if config.is_changed() {
            // The JS side uses these field names:
            //   trail_length     -> history_length
            //   prediction_steps -> prediction_steps
            //   trails_visible   -> enabled
            let json = format!(
                r#"{{"trail_length":{},"prediction_steps":{},"trails_visible":{}}}"#,
                config.history_length, config.prediction_steps, config.enabled,
            );
            if let Ok(mut shared) = crate::js_bridge::TRAJECTORY_CONFIG_SNAPSHOT.lock() {
                *shared = json;
            }
        }
    }
}

/// Applies config changes sent from JavaScript via set_trajectory_config().
fn apply_js_trajectory_config(mut config: ResMut<TrajectoryConfig>) {
    crate::mark_system("apply_js_trajectory_config");

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
///
/// Sampling runs inside Avian's `PhysicsSystems::Last` set (FixedPostUpdate):
/// once per real physics tick, after position writeback.
pub struct TrajectoryPlugin;

impl Plugin for TrajectoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrajectoryConfig>()
            .init_resource::<TrajectoryTickCounter>()
            .init_resource::<PredictionTrail>()
            .add_systems(Update, (prediction_system, apply_js_trajectory_config))
            .add_systems(
                PhysicsSchedule,
                sample_trajectory.after(PhysicsStepSystems::Last),
            )
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

// ============================================================
// Tests (fix A + B + C)
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::celestial::{BodyType, CelestialBody};
    use crate::systems::selection::SelectedBody;
    use crate::systems::timeline::SimulationState;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    // ---------- Fix B: ring buffer ----------

    #[test]
    fn history_ring_buffer_o1() {
        let mut h = TrajectoryHistory::default();
        h.set_max_len(3);
        for i in 0..10 {
            h.push_sample(Vec2::new(i as f32, 0.0));
            assert!(h.positions.len() <= 3, "buffer must never exceed max_len");
        }
        assert_eq!(h.positions.len(), 3);
        // Oldest samples evicted, order preserved (oldest -> newest)
        assert_eq!(h.positions[0], Vec2::new(7.0, 0.0));
        assert_eq!(h.positions[2], Vec2::new(9.0, 0.0));
        // Shrinking the cap evicts oldest samples
        h.set_max_len(2);
        assert_eq!(h.positions.len(), 2);
        assert_eq!(h.positions[0], Vec2::new(8.0, 0.0));
    }

    // ---------- Fix A: live G + real dt ----------

    #[test]
    fn physics_dt_matches_avian_formula() {
        // Bevy default fixed timestep = 64 Hz -> 1/64 s
        assert_eq!(physics_dt(1.0 / 64.0, 1.0), 1.0 / 64.0);
        // Speed 4x -> dt 4x (Avian scales the fixed timestep by relative speed)
        assert_eq!(physics_dt(1.0 / 64.0, 4.0), 4.0 / 64.0);
    }

    #[test]
    fn prediction_follows_live_gravity_constant() {
        let mut app = App::new();
        app.init_resource::<SelectedBody>()
            .init_resource::<TrajectoryConfig>()
            .init_resource::<GravitationalConstant>()
            .init_resource::<PredictionTrail>()
            .insert_resource(Time::<Fixed>::default())
            .insert_resource(Time::<Physics>::default())
            .add_systems(Update, prediction_system);

        // Two bodies: heavy star at origin, planet nearby
        let star = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "Star".into(),
                    body_type: BodyType::Star,
                    mass: 500_000.0,
                    radius: 40.0,
                    color: [1.0, 0.9, 0.4],
                    luminous: true,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
                GlobalTransform::from_xyz(0.0, 0.0, 0.0),
                LinearVelocity(Vec2::ZERO),
            ))
            .id();
        let planet = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "Planet".into(),
                    body_type: BodyType::Planet,
                    mass: 10.0,
                    radius: 8.0,
                    color: [0.4, 0.6, 1.0],
                    luminous: false,
                },
                Transform::from_xyz(300.0, 0.0, 0.0),
                GlobalTransform::from_xyz(300.0, 0.0, 0.0),
                LinearVelocity(Vec2::new(0.0, 40.0)),
            ))
            .id();
        app.world_mut().resource_mut::<SelectedBody>().0 = Some(planet);

        app.update();
        let trail_g1 = app.world().resource::<PredictionTrail>().0.clone();
        assert!(!trail_g1.is_empty());

        // Double the gravitational constant (as the settings panel does)
        app.world_mut().resource_mut::<GravitationalConstant>().0 = 10000.0;
        app.update();
        let trail_g2 = app.world().resource::<PredictionTrail>().0.clone();

        // Different G must produce a different predicted trajectory.
        let differs = trail_g1.iter().zip(trail_g2.iter()).any(|(a, b)| a != b);
        assert!(
            differs,
            "prediction must react to GravitationalConstant changes"
        );

        let _ = (star, planet);
    }

    // ---------- Fix C: sampling per physics tick ----------

    fn avian_test_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .insert_resource(GravitationalConstant(5000.0))
        .init_resource::<SelectedBody>()
        .init_resource::<TrajectoryConfig>()
        .init_resource::<TrajectoryTickCounter>()
        .init_resource::<PredictionTrail>()
        // Only the sampling system (no render systems: they need GizmoConfigStore)
        .add_systems(
            PhysicsSchedule,
            sample_trajectory.after(PhysicsStepSystems::Last),
        )
        // One fixed update per app.update() (timestep = Bevy fixed 64 Hz)
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Time::<Fixed>::default().timestep(),
        ));
        // Avian plugins register diagnostics resources in finish() — without
        // this call those systems panic (same as Avian's own test harness).
        app.finish();
        app
    }

    #[test]
    fn sampling_runs_per_physics_tick_not_per_frame() {
        let mut app = avian_test_app();
        let entity = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "P".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius: 10.0,
                    color: [0.5, 0.5, 0.8],
                    luminous: false,
                },
                Transform::from_xyz(200.0, 0.0, 0.0),
                RigidBody::Dynamic,
                Collider::circle(10.0),
                Mass(100.0),
                LinearVelocity(Vec2::new(0.0, 30.0)),
                TrajectoryHistory::default(),
            ))
            .id();

        // Prime the clock: the very first app.update() advances the time
        // resources but does not run any fixed update yet (accumulator empty).
        app.update();
        let ticks_before = app.world().resource::<TrajectoryTickCounter>().0;

        // 8 fixed updates, sample_interval = 2 -> exactly 4 samples
        for _ in 0..8 {
            app.update();
        }
        let len_after_8_ticks = app
            .world()
            .entity(entity)
            .get::<TrajectoryHistory>()
            .unwrap()
            .positions
            .len();

        // 8 more updates: +4 more samples (total is differential, not
        // dependent on frame count)
        for _ in 0..8 {
            app.update();
        }
        let len_after_16 = app
            .world()
            .entity(entity)
            .get::<TrajectoryHistory>()
            .unwrap()
            .positions
            .len();

        let ticks = app.world().resource::<TrajectoryTickCounter>().0 - ticks_before;
        assert_eq!(ticks, 16, "16 updates at 64Hz step = 16 physics ticks");
        assert_eq!(len_after_8_ticks, 4, "8 ticks / interval 2 = 4 samples");
        assert_eq!(
            len_after_16 - len_after_8_ticks,
            4,
            "8 more ticks -> +4 more samples"
        );
    }

    #[test]
    fn paused_physics_stops_sampling() {
        let mut app = avian_test_app();
        let entity = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "P".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius: 10.0,
                    color: [0.5, 0.5, 0.8],
                    luminous: false,
                },
                Transform::from_xyz(200.0, 0.0, 0.0),
                RigidBody::Dynamic,
                Collider::circle(10.0),
                Mass(100.0),
                LinearVelocity(Vec2::new(0.0, 30.0)),
                TrajectoryHistory::default(),
            ))
            .id();
        let _ = entity;

        // Pause physics, run many frames: no samples must appear
        app.world_mut().resource_mut::<Time<Physics>>().pause();
        for _ in 0..10 {
            app.update();
        }
        let len = app
            .world()
            .entity(entity)
            .get::<TrajectoryHistory>()
            .unwrap()
            .positions
            .len();
        assert_eq!(len, 0, "paused physics must not sample");
    }
}
