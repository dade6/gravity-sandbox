use avian2d::prelude::*;
use bevy::prelude::*;
use std::collections::VecDeque;

use crate::components::celestial::CelestialBody;
use crate::components::trajectory::{
    GhostBody, GhostPrediction, TrajectoryConfig, TrajectoryHistory, TrajectoryTickCounter,
};
use crate::systems::persistence::GravitationalConstant;
use crate::systems::selection::SelectedBody;
use crate::systems::timeline::SimulationState;

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
            //   prediction_steps -> prediction_steps (back-compat, ignored by ghost)
            //   horizon_seconds  -> horizon_seconds (ghost horizon, T22-D)
            //   trails_visible   -> enabled
            let json = format!(
                r#"{{"trail_length":{},"prediction_steps":{},"horizon_seconds":{},"trails_visible":{}}}"#,
                config.history_length,
                config.prediction_steps,
                config.horizon_seconds,
                config.enabled,
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
                // T22-D/E: ghost horizon in sim-seconds (clamp 10–3600).
                // The dirty flag on horizon change is raised by
                // `ghost_dirty_triggers`, so no GhostPrediction access here.
                if let Some(val) = parsed.get("horizon_seconds").and_then(|v| v.as_f64()) {
                    config.horizon_seconds = (val as f32).clamp(10.0, 3600.0);
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

/// Plugin for all trajectory systems (history + ghost forecast).
///
/// Registers resources and systems for:
/// - History trail sampling & rendering (T11-A)
/// - Ghost N-body forecast: snapshot + progressive compute (T22-B),
///   collisions/merge (T22-C), sliding window in Run + dashed
///   rendering (T22-E). The old RK4 `prediction_system` was REPLACED
///   by the ghost (ADR 0001 Dec. 10).
///
/// Sampling runs inside Avian's `PhysicsSystems::Last` set (FixedPostUpdate):
/// once per real physics tick, after position writeback.
pub struct TrajectoryPlugin;

impl Plugin for TrajectoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrajectoryConfig>()
            .init_resource::<TrajectoryTickCounter>()
            .init_resource::<GhostPrediction>()
            .add_systems(Update, apply_js_trajectory_config)
            .add_systems(
                Update,
                (
                    ghost_dirty_triggers,
                    ghost_snapshot_system,
                    ghost_compute_system,
                    ghost_sliding_window_system,
                )
                    .chain(),
            )
            .add_systems(
                PhysicsSchedule,
                sample_trajectory.after(PhysicsStepSystems::Last),
            )
            .add_systems(
                PostUpdate,
                (
                    render_trajectories,
                    render_ghost_predictions,
                    sync_trajectory_config_to_js,
                ),
            );
    }
}

// ============================================================
// Ghost N-body forecast — faithful integrator (T22-B, ADR 0001 Dec. 1-4)
// ============================================================

/// Ticks of ghost future integrated per frame (Dec. 1: progressive compute).
pub const GHOST_TICKS_PER_FRAME: usize = 256;

/// Max wall-clock budget per frame chunk before a warn is logged (ms).
/// No adaptivity: fixed chunk, just observability (T22-B scope).
/// NOTE: wall-clock timing via `std::time::Instant` is NOT used here on
/// purpose — `Instant::now()` panics on wasm32-unknown-unknown
/// (white page, "Unreachable code should not be executed" in
/// `ghost_compute_system`, v0.14.89). Fixed chunk, no timing.
#[allow(dead_code)]
const GHOST_CHUNK_WARN_MS: u128 = 4;

/// Faithful ghost tick (ADR-4, v0.14.104): EXACT replica of
/// `substep_gravity_system` — force re-evaluated from current positions at
/// EVERY substep (same formula via `pair_force`, same `dist_sq < 1.0` skip,
/// same i<j action/reaction order), each followed by one semi-implicit
/// Euler step with `dt_tick/6` — the same 6 substeps Avian runs per tick
/// (`SubstepCount` default 6).
///
/// (Fino a v0.14.103 la forza era valutata 1x/tick e congelata per i 6
/// substep, come il vecchio `gravity_system`+ConstantForce: pompaggio
/// secolare +27/orbita a r=200. Ora forza fresca per-substep.)
///
/// Mass subtlety (bug v0.14.100 investigation, CLOSED): Avian integrates ConstantForce
/// as acceleration via `ComputedMass.inverse()`. Verified headless
/// (`ghost_total_mass_matches_avian_computed_mass`): with explicit `Mass`
/// present, `ComputedMass == Mass` EXACTLY (explicit mass REPLACES
/// collider auto-mass). Our bodies always spawn with explicit `Mass`
/// (== `CelestialBody.mass`, synced on edit), so dividing by `body.mass`
/// here is already the identical computation the sim performs. No
/// compensation needed.
/// Pure function over plain data: no Avian components on ghosts (Dec. 2).
pub fn ghost_step_tick(ghosts: &mut [GhostBody], g: f32, dt_tick: f32) {
    use crate::systems::gravity::pair_force;

    let n = ghosts.len();
    if n == 0 {
        return;
    }
    // --- 6 substeps: fresh force eval + one Euler step each ---
    // Dead (merged-away) ghosts exert and feel no force (T22-C) and are
    // frozen in place.
    let dt_sub = dt_tick / 6.0;
    let mut forces = vec![Vec2::ZERO; n];
    for _ in 0..6 {
        for f in forces.iter_mut() {
            *f = Vec2::ZERO;
        }
        for i in 0..n {
            if !ghosts[i].alive {
                continue;
            }
            for j in (i + 1)..n {
                if !ghosts[j].alive {
                    continue;
                }
                let delta = ghosts[j].pos - ghosts[i].pos;
                let dist_sq = delta.length_squared();
                if dist_sq < 1.0 {
                    continue;
                }
                let force_vec = pair_force(delta, dist_sq, ghosts[i].mass, ghosts[j].mass, g);
                forces[i] += force_vec;
                forces[j] -= force_vec;
            }
        }
        for (body, force) in ghosts.iter_mut().zip(forces.iter()) {
            if !body.alive {
                continue;
            }
            let acc = *force / body.mass;
            body.vel += acc * dt_sub;
            body.pos += body.vel * dt_sub;
        }
    }
}

/// Total inertial mass Avian uses for a body: EXPLICIT `Mass` REPLACES the
/// collider auto-mass (verified: `ComputedMass == Mass` whenever `Mass` is
/// present — Avian's own `mass_properties_rb_collider_with_set_mass` test
/// asserts exactly this). `ghost_total_mass` is therefore the identity, kept
/// as a named choke point (with its contract test below) so any future Avian
/// change to this rule breaks loudly instead of drifting the forecast.
pub fn ghost_total_mass(mass: f32, _radius: f32) -> f32 {
    mass
}

/// Snapshot helper shared by `ghost_snapshot_system` and
/// `ghost_compute_system`: clones every live body into
/// `GhostPrediction.bodies`, sizes one trail deque per ghost, records the
/// horizon and anchor tick, and clears the dirty flag.
fn take_ghost_snapshot(
    pred: &mut GhostPrediction,
    snapshot: Vec<GhostBody>,
    horizon_ticks: usize,
    anchor_tick: u64,
) {
    let n = snapshot.len();
    pred.bodies = snapshot;
    pred.trails = vec![VecDeque::new(); n];
    pred.collision_markers.clear();
    pred.computed_ticks = 0;
    pred.horizon_ticks = horizon_ticks;
    pred.anchor_tick = anchor_tick;
    pred.dirty = false;
}

