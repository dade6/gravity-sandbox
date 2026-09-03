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
///
/// Ticket 19: extended with `color` and `range` so a preset can tint and bound
/// the ambient contribution from the nearest star. The active renderer
/// (bevy_firefly) reads `intensity` + `color` into its `FireflyConfig`.
#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
pub struct AmbientLight {
    /// Ambient brightness multiplier (0 = black).
    pub intensity: f32,
    /// RGB tint of the ambient light (linear, 0..1).
    pub color: [f32; 3],
    /// Optional range gate: if > 0, ambient is only fully applied within
    /// `range` units of the nearest star and decays beyond it. 0 = no limit
    /// (ambient applies everywhere, the historic behaviour).
    pub range: f32,
}

impl Default for AmbientLight {
    fn default() -> Self {
        Self {
            intensity: 0.12,
            color: [1.0, 1.0, 1.0],
            range: 0.0,
        }
    }
}

/// Which falloff curve a star's light uses (serialised in preset.json).
/// Mirrors `bevy_firefly::prelude::Falloff`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LightFalloff {
    /// `1/(1 + dist²·intensity)` style inverse-square falloff.
    InverseSquare,
    /// Linear falloff.
    Linear,
    /// Constant intensity out to `radius` (historic behaviour, gentle edge
    /// fade controlled by `StarLightSettings.fade_width`).
    None,
}

impl Default for LightFalloff {
    fn default() -> Self {
        Self::None
    }
}

/// Per-star light parameters (configurable). Attached to luminous bodies.
/// Serialised per-body under `BodyData.light` in preset.json.
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
#[serde(default)]
pub struct StarLightSettings {
    /// Base intensity of the point light. (historic 1.8)
    pub intensity: f32,
    /// Outer radius of the light in world units. (historic 5000)
    pub radius: f32,
    /// Falloff curve applied before the radius edge.
    pub falloff: LightFalloff,
    /// Width (world units) of the soft transition band at the light edge.
    /// 0 = hard historic cutoff at `radius`. >0 → the shader fades the
    /// contribution smoothly to 0 within `radius..radius+fade_width`.
    pub fade_width: f32,
    /// Core radius as a factor of the star's body radius (historic 1.5×).
    pub core_radius_factor: f32,
    /// Brightness boost of the light core. (historic 3.0)
    pub core_boost: f32,
}

impl Default for StarLightSettings {
    fn default() -> Self {
        Self {
            intensity: 1.8,
            radius: 5000.0,
            falloff: LightFalloff::None,
            fade_width: 500.0,
            core_radius_factor: 1.5,
            core_boost: 3.0,
        }
    }
}

/// Per-star glow (halo) parameters, two rings. Attached to luminous bodies.
/// Serialised per-body under `BodyData.glow` in preset.json.
///
/// Inner ring (historic firefly glow): ~4× the body radius, fairly opaque.
/// Outer ring (new faint halo): ~25× the body radius, very subtle.
///
/// The radial falloff *curve* and the soft *edge* band are GLOBAL at the
/// preset level (`LevelData.glow_curve`) — the glow texture is shared, so the
/// curve cannot differ per star without per-star textures (out of scope).
#[derive(Debug, Clone, Component, Serialize, Deserialize)]
#[serde(default)]
pub struct StarGlow {
    /// Inner glow radius = body radius × this factor.
    pub inner_scale: f32,
    /// Inner glow opacity (0..1). (historic 0.55)
    pub inner_alpha: f32,
    /// Outer glow radius = body radius × this factor. (historic 25)
    pub outer_scale: f32,
    /// Outer glow opacity (0..1). (historic 0.18)
    pub outer_alpha: f32,
    /// Brightness MULTIPLIER of the whole halo (inner+outer), independent of
    /// the light the planets receive (`StarLightSettings.intensity`).
    /// 1.0 = default. This is the "Alone luminoso" knob Davide asked for
    /// (Ticket 20: the two effects must be separately controllable).
    #[serde(default = "default_glow_brightness")]
    pub brightness: f32,
}

fn default_glow_brightness() -> f32 {
    1.0
}

impl Default for StarGlow {
    fn default() -> Self {
        Self {
            inner_scale: 4.0,
            inner_alpha: 0.55,
            outer_scale: 25.0,
            outer_alpha: 0.18,
            brightness: 1.0,
        }
    }
}

/// Global glow-curve resource (loaded from `LevelData.glow_curve` at preset
/// load / Reset). Controls the radial alpha falloff of the shared glow
/// texture:
/// - `falloff_exp` is the exponent of the `(1 - t)^exp` radial falloff.
/// - `soft_edge` (0..~0.1, fraction of the glow radius) is the band over which
///   the alpha ramps to exactly 0, so no hard disc edge is visible on black.
#[derive(Debug, Clone, PartialEq, Resource, Serialize, Deserialize)]
pub struct GlowCurve {
    pub falloff_exp: f32,
    pub soft_edge: f32,
}

impl Default for GlowCurve {
    fn default() -> Self {
        Self {
            falloff_exp: 2.0,
            soft_edge: 0.04,
        }
    }
}

