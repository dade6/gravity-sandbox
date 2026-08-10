use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Component for light-emitting bodies (stars).
///
/// Attached automatically to entities with `CelestialBody { luminous: true }`
/// by the `init_light_sources` system.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
pub struct LightSource {
    /// Base light intensity (brightness multiplier).
    pub intensity: f32,
    /// Controls how intensity falls off with distance:
    /// `received = intensity / (1.0 + dist² * falloff)`
    pub falloff: f32,
}

impl Default for LightSource {
    fn default() -> Self {
        Self {
            intensity: 1.0,
            falloff: 0.0001,
        }
    }
}

/// Component attached to non-luminous bodies with computed lighting data.
/// Updated every frame by the light system.
#[derive(Debug, Clone, Component)]
pub struct LightInfo {
    /// Normalized direction from the body toward its nearest star
    pub direction: Vec2,
    /// Received light intensity (after distance falloff).
    /// 0.0 if too far from any star or no stars in scene.
    pub intensity: f32,
    /// Distance to the nearest star in world units
    pub distance_to_star: f32,
    /// World-space position of the nearest star (fed to the LightMaterial
    /// per-pixel shader so it can compute the light direction itself).
    pub light_pos: Vec2,
    /// RGB colour of the nearest star (from `CelestialBody.color` —
    /// `LightSource` carries no colour).
    pub light_color: Vec3,
    /// Raw intensity of the nearest star (before distance falloff).
    pub star_intensity: f32,
    /// Falloff constant of the nearest star.
    pub falloff: f32,
}

/// Global ambient light resource.
/// Bodies far from any star receive only this much light (default ~12%).
#[derive(Debug, Clone, Resource)]
pub struct AmbientLight {
    pub intensity: f32,
}

impl Default for AmbientLight {
    fn default() -> Self {
        Self { intensity: 0.12 }
    }
}