/// Snapshot system (T22-B.1): while the sim is paused (editing is only
/// allowed paused — see `tools.rs`), a dirty forecast is re-anchored to
/// the live bodies: Entity + pos (GlobalTransform) + vel (LinearVelocity)
/// + mass (CelestialBody) + radius + color.
pub fn ghost_snapshot_system(
    sim_state: Res<SimulationState>,
    config: Res<TrajectoryConfig>,
    counter: Res<TrajectoryTickCounter>,
    bodies: Query<(Entity, &CelestialBody, &GlobalTransform, &LinearVelocity)>,
    mut pred: ResMut<GhostPrediction>,
) {
    crate::mark_system("ghost_snapshot_system");

    if !sim_state.paused {
        return;
    }
    if !pred.dirty {
        return;
    }
    let snapshot: Vec<GhostBody> = bodies
        .iter()
        .map(|(e, body, xform, vel)| {
            let [r, g, b] = body.color;
            GhostBody {
                entity: e,
                pos: xform.translation().truncate(),
                vel: vel.0,
                mass: body.mass,
                radius: body.radius,
                color: Color::srgb(r, g, b),
                alive: true,
            }
        })
        .collect();
    take_ghost_snapshot(pred.as_mut(), snapshot, config.horizon_ticks(), counter.0);
}

/// Dirty triggers (T22-B.4, Dec. 3 total invalidation): body drag/edit
/// (Transform/CelestialBody/LinearVelocity change while paused),
/// add/remove body, G change, sim-speed change, HORIZON change (T22-E:
/// the settings panel / JS bridge / preset load only write the resource,
/// this trigger owns the restart) → `mark_dirty()` so the
/// forecast restarts from zero and regrows progressively.
pub fn ghost_dirty_triggers(
    added: Query<Entity, Added<CelestialBody>>,
    changed_body: Query<Entity, Changed<CelestialBody>>,
    changed_physics: Query<
        Entity,
        (
            Or<(Changed<Transform>, Changed<LinearVelocity>)>,
            With<CelestialBody>,
        ),
    >,
    mut removed: RemovedComponents<CelestialBody>,
    grav: Res<GravitationalConstant>,
    sim_state: Res<SimulationState>,
    config: Res<TrajectoryConfig>,
    mut last_speed: Local<Option<f32>>,
    mut last_horizon_ticks: Local<Option<usize>>,
    mut pred: ResMut<GhostPrediction>,
) {
    crate::mark_system("ghost_dirty_triggers");

    let mut dirty = false;
    if !added.is_empty() {
        dirty = true;
    }
    if removed.read().count() > 0 {
        dirty = true;
    }
    // User edits to body properties (mass, radius, ...) always invalidate,
    // paused or running.
    if !changed_body.is_empty() {
        dirty = true;
    }
    // NOTE: Avian rewrites Transform/LinearVelocity EVERY physics tick, so
    // these Changed flags are hot continuously during Run. Gating on paused:
    // they mean user drag/edit only when paused (tools are pause-only), while
    // in Run they are pure physics noise that must NOT invalidate the sliding
    // window (bug v0.14.90: forecast wiped every frame in Run, future curves
    // vanished on Play and regrew on pause).
    // The query is ALSO restricted to bodies (With<CelestialBody>): cameras,
    // parallax layers and the minimap camera rewrite their Transform every
    // frame, and `sync_sprite_z` rewrote body z unconditionally (fixed to
    // write-on-change in firefly_bridge.rs). Without the filter + the
    // conditional write, Changed<Transform> stayed hot while paused and the
    // ghost restarted (snapshot + 1 chunk) every frame — computed froze at
    // 256 ticks at any horizon, so the horizon setting had no visible effect
    // (bug v0.14.93).
    if sim_state.paused && !changed_physics.is_empty() {
        dirty = true;
    }
    if grav.is_changed() {
        dirty = true;
    }
    match *last_speed {
        Some(s) if (s - sim_state.speed).abs() < f32::EPSILON => {}
        _ => {
            dirty = true;
            *last_speed = Some(sim_state.speed);
        }
    }
    // Horizon change (T22-D writes, T22-E restarts): compare converted
    // ticks so any source (settings panel, JS bridge, preset load) funnels
    // through the same total invalidation.
    let horizon_ticks = config.horizon_ticks();
    match *last_horizon_ticks {
        Some(h) if h == horizon_ticks => {}
        _ => {
            dirty = true;
            *last_horizon_ticks = Some(horizon_ticks);
        }
    }
    if dirty && !pred.dirty {
        pred.mark_dirty();
    }
}

// ============================================================
// Ghost collisions + perfectly-inelastic merge (T22-C, ADR 0001 Dec. 5-6)
// ============================================================

/// Mean of two ghost colors in sRGB space, used for collision markers.
pub fn ghost_mix_colors(a: Color, b: Color) -> Color {
    let x = a.to_srgba();
    let y = b.to_srgba();
    Color::srgba(
        (x.red + y.red) * 0.5,
        (x.green + y.green) * 0.5,
        (x.blue + y.blue) * 0.5,
        (x.alpha + y.alpha) * 0.5,
    )
}

/// Pairwise collision check over the ghost bodies, run once per ghost tick
/// AFTER integration (Dec. 5).
///
/// At the FIRST overlap of a pair (`dist < r1+r2`):
/// - one marker is pushed to `collision_markers` at the barycenter, colored
///   with the mean of the two ghost colors;
/// - the pair merges perfectly inelastically (Dec. 6, coherent with the
///   real sim's `Restitution 0.0`): `mass = m1+m2`, `pos` = barycenter,
///   `vel` = momentum conservation, `radius = sqrt(r1²+r2²)` (area
///   conserved), `color` = the more massive body's (tie → lower index).
///   The absorbed ghost is flagged `alive = false`; its trail is closed at
///   the merge point (the merged position is appended to it) while the
///   survivor's trail keeps growing in the compute loop.
/// - computation NEVER stops (Davide's explicit request): merged ghosts
///   keep integrating and later ticks may merge again (N sequential
///   collisions with 3+ bodies, even within the same tick).
pub fn ghost_check_collisions(pred: &mut GhostPrediction) {
    let n = pred.bodies.len();
    for i in 0..n {
        for j in (i + 1)..n {
            // Re-read liveness: either side may have merged earlier this tick.
            if !pred.bodies[i].alive || !pred.bodies[j].alive {
                continue;
            }
            let (pi, pj, ri, rj) = (
                pred.bodies[i].pos,
                pred.bodies[j].pos,
                pred.bodies[i].radius,
                pred.bodies[j].radius,
            );
            if pi.distance(pj) >= ri + rj {
                continue;
            }
            // --- Collision: snapshot both sides, then merge. ---
            let (mi, mj) = (pred.bodies[i].mass, pred.bodies[j].mass);
            let total = mi + mj;
            // Guard against zero-mass ghosts (should not happen from the
            // snapshot, but avoid a NaN merge if it ever does).
            if total <= 0.0 {
                continue;
            }
            let merged_pos = (pi * mi + pj * mj) / total;
            let merged_vel = (pred.bodies[i].vel * mi + pred.bodies[j].vel * mj) / total;
            let merged_radius = (ri * ri + rj * rj).sqrt();
            // Survivor = more massive (tie → lower index i). DECISIONE orchestrator.
            let (s, d) = if mj > mi { (j, i) } else { (i, j) };
            let survivor_color = pred.bodies[s].color;
            let marker = ghost_mix_colors(pred.bodies[i].color, pred.bodies[j].color);
            pred.collision_markers.push((merged_pos, marker));
            pred.bodies[s].mass = total;
            pred.bodies[s].pos = merged_pos;
            pred.bodies[s].vel = merged_vel;
            pred.bodies[s].radius = merged_radius;
            pred.bodies[s].color = survivor_color;
            pred.bodies[d].alive = false;
            // Close the absorbed trail AT the merge point; the survivor's
            // trail continues via the normal per-tick push. Trails stay
            // 1:1 with bodies by index (dead trails simply stop growing).
            if pred.trails.len() == n {
                pred.trails[d].push_back(merged_pos);
            }
        }
    }
}