impl GlowCurve {
    /// Radial alpha for a normalised radius `t` in [0, 1) (0 = centre,
    /// approaching 1 = disc edge). Returns 0 at / beyond `1 - soft_edge`, so
    /// the last non-zero texel sits within `soft_edge` of the rim.
    pub fn alpha_at(&self, t: f32) -> f32 {
        let edge = (1.0 - self.soft_edge).max(0.0);
        let x = (t / edge).clamp(0.0, 1.0);
        (1.0 - x).powf(self.falloff_exp)
    }
}

/// Edge-fade multiplier mirroring the vendored firefly shader soft edge
/// (`create_lightmap.wgsl`): full (1.0) at `dist <= radius`, ramping smoothly
/// (smoothstep) to 0.0 at `dist = radius + fade_width`. `fade_width <= 0`
/// keeps the historic hard cutoff (factor always 1.0).
#[inline]
pub fn light_edge_fade(dist: f32, radius: f32, fade_width: f32) -> f32 {
    if fade_width <= 0.0 || dist <= radius {
        return 1.0;
    }
    let t = ((dist - radius) / fade_width).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_edge_fade_legacy_zero_width() {
        // fade_width == 0 -> always 1.0 (historic hard cutoff).
        for d in [0.0f32, 4999.0, 5000.0, 99999.0] {
            assert_eq!(light_edge_fade(d, 5000.0, 0.0), 1.0);
        }
    }

    #[test]
    fn light_edge_fade_values() {
        // Inside radius (incl. exactly at radius): full.
        assert_eq!(light_edge_fade(2500.0, 5000.0, 500.0), 1.0);
        assert!((light_edge_fade(5000.0, 5000.0, 500.0) - 1.0).abs() < 1e-6);
        // At radius + fade_width: exactly 0 (light fully off).
        assert!(light_edge_fade(5500.0, 5000.0, 500.0).abs() < 1e-6);
        // Beyond: 0.
        assert_eq!(light_edge_fade(99999.0, 5000.0, 500.0), 0.0);
    }

    #[test]
    fn light_edge_fade_is_continuous() {
        // No jump > epsilon across [0.5r, r+fade] — the planet must dim
        // gradually, never snap off.
        let radius = 5000.0;
        let fade = 500.0;
        let eps = 0.02;
        let steps = 2000;
        let mut prev = light_edge_fade(radius * 0.5, radius, fade);
        for i in 1..=steps {
            let d = radius * 0.5 + (radius * 0.5 + fade) * (i as f32 / steps as f32);
            let cur = light_edge_fade(d, radius, fade);
            assert!(
                (cur - prev).abs() <= eps,
                "jump > {eps} at d={d}: {prev} -> {cur}"
            );
            prev = cur;
        }
        // Monotonic non-increasing.
        let mut last = 1.0f32;
        for i in 0..=steps {
            let d = radius * 0.5 + (radius * 0.5 + fade) * (i as f32 / steps as f32);
            let v = light_edge_fade(d, radius, fade);
            assert!(v <= last + 1e-6, "non-monotonic at d={d}: {v} > {last}");
            last = v;
        }
    }

    #[test]
    fn glow_curve_soft_edge_bounds_last_nonzero_texel() {
        // With soft_edge=s, alpha is 0 from t >= 1-s onward, so the last
        // non-zero texel is within `s` (fraction of radius) of the rim.
        let curve = GlowCurve {
            falloff_exp: 2.0,
            soft_edge: 0.04,
        };
        let mut last_nonzero = 0.0f32;
        for i in 0..=10_000 {
            let t = i as f32 / 10_000.0;
            let a = curve.alpha_at(t);
            if a > 0.0 {
                last_nonzero = t;
            }
            if t >= 1.0 - curve.soft_edge {
                assert_eq!(a, 0.0, "alpha must be 0 at t={t}");
            }
        }
        assert!(
            last_nonzero <= 1.0 - curve.soft_edge + 1e-3,
            "last non-zero texel {last_nonzero} not within soft_edge of rim"
        );
        // Default curve reproduces the soft_edge property too.
        let d = GlowCurve::default();
        assert!(d.alpha_at(1.0 - d.soft_edge - 0.001) > 0.0);
        assert_eq!(d.alpha_at(1.0 - d.soft_edge), 0.0);
        assert_eq!(d.alpha_at(2.0), 0.0);
    }

    #[test]
    fn serde_roundtrip_star_settings_and_global_defaults() {
        let s = StarLightSettings {
            intensity: 2.5,
            radius: 8000.0,
            falloff: LightFalloff::Linear,
            fade_width: 0.0,
            core_radius_factor: 2.0,
            core_boost: 1.5,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StarLightSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.intensity, 2.5);
        assert_eq!(back.falloff, LightFalloff::Linear);
        assert_eq!(back.fade_width, 0.0);

        let g = StarGlow::default();
        let gj = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<StarGlow>(&gj).unwrap().outer_scale, 25.0);

        // Missing fields fall back to defaults (old preset compatibility).
        let old = r#"{"intensity":1.8}"#;
        let s2: StarLightSettings = serde_json::from_str(old).unwrap();
        assert_eq!(s2.radius, StarLightSettings::default().radius);
        assert_eq!(s2.fade_width, StarLightSettings::default().fade_width);
    }
}