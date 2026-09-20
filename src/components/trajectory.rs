use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Component that stores historical positions of a celestial body
/// for rendering trails.
///
/// Uses a ring buffer ([`VecDeque`]) so both push and eviction are O(1)
/// (the old `Vec` implementation memmoved the whole buffer on every
/// `remove(0)`).
#[derive(Component)]
pub struct TrajectoryHistory {
    pub positions: VecDeque<Vec2>,
    pub max_len: usize,
}

impl Default for TrajectoryHistory {
    fn default() -> Self {
        Self {
            positions: VecDeque::with_capacity(500),
            max_len: 500,
        }
    }
}

impl TrajectoryHistory {
    /// Appends a new sample, evicting the oldest one when the buffer
    /// is at capacity. O(1).
    pub fn push_sample(&mut self, pos: Vec2) {
        if self.positions.len() == self.max_len {
            self.positions.pop_front();
        }
        self.positions.push_back(pos);
    }

    /// Applies a new capacity limit, evicting oldest samples if it shrinks.
    pub fn set_max_len(&mut self, max_len: usize) {
        self.max_len = max_len;
        while self.positions.len() > max_len {
            self.positions.pop_front();
        }
    }
}

/// Default prediction horizon in simulation seconds (ADR 0001, Dec. 7).
pub fn default_horizon_seconds() -> f32 {
    300.0
}

/// Global configuration for trajectory rendering.
///
/// Serialised at the level level (`LevelData.trajectory`) since Ticket 21;
/// each field carries `#[serde(default)]` so presets saved BEFORE the field
/// existed load with the historic defaults without errors.
#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
#[serde(default)]
pub struct TrajectoryConfig {
    pub enabled: bool,
    pub history_length: usize,
    pub prediction_steps: usize,
    pub sample_interval: usize,
    #[serde(default = "default_horizon_seconds")]
    pub horizon_seconds: f32,
}

impl Default for TrajectoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_length: 500,
            prediction_steps: 200,
            sample_interval: 2,
            horizon_seconds: default_horizon_seconds(),
        }
    }
}

impl TrajectoryConfig {
    /// Horizon converted to physics ticks (64 ticks/s), clamped 10–3600 s.
    pub fn horizon_ticks(&self) -> usize {
        (self.horizon_seconds.clamp(10.0, 3600.0) * 64.0) as usize
    }
}

/// Per-tick counter to enforce the sample interval (physics ticks).
#[derive(Resource, Default)]
pub struct TrajectoryTickCounter(pub u64);

/// Snapshot of one simulated body in the ghost (future) simulation
/// (ADR 0001, Dec. 1). Plain data — no Avian components (Dec. 2).
#[derive(Debug, Clone)]
pub struct GhostBody {
    pub entity: Entity,
    pub pos: Vec2,
    pub vel: Vec2,
    pub mass: f32,
    pub radius: f32,
    pub color: Color,
    /// False once absorbed by a perfectly-inelastic ghost merge (T22-C,
    /// ADR 0001 Dec. 5-6). Dead ghosts exert no force, are not integrated,
    /// and their trail stays frozen at the collision point.
    pub alive: bool,
}

/// Ghost (future) prediction state: full N-body forecast of ALL bodies.
///
/// Lives outside the physical ECS (Dec. 2); integrated progressively by a
/// system in `Update` (Dec. 1). Total invalidation via `dirty` flag (Dec. 3).
#[derive(Debug, Resource, Default)]
pub struct GhostPrediction {
    pub bodies: Vec<GhostBody>,
    /// One future trail per ghost, oldest -> newest. `VecDeque` so the
    /// Run sliding window (T22-E, ADR 0001 Dec. 8) pops the oldest point
    /// and pushes the newest in O(1) with constant length.
    pub trails: Vec<VecDeque<Vec2>>,
    /// Arc-length already consumed by the sliding window, per ghost.
    /// The mesh UV restarts at 0 every rebuild, so without this the dash
    /// pattern would slide along with the window. Adding the popped
    /// length back keeps dashes anchored in world space: old points keep
    /// the same UV, new points continue the sequence.
    pub trail_arc_offset: Vec<f32>,
    pub collision_markers: Vec<(Vec2, Color)>,
    pub dirty: bool,
    pub computed_ticks: usize,
    pub horizon_ticks: usize,
    pub anchor_tick: u64,
}

impl GhostPrediction {
    /// Total invalidation: restarts the forecast from zero (Dec. 3).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.trails.clear();
        self.trail_arc_offset.clear();
        self.collision_markers.clear();
        self.computed_ticks = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_horizon_is_300s_19200_ticks() {
        let cfg = TrajectoryConfig::default();
        assert!((cfg.horizon_seconds - 300.0).abs() < f32::EPSILON);
        assert_eq!(cfg.horizon_ticks(), 19200);
    }

    #[test]
    fn horizon_ticks_clamps() {
        let low = TrajectoryConfig {
            horizon_seconds: 5.0,
            ..TrajectoryConfig::default()
        };
        assert_eq!(low.horizon_ticks(), 640);
        let high = TrajectoryConfig {
            horizon_seconds: 9999.0,
            ..TrajectoryConfig::default()
        };
        assert_eq!(high.horizon_ticks(), 230400);
    }

    #[test]
    fn legacy_preset_without_horizon_loads_with_default() {
        let json =
            r#"{"enabled":true,"history_length":500,"prediction_steps":200,"sample_interval":2}"#;
        let cfg: TrajectoryConfig = serde_json::from_str(json).expect("legacy preset must load");
        assert!((cfg.horizon_seconds - 300.0).abs() < f32::EPSILON);
    }

    #[test]
    fn mark_dirty_resets() {
        let mut pred = GhostPrediction {
            bodies: vec![],
            trails: vec![VecDeque::from([Vec2::ZERO])],
            trail_arc_offset: vec![0.0],
            collision_markers: vec![(Vec2::ZERO, Color::WHITE)],
            dirty: false,
            computed_ticks: 42,
            horizon_ticks: 19200,
            anchor_tick: 7,
        };
        pred.mark_dirty();
        assert!(pred.dirty);
        assert!(pred.trails.is_empty());
        assert!(pred.collision_markers.is_empty());
        assert_eq!(pred.computed_ticks, 0);
    }
}