/// Total alive-ghost momentum (dead ghosts are merged mass, not missing mass).
pub fn ghost_alive_momentum(bodies: &[GhostBody]) -> Vec2 {
    bodies
        .iter()
        .filter(|b| b.alive)
        .map(|b| b.mass * b.vel)
        .fold(Vec2::ZERO, |a, v| a + v)
}
/// snapshot) re-anchor + restart from zero; otherwise integrate one fixed
/// `GHOST_TICKS_PER_FRAME` chunk per frame, appending 1 point per ghost
/// per tick, until `horizon_ticks`. Paused-only: in Run the sliding window
/// (`ghost_sliding_window_system`, T22-E) owns the forecast instead.
pub fn ghost_compute_system(
    sim_state: Res<SimulationState>,
    config: Res<TrajectoryConfig>,
    grav: Res<GravitationalConstant>,
    fixed_time: Res<Time<Fixed>>,
    physics_time: Res<Time<Physics>>,
    counter: Res<TrajectoryTickCounter>,
    bodies: Query<(Entity, &CelestialBody, &GlobalTransform, &LinearVelocity)>,
    mut pred: ResMut<GhostPrediction>,
) {
    crate::mark_system("ghost_compute_system");

    if !sim_state.paused {
        return;
    }
    // Re-anchor on dirty or when no snapshot exists yet ("assente").
    if pred.dirty || pred.bodies.is_empty() {
        let snapshot: Vec<GhostBody> = bodies
            .iter()
            .map(|(e, body, xform, vel)| {
                let [r, g, b] = body.color;
                GhostBody {
                    entity: e,
                    pos: xform.translation().truncate(),
                    vel: vel.0,
                    mass: body.mass,
                    radius: body.radius,
                    color: Color::srgb(r, g, b),
                    alive: true,
                }
            })
            .collect();
        if snapshot.is_empty() {
            return;
        }
        // A clean (non-dirty) but empty prediction with live bodies means
        // "assente": snapshot without wiping — take_ghost_snapshot starts
        // from zero either way, which is the required restart semantics.
        take_ghost_snapshot(pred.as_mut(), snapshot, config.horizon_ticks(), counter.0);
    }
    if pred.bodies.is_empty() || pred.computed_ticks >= pred.horizon_ticks {
        return;
    }
    // Same dt Avian advances per tick (includes current sim speed).
    let dt_tick = physics_dt(
        fixed_time.timestep().as_secs_f64(),
        physics_time.relative_speed_f64(),
    );
    let g = grav.0;
    let remaining = pred.horizon_ticks - pred.computed_ticks;
    let chunk = remaining.min(GHOST_TICKS_PER_FRAME);
    for _ in 0..chunk {
        ghost_step_tick(&mut pred.bodies, g, dt_tick);
        // T22-C: collision check AFTER integration; never stops the forecast.
        ghost_check_collisions(pred.as_mut());
        // Only alive ghosts extend their trail; absorbed trails stay
        // frozen at the merge point (closed inside ghost_check_collisions).
        // Index loop (not zip of two borrows) so dead-trail close and
        // live-trail push coexist under the borrow checker.
        for idx in 0..pred.bodies.len() {
            if pred.bodies[idx].alive {
                let pos = pred.bodies[idx].pos;
                if let Some(trail) = pred.trails.get_mut(idx) {
                    trail.push_back(pos);
                }
            }
        }
    }
    pred.computed_ticks += chunk;
}

// ============================================================
// Ghost sliding window in Run + dashed rendering (T22-E, ADR 0001 Dec. 8-9)
// ============================================================

/// Advance the sliding window by ONE consumed physics tick (Dec. 8).
///
/// Drops the oldest point of every live trail (`pop_front`), integrates one
/// ghost tick from the frontier state, runs the collision check, and appends
/// the new point (`push_back`). Length stays constant — no total recompute.
/// Dead (merged-away) trails stay frozen and are never touched.
///
/// If the forecast is still growing (`computed_ticks < horizon_ticks`), the
/// pop is skipped so a partial forecast is never eaten: it grows until the
/// horizon is reached, then slides.
///
/// NEVER re-anchors to the live bodies: the window slides purely from the
/// ghost frontier, so real-vs-predicted divergence stays visible.
pub fn ghost_slide_tick(pred: &mut GhostPrediction, g: f32, dt_tick: f32) {
    ghost_step_tick(&mut pred.bodies, g, dt_tick);
    ghost_check_collisions(pred);
    let growing = pred.computed_ticks < pred.horizon_ticks;
    for idx in 0..pred.bodies.len() {
        if !pred.bodies[idx].alive {
            continue;
        }
        if let Some(trail) = pred.trails.get_mut(idx) {
            if !growing && !trail.is_empty() {
                trail.pop_front();
            }
            trail.push_back(pred.bodies[idx].pos);
        }
    }
    if growing {
        pred.computed_ticks += 1;
    }
}

/// Sliding-window driver, runs in `Update` (chained after the compute
/// system, which is paused-only so the two never fight).
///
/// - While paused: records the pause edge, nothing else (the progressive
///   compute owns the forecast).
/// - On the pause → Run edge: saves `anchor_tick` (= current
///   `TrajectoryTickCounter`) and the last-seen tick.
/// - In Run: advances one window tick per consumed physics tick
///   (`counter - last_seen`). Per-frame work is capped at
///   `GHOST_TICKS_PER_FRAME`; leftover ticks are caught up over the next
///   frames (the cap matches the progressive-compute budget).
/// - If the forecast is dirty in Run (e.g. horizon changed mid-Run), it is
///   re-anchored to the live bodies immediately — a conditions change
///   sanctions the total recompute (Dec. 3/8) — then regrows progressively.
pub fn ghost_sliding_window_system(
    sim_state: Res<SimulationState>,
    config: Res<TrajectoryConfig>,
    grav: Res<GravitationalConstant>,
    fixed_time: Res<Time<Fixed>>,
    physics_time: Res<Time<Physics>>,
    counter: Res<TrajectoryTickCounter>,
    bodies: Query<(Entity, &CelestialBody, &GlobalTransform, &LinearVelocity)>,
    mut pred: ResMut<GhostPrediction>,
    mut prev_paused: Local<bool>,
    mut last_tick: Local<Option<u64>>,
) {
    crate::mark_system("ghost_sliding_window_system");

    if sim_state.paused {
        *prev_paused = true;
        return;
    }
    // --- Running ---
    if *prev_paused {
        // Pause -> Run edge: anchor the window to the current tick.
        pred.anchor_tick = counter.0;
        *last_tick = Some(counter.0);
        *prev_paused = false;
    }
    let last = match *last_tick {
        Some(l) => l,
        None => {
            *last_tick = Some(counter.0);
            return;
        }
    };
    let mut pending = counter.0.saturating_sub(last);
    if pending == 0 {
        return;
    }
    // Dirty in Run (horizon change, G change, speed change...): re-anchor
    // to the live bodies now — conditions changed, total recompute applies.
    if pred.dirty || pred.bodies.is_empty() {
        if !pred.dirty && pred.bodies.is_empty() {
            // "Assente" with no snapshot yet: same re-anchor path.
        }
        let snapshot: Vec<GhostBody> = bodies
            .iter()
            .map(|(e, body, xform, vel)| {
                let [r, g, b] = body.color;
                GhostBody {
                    entity: e,
                    pos: xform.translation().truncate(),
                    vel: vel.0,
                    mass: body.mass,
                    radius: body.radius,
                    color: Color::srgb(r, g, b),
                    alive: true,
                }
            })
            .collect();
        if snapshot.is_empty() {
            *last_tick = Some(counter.0);
            return;
        }
        take_ghost_snapshot(pred.as_mut(), snapshot, config.horizon_ticks(), counter.0);
        *last_tick = Some(counter.0);
        return;
    }
    let dt_tick = physics_dt(
        fixed_time.timestep().as_secs_f64(),
        physics_time.relative_speed_f64(),
    );
    let g = grav.0;
    let mut advanced: u64 = 0;
    while pending > 0 && (advanced as usize) < GHOST_TICKS_PER_FRAME {
        ghost_slide_tick(pred.as_mut(), g, dt_tick);
        pending -= 1;
        advanced += 1;
    }
    *last_tick = Some(last + advanced);
}

/// Decimation stride for ghost rendering (Dec. 9): 1 drawn point every
/// `max(1, computed_ticks/1500)` trail points, so far-future density
/// (long horizons) stays bounded.
pub fn ghost_decimation_stride(computed_ticks: usize) -> usize {
    (computed_ticks / 1500).max(1)
}

/// Half-extent (world units) of the collision-marker X arms.
pub const GHOST_MARKER_HALF: f32 = 7.0;

