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
}

impl Default for TrajectoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_length: 500,
            prediction_steps: 200,
            sample_interval: 2,
        }
    }
}

/// Per-tick counter to enforce the sample interval (physics ticks).
#[derive(Resource, Default)]
pub struct TrajectoryTickCounter(pub u64);

/// Resource holding the prediction trail (RK4 positions) for the selected body.
#[derive(Resource, Default)]
pub struct PredictionTrail(pub Vec<Vec2>);
