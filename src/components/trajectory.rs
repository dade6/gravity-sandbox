use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Component that stores historical positions of a celestial body
/// for rendering trails.
#[derive(Component)]
pub struct TrajectoryHistory {
    pub positions: Vec<Vec2>,
    pub max_len: usize,
}

impl Default for TrajectoryHistory {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            max_len: 500,
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

/// Per-frame counter to enforce the sample interval.
#[derive(Resource, Default)]
pub struct TrajectoryFrameCounter(pub u64);

/// Resource holding the prediction trail (RK4 positions) for the selected body.
#[derive(Resource, Default)]
pub struct PredictionTrail(pub Vec<Vec2>);