/// Renders the ghost forecast in `PostUpdate` with the same `Gizmos` as the
/// historic trail (Dec. 9 + orchestrator decisions):
/// - one curve per ghost in its own color;
/// - the `SelectedBody` curve more opaque + dotted at sampled points
///   (Gizmos lines have a fixed width, so presence — not width —
///   carries the emphasis), other curves attenuated;
/// - FUTURE IS DASHED: alternating drawn/skipped segments; the historic
///   trail (`render_trajectories`) stays continuous;
/// - decimation via [`ghost_decimation_stride`];
/// - collision markers as an X cross (two segments) in the marker color.
pub fn render_ghost_predictions(
    config: Res<TrajectoryConfig>,
    selected: Res<SelectedBody>,
    pred: Res<GhostPrediction>,
    mut gizmos: Gizmos,
) {
    if !config.enabled {
        return;
    }
    if pred.trails.is_empty() {
        return;
    }
    let stride = ghost_decimation_stride(pred.computed_ticks);
    for (idx, trail) in pred.trails.iter().enumerate() {
        if trail.len() < 2 {
            continue;
        }
        let body = pred.bodies.get(idx);
        let base = body.map(|b| b.color).unwrap_or(Color::WHITE).to_srgba();
        let is_selected = body.map(|b| selected.0 == Some(b.entity)).unwrap_or(false);
        // Selected: opaque; others attenuated. Dead trails (frozen at the
        // merge point) render dimmer still — they are history, not future.
        let alive = body.map(|b| b.alive).unwrap_or(true);
        let alpha = if is_selected {
            0.85
        } else if alive {
            0.35
        } else {
            0.22
        };
        let color = Color::srgba(base.red, base.green, base.blue, alpha);
        // Sampled indices, always including the newest point.
        let mut sampled: Vec<Vec2> = trail.iter().step_by(stride).copied().collect();
        if let Some(last) = trail.back() {
            if sampled.last() != Some(last) {
                sampled.push(*last);
            }
        }
        // Dashed: draw even segments, skip odd ones.
        for (k, pair) in sampled.windows(2).enumerate() {
            if k % 2 == 0 {
                gizmos.line_2d(pair[0], pair[1], color);
            }
        }
        // Selected emphasis dots at sampled points.
        if is_selected {
            for p in sampled.iter().step_by(2) {
                gizmos.circle_2d(*p, 2.5, color);
            }
        }
    }
    // Collision markers: X cross in the marker color, full opacity.
    for (pos, marker) in pred.collision_markers.iter() {
        let m = marker.to_srgba();
        let c = Color::srgba(m.red, m.green, m.blue, 1.0);
        let h = GHOST_MARKER_HALF;
        gizmos.line_2d(*pos + Vec2::new(-h, -h), *pos + Vec2::new(h, h), c);
        gizmos.line_2d(*pos + Vec2::new(-h, h), *pos + Vec2::new(h, -h), c);
    }
}

// ============================================================
// Tests (fix B + C)
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

    // ---------- Fix A: real dt (ghost reuses the same helper) ----------

    #[test]
    fn physics_dt_matches_avian_formula() {
        // Bevy default fixed timestep = 64 Hz -> 1/64 s
        assert_eq!(physics_dt(1.0 / 64.0, 1.0), 1.0 / 64.0);
        // Speed 4x -> dt 4x (Avian scales the fixed timestep by relative speed)
        assert_eq!(physics_dt(1.0 / 64.0, 4.0), 4.0 / 64.0);
    }

    // NOTE (T22-E, ADR 0001 Dec. 10): the old RK4 `prediction_system` test
    // (`prediction_follows_live_gravity_constant`) was removed together with
    // the system it covered. Live-G reactivity is now owned by the ghost:
    // `ghost_dirty_triggers` marks dirty on G change (see
    // `ghost_horizon_change_marks_dirty` for the trigger pattern).

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

// ============================================================
// Ghost forecast tests (T22-B)
// ============================================================

#[cfg(test)]
mod ghost_tests {
    use super::*;
    use crate::components::celestial::{BodyType, CelestialBody};

