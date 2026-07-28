use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodyType {
    Star,
    Planet,
    Moon,
    Asteroid,
    Spaceship,
}

#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct CelestialBody {
    pub body_type: BodyType,
    pub mass: f32,
    pub radius: f32,
    pub color: [f32; 3],
    pub luminous: bool,
}

impl Default for CelestialBody {
    fn default() -> Self {
        Self {
            body_type: BodyType::Planet,
            mass: 100.0,
            radius: 20.0,
            color: [0.5, 0.5, 0.8],
            luminous: false,
        }
    }
}