    fn star_planet_pair() -> Vec<GhostBody> {
        // Massive fixed-ish star + planet on circular velocity (T22-B.5).
        let g: f32 = 5000.0;
        let m_star: f32 = 500_000.0;
        let r: f32 = 300.0;
        let v_circ = (g * m_star / r).sqrt();
        vec![
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: Vec2::ZERO,
                vel: Vec2::ZERO,
                mass: m_star,
                radius: 40.0,
                color: Color::WHITE,
                alive: true,
            },
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: Vec2::new(r, 0.0),
                vel: Vec2::new(0.0, v_circ),
                mass: 10.0,
                radius: 8.0,
                color: Color::WHITE,
                alive: true,
            },
        ]
    }

    #[test]
    fn ghost_tick_is_deterministic_over_k_ticks() {
        let g = 5000.0;
        let dt = 1.0 / 64.0;
        let mut chunked = star_planet_pair();
        let mut bulk = star_planet_pair();
        // Same K=600 ticks: one-by-one vs batches of 10 → bitwise identical.
        for _ in 0..600 {
            ghost_step_tick(&mut chunked, g, dt);
        }
        for _ in 0..60 {
            for _ in 0..10 {
                ghost_step_tick(&mut bulk, g, dt);
            }
        }
        for (a, b) in chunked.iter().zip(bulk.iter()) {
            assert_eq!(a.pos, b.pos, "positions must coincide exactly");
            assert_eq!(a.vel, b.vel, "velocities must coincide exactly");
        }
    }

    #[test]
    fn ghost_total_mass_matches_avian_computed_mass() {
        // Contract test (bug v0.14.100): `ghost_total_mass(mass, radius)` MUST
        // equal the `ComputedMass` Avian derives for a `Collider::circle`
        // body with explicit `Mass(mass)` and default density. If Avian ever
        // changes its density default or circle formula, this test — not a
        // silent forecast drift — will tell us.
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, PhysicsPlugins::default()))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 64.0,
            )));
        app.finish();
        for (mass, radius) in [(8.0, 12.0), (10_000.0, 30.0), (100.0, 8.0)] {
            let e = app
                .world_mut()
                .spawn((RigidBody::Dynamic, Collider::circle(radius), Mass(mass)))
                .id();
            app.world_mut().run_schedule(FixedPostUpdate);
            app.world_mut().run_schedule(FixedPostUpdate);
            let computed = app
                .world()
                .entity(e)
                .get::<ComputedMass>()
                .map(|c| c.value());
            assert_eq!(
                computed,
                Some(ghost_total_mass(mass, radius)),
                "ghost_total_mass({mass}, {radius}) must equal Avian ComputedMass"
            );
        }
    }

    #[test]
    fn ghost_tick_pulls_planet_and_conserves_momentum() {
        let g = 5000.0;
        let dt = 1.0 / 64.0;
        let mut ghosts = star_planet_pair();
        let p0 = ghosts
            .iter()
            .map(|b| b.mass * b.vel)
            .fold(Vec2::ZERO, |a, v| a + v);
        ghost_step_tick(&mut ghosts, g, dt);
        // Planet falls toward the star: x-velocity goes negative.
        assert!(
            ghosts[1].vel.x < 0.0,
            "planet must be pulled toward star, got {:?}",
            ghosts[1].vel
        );
        // Star recoils the other way (action/reaction).
        assert!(ghosts[0].vel.x > 0.0);
        // Total momentum conserved to float precision (frozen-force Euler
        // with exact action/reaction pairs).
        let p1 = ghosts
            .iter()
            .map(|b| b.mass * b.vel)
            .fold(Vec2::ZERO, |a, v| a + v);
        assert!(
            (p1 - p0).length() < 1.0,
            "momentum drift too large: {p0:?} -> {p1:?}"
        );
    }

    #[test]
    fn ghost_tick_skips_close_encounter() {
        // dist_sq < 1.0 → no force: bodies drift ballistically.
        let mut ghosts = vec![
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: Vec2::ZERO,
                vel: Vec2::new(10.0, 0.0),
                mass: 100.0,
                radius: 5.0,
                color: Color::WHITE,
                alive: true,
            },
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: Vec2::new(0.5, 0.0),
                vel: Vec2::ZERO,
                mass: 100.0,
                radius: 5.0,
                color: Color::WHITE,
                alive: true,
            },
        ];
        ghost_step_tick(&mut ghosts, 5000.0, 1.0 / 64.0);
        assert_eq!(ghosts[0].vel, Vec2::new(10.0, 0.0));
        assert_eq!(ghosts[1].vel, Vec2::ZERO);
    }

    fn ghost_test_app() -> App {
        let mut app = App::new();
        app.init_resource::<SimulationState>()
            .init_resource::<TrajectoryConfig>()
            .init_resource::<TrajectoryTickCounter>()
            .init_resource::<GhostPrediction>()
            .init_resource::<GravitationalConstant>()
            .insert_resource(Time::<Fixed>::default())
            .insert_resource(Time::<Physics>::default())
            .add_systems(
                Update,
                (
                    ghost_dirty_triggers,
                    ghost_snapshot_system,
                    ghost_compute_system,
                )
                    .chain(),
            );
        // Paused sim: the only state where snapshot/compute run.
        app.world_mut().resource_mut::<SimulationState>().paused = true;
        app
    }

    fn spawn_body(app: &mut App, x: f32, mass: f32) -> Entity {
        app.world_mut()
            .spawn((
                CelestialBody {
                    name: "B".into(),
                    body_type: BodyType::Planet,
                    mass,
                    radius: 8.0,
                    color: [0.4, 0.6, 1.0],
                    luminous: false,
                },
                Transform::from_xyz(x, 0.0, 0.0),
                GlobalTransform::from_xyz(x, 0.0, 0.0),
                LinearVelocity(Vec2::ZERO),
            ))
            .id()
    }

    #[test]
    fn ghost_snapshot_holds_all_bodies_and_grows_trails() {
        let mut app = ghost_test_app();
        let e1 = spawn_body(&mut app, 0.0, 500_000.0);
        let e2 = spawn_body(&mut app, 300.0, 10.0);
        // Circular orbit velocity so the pair never collides during the
        // horizon (T22-C: a head-on infall would merge and freeze one trail).
        let v_circ = (5000.0_f32 * 500_000.0 / 300.0).sqrt();
        app.world_mut()
            .entity_mut(e2)
            .get_mut::<LinearVelocity>()
            .unwrap()
            .0 = Vec2::new(0.0, v_circ);
        // Short horizon so one frame finishes the forecast.
        app.world_mut()
            .resource_mut::<TrajectoryConfig>()
            .horizon_seconds = 10.0;

        app.update(); // triggers mark dirty (added) — snapshot happens next frame
        app.update(); // snapshot + first chunk
                      // Run until the horizon (640 ticks) is covered.
        for _ in 0..10 {
            app.update();
        }

        let pred = app.world().resource::<GhostPrediction>();
        assert_eq!(pred.bodies.len(), 2, "snapshot must hold ALL bodies");
        assert!(pred.bodies.iter().any(|b| b.entity == e1));
        assert!(pred.bodies.iter().any(|b| b.entity == e2));
        assert_eq!(pred.trails.len(), 2, "one trail Vec per ghost");
        assert_eq!(pred.computed_ticks, pred.horizon_ticks);
        assert_eq!(pred.horizon_ticks, 640);
        for trail in pred.trails.iter() {
            assert_eq!(trail.len(), 640, "1 point per ghost per tick");
        }
        assert!(!pred.dirty);
    }

    #[test]
    fn ghost_mark_dirty_restarts_from_zero() {
        let mut app = ghost_test_app();
        let _ = spawn_body(&mut app, 0.0, 500_000.0);
        let _ = spawn_body(&mut app, 300.0, 10.0);
        app.world_mut()
            .resource_mut::<TrajectoryConfig>()
            .horizon_seconds = 10.0;
        for _ in 0..6 {
            app.update();
        }
        {
            let pred = app.world().resource::<GhostPrediction>();
            assert!(pred.computed_ticks > 0);
        }
        // Manual invalidation restarts the forecast from zero.
        app.world_mut()
            .resource_mut::<GhostPrediction>()
            .mark_dirty();
        app.update();
        app.update();
        let pred = app.world().resource::<GhostPrediction>();
        assert!(!pred.dirty, "compute must re-anchor after dirty");
        assert!(
            pred.computed_ticks > 0 && pred.computed_ticks <= pred.horizon_ticks,
            "forecast regrows progressively from zero"
        );
        assert_eq!(pred.trails.len(), 2);
    }

    // ---------- T22-C: collisions + inelastic merge ----------

    fn head_on_pair() -> Vec<GhostBody> {
        // Equal masses on a frontal collision course (symmetric about origin).
        vec![
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: Vec2::new(-50.0, 0.0),
                vel: Vec2::new(20.0, 0.0),
                mass: 100.0,
                radius: 5.0,
                color: Color::srgb(1.0, 0.0, 0.0),
                alive: true,
            },
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: Vec2::new(50.0, 0.0),
                vel: Vec2::new(-20.0, 0.0),
                mass: 100.0,
                radius: 5.0,
                color: Color::srgb(0.0, 0.0, 1.0),
                alive: true,
            },
        ]
    }

    fn run_ghost_ticks(pred: &mut GhostPrediction, ticks: usize, g: f32, dt: f32) {
        for _ in 0..ticks {
            ghost_step_tick(&mut pred.bodies, g, dt);
            ghost_check_collisions(pred);
            for (body, trail) in pred.bodies.iter().zip(pred.trails.iter_mut()) {
                if body.alive {
                    trail.push_back(body.pos);
                }
            }
        }
        pred.computed_ticks += ticks;
    }

    #[test]
    fn ghost_frontal_collision_marks_merges_and_conserves_momentum() {
        let mut pred = GhostPrediction {
            bodies: head_on_pair(),
            trails: vec![VecDeque::new(), VecDeque::new()],
            collision_markers: Vec::new(),
            dirty: false,
            computed_ticks: 0,
            horizon_ticks: 600,
            anchor_tick: 0,
        };
        let p0 = ghost_alive_momentum(&pred.bodies);
        run_ghost_ticks(&mut pred, 600, 5000.0, 1.0 / 64.0);

        // Exactly one marker for the pair (first overlap only).
        assert_eq!(pred.collision_markers.len(), 1);
        // Symmetric pair → barycenter at the origin.
        assert!(
            pred.collision_markers[0].0.length() < 1.0,
            "marker must sit at the barycenter, got {:?}",
            pred.collision_markers[0].0
        );
        // Marker color = mean of red and blue = purple.
        let mixed = pred.collision_markers[0].1.to_srgba();
        assert!((mixed.red - 0.5).abs() < 1e-3);
        assert!(mixed.green.abs() < 1e-3);
        assert!((mixed.blue - 0.5).abs() < 1e-3);

        // Perfectly-inelastic merge: one survivor, summed mass, area-kept radius.
        let alive: Vec<_> = pred.bodies.iter().filter(|b| b.alive).collect();
        assert_eq!(alive.len(), 1, "two ghosts must become one");
        assert!((alive[0].mass - 200.0).abs() < 1e-3);
        assert!((alive[0].radius - (50.0f32).sqrt()).abs() < 1e-3);

        // Momentum conserved through integration + merge (tolerance 1e-3).
        let p1 = ghost_alive_momentum(&pred.bodies);
        assert!(
            (p1 - p0).length() < 1e-3,
            "momentum drift too large: {p0:?} -> {p1:?}"
        );

        // One trail active to the end; the absorbed one truncated at the merge.
        let survivor_idx = pred.bodies.iter().position(|b| b.alive).unwrap();
        let dead_idx = 1 - survivor_idx;
        assert_eq!(pred.trails[survivor_idx].len(), 600);
        assert!(
            pred.trails[dead_idx].len() < pred.trails[survivor_idx].len(),
            "absorbed trail must stay truncated"
        );
    }

    #[test]
    fn ghost_no_collision_no_markers() {
        // Stable circular pair: forecast runs clean, nothing merges.
        let mut pred = GhostPrediction {
            bodies: star_planet_pair(),
            trails: vec![VecDeque::new(), VecDeque::new()],
            collision_markers: Vec::new(),
            dirty: false,
            computed_ticks: 0,
            horizon_ticks: 600,
            anchor_tick: 0,
        };
        run_ghost_ticks(&mut pred, 600, 5000.0, 1.0 / 64.0);
        assert!(pred.collision_markers.is_empty());
        assert!(pred.bodies.iter().all(|b| b.alive));
        for trail in pred.trails.iter() {
            assert_eq!(trail.len(), 600);
        }
    }

    #[test]
    fn ghost_sequential_merges_absorb_n_bodies() {
        // A and B overlap now; the AB survivor immediately overlaps C too.
        // Survivor color must follow the more massive body at each merge.
        let red = Color::srgb(1.0, 0.0, 0.0);
        let blue = Color::srgb(0.0, 0.0, 1.0);
        let green = Color::srgb(0.0, 1.0, 0.0);
        let mut pred = GhostPrediction {
            bodies: vec![
                GhostBody {
                    entity: Entity::PLACEHOLDER,
                    pos: Vec2::ZERO,
                    vel: Vec2::ZERO,
                    mass: 50.0,
                    radius: 5.0,
                    color: red,
                    alive: true,
                },
                GhostBody {
                    entity: Entity::PLACEHOLDER,
                    pos: Vec2::new(8.0, 0.0),
                    vel: Vec2::ZERO,
                    mass: 200.0,
                    radius: 5.0,
                    color: blue,
                    alive: true,
                },
                GhostBody {
                    entity: Entity::PLACEHOLDER,
                    pos: Vec2::new(14.0, 0.0),
                    vel: Vec2::ZERO,
                    mass: 100.0,
                    radius: 5.0,
                    color: green,
                    alive: true,
                },
            ],
            trails: vec![VecDeque::new(), VecDeque::new(), VecDeque::new()],
            collision_markers: Vec::new(),
            dirty: false,
            computed_ticks: 0,
            horizon_ticks: 600,
            anchor_tick: 0,
        };
        // One check pass merges A+B (B heavier, survives at index 1) and
        // then AB+C (AB heavier, still index 1) — N sequential collisions.
        ghost_check_collisions(&mut pred);
        assert_eq!(pred.collision_markers.len(), 2);
        let alive: Vec<_> = pred.bodies.iter().filter(|b| b.alive).collect();
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].mass, 350.0);
        // Survivor kept the heaviest color at every merge (blue beats red,
        // then 250-mass blue beats 100-mass green).
        assert_eq!(alive[0].color.to_srgba(), blue.to_srgba());
        // Both absorbed trails closed with exactly the merge point.
        assert_eq!(pred.trails[0].len(), 1);
        assert_eq!(pred.trails[2].len(), 1);

        // The forecast CONTINUES after the collisions (never stops).
        run_ghost_ticks(&mut pred, 100, 5000.0, 1.0 / 64.0);
        assert_eq!(pred.collision_markers.len(), 2, "no new pairs to merge");
        assert_eq!(pred.trails[1].len(), 100);
        assert_eq!(pred.trails[0].len(), 1, "absorbed trail stays frozen");
        assert_eq!(pred.trails[2].len(), 1, "absorbed trail stays frozen");
    }

    // ---------- T22-E: sliding window + decimation + horizon dirty ----------

    fn drifting_body(x: f32) -> GhostBody {
        // g = 0 in the slide tests: pure ballistic drift, exact positions.
        GhostBody {
            entity: Entity::PLACEHOLDER,
            pos: Vec2::new(x, 0.0),
            vel: Vec2::new(64.0, 0.0),
            mass: 100.0,
            radius: 5.0,
            color: Color::WHITE,
            alive: true,
        }
    }

    #[test]
    fn ghost_slide_tick_keeps_constant_length() {
        // dt = 1/64 s, vel = 64 u/s -> exactly +1.0 x per slide tick.
        let dt = 1.0 / 64.0;
        let mut pred = GhostPrediction {
            bodies: vec![drifting_body(4.0)],
            trails: vec![VecDeque::from([
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(3.0, 0.0),
                Vec2::new(4.0, 0.0),
            ])],
            collision_markers: Vec::new(),
            dirty: false,
            computed_ticks: 5,
            horizon_ticks: 5,
            anchor_tick: 0,
        };
        ghost_slide_tick(&mut pred, 0.0, dt);
        // Length constant: oldest popped, newest integrated from the frontier.
        assert_eq!(pred.trails[0].len(), 5);
        assert_eq!(pred.trails[0][0], Vec2::new(1.0, 0.0));
        // Integrated points carry f32 substep rounding (6 x dt/6): approx.
        assert!((pred.trails[0][4].x - 5.0).abs() < 1e-4);
        assert_eq!(pred.computed_ticks, 5, "sliding never grows the counter");
        // Window content equals a fresh full-horizon integration from the
        // new anchor: slide 4 more ticks, the trail must be x = 5..=9.
        for _ in 0..4 {
            ghost_slide_tick(&mut pred, 0.0, dt);
        }
        assert_eq!(pred.trails[0].len(), 5);
        assert!((pred.trails[0][0].x - 5.0).abs() < 1e-4);
        assert!((pred.trails[0][4].x - 9.0).abs() < 1e-4);
    }

    #[test]
    fn ghost_slide_tick_grows_partial_forecast_without_pop() {
        let dt = 1.0 / 64.0;
        let mut pred = GhostPrediction {
            bodies: vec![drifting_body(2.0)],
            trails: vec![VecDeque::from([
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(2.0, 0.0),
            ])],
            collision_markers: Vec::new(),
            dirty: false,
            computed_ticks: 3,
            horizon_ticks: 5,
            anchor_tick: 0,
        };
        ghost_slide_tick(&mut pred, 0.0, dt);
        // Growing: no pop, the head is preserved and the counter advances.
        assert_eq!(pred.trails[0].len(), 4);
        assert_eq!(pred.trails[0][0], Vec2::new(0.0, 0.0));
        assert!((pred.trails[0][3].x - 3.0).abs() < 1e-4);
        assert_eq!(pred.computed_ticks, 4);
    }

    #[test]
    fn ghost_slide_tick_leaves_dead_trails_frozen() {
        let dt = 1.0 / 64.0;
        let mut dead = drifting_body(1000.0);
        dead.alive = false;
        let frozen: VecDeque<Vec2> = VecDeque::from([Vec2::new(7.0, 0.0), Vec2::new(8.0, 0.0)]);
        let mut pred = GhostPrediction {
            bodies: vec![drifting_body(4.0), dead],
            trails: vec![
                VecDeque::from([
                    Vec2::new(0.0, 0.0),
                    Vec2::new(1.0, 0.0),
                    Vec2::new(2.0, 0.0),
                    Vec2::new(3.0, 0.0),
                    Vec2::new(4.0, 0.0),
                ]),
                frozen.clone(),
            ],
            collision_markers: Vec::new(),
            dirty: false,
            computed_ticks: 5,
            horizon_ticks: 5,
            anchor_tick: 0,
        };
        ghost_slide_tick(&mut pred, 0.0, dt);
        assert_eq!(pred.trails[1], frozen, "dead trail must stay frozen");
        assert!(pred.collision_markers.is_empty());
        assert_eq!(pred.trails[0].len(), 5);
    }

    #[test]
    fn ghost_decimation_stride_bounds_far_future_density() {
        assert_eq!(ghost_decimation_stride(0), 1);
        assert_eq!(ghost_decimation_stride(640), 1);
        assert_eq!(ghost_decimation_stride(1500), 1);
        assert_eq!(ghost_decimation_stride(3000), 2);
        // Default horizon: 300 s * 64 = 19200 ticks -> stride 12.
        assert_eq!(ghost_decimation_stride(19200), 12);
        // Max horizon: 3600 s * 64 = 230400 ticks -> stride 153.
        assert_eq!(ghost_decimation_stride(230400), 153);
    }

    fn dirty_trigger_test_app() -> App {
        let mut app = App::new();
        app.init_resource::<SimulationState>()
            .init_resource::<TrajectoryConfig>()
            .init_resource::<TrajectoryTickCounter>()
            .init_resource::<GravitationalConstant>()
            .init_resource::<GhostPrediction>()
            .add_systems(Update, ghost_dirty_triggers);
        app
    }

    #[test]
    fn ghost_horizon_change_marks_dirty() {
        let mut app = dirty_trigger_test_app();
        let _ = app
            .world_mut()
            .spawn((CelestialBody {
                name: "B".into(),
                body_type: BodyType::Planet,
                mass: 100.0,
                radius: 8.0,
                color: [0.4, 0.6, 1.0],
                luminous: false,
            },))
            .id();
        // Settle: locals latch, Added/Changded triggers drain.
        app.world_mut()
            .resource_mut::<TrajectoryConfig>()
            .horizon_seconds = 10.0;
        app.update();
        app.update();
        // Clear the settled dirty flag to isolate the horizon change.
        app.world_mut().resource_mut::<GhostPrediction>().dirty = false;
        app.update();
        assert!(
            !app.world().resource::<GhostPrediction>().dirty,
            "steady state must not re-dirty"
        );
        // The T22-D settings write (horizon 10 s -> 20 s) restarts the ghost.
        app.world_mut()
            .resource_mut::<TrajectoryConfig>()
            .horizon_seconds = 20.0;
        app.update();
        assert!(
            app.world().resource::<GhostPrediction>().dirty,
            "horizon change must mark the forecast dirty"
        );
    }

    #[test]
    fn ghost_physics_writeback_in_run_does_not_dirty() {
        // Regression test (bug v0.14.90): Avian rewrites Transform +
        // LinearVelocity every physics tick. Those Changed flags must NOT
        // invalidate the forecast during Run — otherwise the sliding window
        // re-anchors (empty trails) every frame and the future curves vanish
        // on Play. Only real user edits (CelestialBody) dirty in Run.
        let mut app = dirty_trigger_test_app();
        let e = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "B".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius: 8.0,
                    color: [0.4, 0.6, 1.0],
                    luminous: false,
                },
                Transform::default(),
                LinearVelocity(Vec2::ZERO),
            ))
            .id();
        // Settle: Added/Changed triggers drain, locals latch. Default sim
        // state is running (paused = false) — the Run case under test.
        app.update();
        app.update();
        app.world_mut().resource_mut::<GhostPrediction>().dirty = false;
        // Simulate one Avian writeback tick touching physics components.
        app.world_mut()
            .entity_mut(e)
            .insert(Transform::from_xyz(1.0, 0.0, 0.0));
        app.world_mut()
            .entity_mut(e)
            .insert(LinearVelocity(Vec2::new(1.0, 0.0)));
        app.update();
        assert!(
            !app.world().resource::<GhostPrediction>().dirty,
            "physics writeback in Run must NOT invalidate the forecast"
        );
        // A genuine user edit (body property) in Run MUST still invalidate.
        app.world_mut().get_mut::<CelestialBody>(e).unwrap().mass = 999.0;
        app.update();
        assert!(
            app.world().resource::<GhostPrediction>().dirty,
            "user edit to CelestialBody must mark the forecast dirty"
        );
    }

    #[test]
    fn ghost_non_body_transform_does_not_dirty_in_pause() {
        // Regression test (bug v0.14.93): cameras, parallax layers and the
        // minimap camera rewrite their Transform every frame. Those entities
        // are NOT bodies, so their Changed<Transform> must NOT invalidate the
        // forecast — otherwise the ghost restarted every frame while paused
        // (snapshot + 1 chunk: computed froze at 256 ticks at any horizon).
        let mut app = dirty_trigger_test_app();
        app.world_mut().resource_mut::<SimulationState>().paused = true;
        let body = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "B".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius: 8.0,
                    color: [0.4, 0.6, 1.0],
                    luminous: false,
                },
                Transform::default(),
            ))
            .id();
        // Camera-like entity: Transform but NO CelestialBody.
        let cam = app.world_mut().spawn(Transform::default()).id();
        app.update();
        app.update();
        app.world_mut().resource_mut::<GhostPrediction>().dirty = false;
        // Per-frame rewrite of a non-body Transform (camera/parallax pattern).
        app.world_mut()
            .entity_mut(cam)
            .insert(Transform::from_xyz(5.0, 5.0, 0.0));
        app.update();
        assert!(
            !app.world().resource::<GhostPrediction>().dirty,
            "non-body Transform write must NOT invalidate the forecast"
        );
        // ...but a real body drag in pause MUST still invalidate.
        app.world_mut()
            .entity_mut(body)
            .insert(Transform::from_xyz(1.0, 0.0, 0.0));
        app.update();
        assert!(
            app.world().resource::<GhostPrediction>().dirty,
            "body Transform change in pause must mark the forecast dirty"
        );
    }

    #[test]
    fn ghost_grows_past_first_chunk_with_real_writers_running() {
        // Integration regression test (bug v0.14.93): the REAL per-frame
        // writers run (here `sync_sprite_z`, standing in for the app's
        // cameras/parallax/minimap too). A paused forecast must GROW past
        // the first 256-tick chunk. Pre-fix steady state: every frame
        // restarted (snapshot + exactly 1 chunk), so computed froze at 256
        // ticks at ANY horizon and the horizon setting had no visible effect.
        use crate::systems::firefly_bridge::sync_sprite_z;
        let mut app = App::new();
        app.init_resource::<SimulationState>()
            .init_resource::<TrajectoryConfig>()
            .init_resource::<TrajectoryTickCounter>()
            .init_resource::<GravitationalConstant>()
            .init_resource::<GhostPrediction>()
            .insert_resource(Time::<Fixed>::default())
            .insert_resource(Time::<Physics>::default())
            .add_systems(
                Update,
                (
                    sync_sprite_z,
                    ghost_dirty_triggers,
                    ghost_snapshot_system,
                    ghost_compute_system,
                )
                    .chain(),
            );
        app.world_mut().resource_mut::<SimulationState>().paused = true;
        // Star + ONE planet on a circular orbit (v = sqrt(G*M/r),
        // G = 5000 = default): zero-velocity planets would fall straight
        // into the star and merge (correct behavior, covered by the T22-C
        // tests), and two planets perturb each other into real close
        // encounters — so a single planet keeps this growth test
        // collision-free by construction.
        let bodies = [
            ("Star", Vec2::ZERO, Vec2::ZERO, 5000.0, 30.0, true),
            (
                "P1",
                Vec2::new(300.0, 0.0),
                Vec2::new(0.0, 288.7),
                10.0,
                8.0,
                false,
            ),
        ];
        for (name, pos, vel, mass, radius, luminous) in bodies {
            app.world_mut().spawn((
                CelestialBody {
                    name: name.into(),
                    body_type: BodyType::Planet,
                    mass,
                    radius,
                    color: [0.4, 0.6, 1.0],
                    luminous,
                },
                Transform::from_xyz(pos.x, pos.y, 0.0),
                GlobalTransform::from_xyz(pos.x, pos.y, 0.0),
                LinearVelocity(vel),
                Sprite {
                    color: Color::srgba(0.5, 0.5, 0.5, 1.0),
                    ..default()
                },
            ));
        }
        for _ in 0..6 {
            app.update();
        }
        let pred = app.world().resource::<GhostPrediction>();
        let lens: Vec<usize> = pred.trails.iter().map(|t| t.len()).collect();
        assert_eq!(
            pred.collision_markers.len(),
            0,
            "no collisions expected on circular orbits (markers={:?})",
            pred.collision_markers
        );
        assert_eq!(
            pred.computed_ticks,
            6 * GHOST_TICKS_PER_FRAME,
            "paused forecast must grow 256 ticks/frame (got {})",
            pred.computed_ticks
        );
        assert!(
            pred.trails
                .iter()
                .all(|t| t.len() == 6 * GHOST_TICKS_PER_FRAME),
            "all trails must grow with computed ticks (lens={lens:?})"
        );
    }

    /// End-to-end fidelity probe (bug report v0.14.94: il ghost "avanza più
    /// in fretta del pianeta" + "errore a lungo termine"): catena ghost pura
    /// contro il VERO solver Avian + la VERA gravità per-substep, headless.
    ///
    /// Misura la divergenza su 200 tick (~5 orbite): se il ghost fosse un
    /// "ghost run deterministico della simulazione" (ADR 0001 Dec. 4), i due
    /// resterebbero sovrapposti. Qualunque scostamento sistematico qui è la
    /// causa radice del distacco testa-trail in Run.
    #[test]
    fn ghost_divergence_vs_real_avian_over_200_ticks() {
        use avian2d::dynamics::integrator::{integrate_velocities, IntegrationSystems};
        use avian2d::dynamics::solver::schedule::SubstepSchedule;
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let g = 5000.0_f32;
        let dt = 1.0 / 64.0_f32;
        let m_star = 500_000.0_f32;
        let r = 300.0_f32;
        let v_circ = (g * m_star / r).sqrt();

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(GravitationalConstant(g))
        .add_systems(
            SubstepSchedule,
            crate::systems::gravity::substep_gravity_system
                .in_set(IntegrationSystems::Velocity)
                .after(ForceSystems::ApplyLocalAcceleration)
                .before(integrate_velocities),
        )
        // 1 update = esattamente 1 fixed tick (niente resto accumulato).
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 64.0,
        )));
        app.finish();

        let spawn =
            |world: &mut World, name: &str, pos: Vec2, vel: Vec2, mass: f32, radius: f32| {
                world
                    .spawn((
                        CelestialBody {
                            name: name.into(),
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
                        ConstantForce(Vec2::ZERO),
                    ))
                    .id()
            };
        let mut world = app.world_mut();
        let star = spawn(&mut world, "Star", Vec2::ZERO, Vec2::ZERO, m_star, 40.0);
        let planet = spawn(
            &mut world,
            "P",
            Vec2::new(r, 0.0),
            Vec2::new(0.0, v_circ),
            10.0,
            8.0,
        );
        drop(world);

        app.update(); // prime: avanza l'orologio, nessun tick garantito
        let t0 = app.world().resource::<Time<Physics>>().elapsed();

        // Snapshot iniziale dallo stesso stato che vedrebbe il ghost.
        let (p0, v0) = {
            let w = app.world();
            (
                w.entity(planet).get::<Position>().unwrap().0,
                w.entity(planet).get::<LinearVelocity>().unwrap().0,
            )
        };
        let (s0, sv0) = {
            let w = app.world();
            (
                w.entity(star).get::<Position>().unwrap().0,
                w.entity(star).get::<LinearVelocity>().unwrap().0,
            )
        };
        let mut ghosts = vec![
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: s0,
                vel: sv0,
                mass: m_star,
                radius: 40.0,
                color: Color::WHITE,
                alive: true,
            },
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: p0,
                vel: v0,
                mass: 10.0,
                radius: 8.0,
                color: Color::WHITE,
                alive: true,
            },
        ];

        // Ordine entità nel ghost: [star, planet] — l'indice 1 è il pianeta.
        let k = 200_usize;
        let mut max_dev = 0.0_f32;
        for _ in 0..k {
            ghost_step_tick(&mut ghosts, g, dt);
            app.update();
            let real = app.world().entity(planet).get::<Position>().unwrap().0;
            let dev = (ghosts[1].pos - real).length();
            max_dev = max_dev.max(dev);
        }
        let real_end = app.world().entity(planet).get::<Position>().unwrap().0;
        let final_dev = (ghosts[1].pos - real_end).length();
        let elapsed = app.world().resource::<Time<Physics>>().elapsed() - t0;
        eprintln!(
            "ghost-vs-real: ticks target={k}, physics elapsed={elapsed:?}, \
             max_dev={max_dev:.4}, final_dev={final_dev:.4}"
        );
        // Sanity: devono essere girati davvero K tick fisici.
        assert!(
            (elapsed.as_secs_f32() - k as f32 * dt).abs() < 1e-3,
            "devono girare esattamente {k} tick (elapsed={elapsed:?})"
        );
        assert!(
            final_dev < 1.0,
            "ghost fedele? scostamento finale {final_dev:.4} (max {max_dev:.4}) su orbita r={r}"
        );
    }

    /// Sonda diagnostica (report utente v0.14.99: testa-trail ghost si stacca
    /// dal pianeta dopo un paio di orbite, sim 1.0x, 2 corpi, markers 0).
    /// Stessa catena del test sopra ma con la configurazione REALE del preset
    /// (Sole 10000 + Alpha 8) e sull'intero orizzonte utente (1920 tick =
    /// 30 s): misura la divergenza a intervalli per vedere SE cresce e COME
    /// (deriva lineare di fase vs salto improvviso). Solo sanity assert sul
    /// conteggio tick — i numeri escono su stderr per la diagnosi.
    #[test]
    fn ghost_divergence_user_config_over_full_horizon() {
        use avian2d::dynamics::integrator::{integrate_velocities, IntegrationSystems};
        use avian2d::dynamics::solver::schedule::SubstepSchedule;
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let g = 5000.0_f32;
        let dt = 1.0 / 64.0_f32;

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(GravitationalConstant(g))
        .add_systems(
            SubstepSchedule,
            crate::systems::gravity::substep_gravity_system
                .in_set(IntegrationSystems::Velocity)
                .after(ForceSystems::ApplyLocalAcceleration)
                .before(integrate_velocities),
        )
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 64.0,
        )));
        app.finish();

        let spawn =
            |world: &mut World, name: &str, pos: Vec2, vel: Vec2, mass: f32, radius: f32| {
                world
                    .spawn((
                        CelestialBody {
                            name: name.into(),
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
                        ConstantForce(Vec2::ZERO),
                    ))
                    .id()
            };
        let mut world = app.world_mut();
        let star = spawn(
            &mut world,
            "Sun",
            Vec2::new(63.372482, 57.54164),
            Vec2::ZERO,
            10_000.0,
            30.0,
        );
        let planet = spawn(
            &mut world,
            "Planet Alpha",
            Vec2::new(215.95473, -1000.0),
            Vec2::new(-250.0, -15.0),
            8.0,
            12.0,
        );
        drop(world);

        app.update();
        let t0 = app.world().resource::<Time<Physics>>().elapsed();
        let (p0, v0) = {
            let w = app.world();
            (
                w.entity(planet).get::<Position>().unwrap().0,
                w.entity(planet).get::<LinearVelocity>().unwrap().0,
            )
        };
        let (s0, sv0) = {
            let w = app.world();
            (
                w.entity(star).get::<Position>().unwrap().0,
                w.entity(star).get::<LinearVelocity>().unwrap().0,
            )
        };
        let mut ghosts = vec![
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: s0,
                vel: sv0,
                mass: 10_000.0,
                radius: 30.0,
                color: Color::WHITE,
                alive: true,
            },
            GhostBody {
                entity: Entity::PLACEHOLDER,
                pos: p0,
                vel: v0,
                mass: 8.0,
                radius: 12.0,
                color: Color::WHITE,
                alive: true,
            },
        ];

        let k = 1920_usize;
        let mut max_dev = 0.0_f32;
        let mut min_dist = f32::MAX;
        for i in 1..=k {
            ghost_step_tick(&mut ghosts, g, dt);
            app.update();
            let real = app.world().entity(planet).get::<Position>().unwrap().0;
            let dev = (ghosts[1].pos - real).length();
            max_dev = max_dev.max(dev);
            let d = (ghosts[1].pos - ghosts[0].pos).length();
            min_dist = min_dist.min(d);
            if i % 240 == 0 {
                eprintln!("tick {i}: dev={dev:.4} max_dev={max_dev:.4}");
            }
        }
        let real_end = app.world().entity(planet).get::<Position>().unwrap().0;
        let final_dev = (ghosts[1].pos - real_end).length();
        let elapsed = app.world().resource::<Time<Physics>>().elapsed() - t0;
        eprintln!(
            "user-config: ticks={k}, physics elapsed={elapsed:?}, \
             final_dev={final_dev:.4} max_dev={max_dev:.4} min_star_dist={min_dist:.2}"
        );
        assert!(
            (elapsed.as_secs_f32() - k as f32 * dt).abs() < 1e-3,
            "devono girare esattamente {k} tick (elapsed={elapsed:?})"
        );
    }
}
