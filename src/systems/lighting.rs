use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::sprite_render::{Material2d, Material2dPlugin};
use bevy_shader::ShaderRef;

// ═══════════════════════════════════════════════════════════════════
//  LightMaterial — custom Material2d for per-pixel 2D lighting
// ═══════════════════════════════════════════════════════════════════

/// Custom 2D material that applies per-pixel lighting from a
/// [`LightSource`](crate::components::lighting::LightSource).
///
/// The fragment shader (`assets/shaders/light_material.wgsl`) receives
/// these uniforms and texture bindings via Bevy's `AsBindGroup` derive.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct LightMaterial {
    /// World-space position of the light source (fed per-instance).
    #[uniform(0)]
    pub light_pos: Vec2,

    /// Light intensity / brightness multiplier.
    #[uniform(1)]
    pub light_intensity: f32,

    /// RGB colour of the emitted light.
    #[uniform(2)]
    pub light_color: Vec3,

    /// Minimum ambient light strength applied to the base colour.
    #[uniform(3)]
    pub ambient_strength: f32,

    /// Optional normal map for per-pixel directional lighting.
    /// When `None` the shader falls back to a front-facing normal,
    /// producing a flat radial gradient.
    #[texture(4)]
    #[sampler(5)]
    pub normal_map: Option<Handle<Image>>,
}

impl Material2d for LightMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/light_material.wgsl".into()
    }
}

// ═══════════════════════════════════════════════════════════════════
//  LightingPlugin — registers the material and its asset pipeline
// ═══════════════════════════════════════════════════════════════════

/// Plugin that registers [`LightMaterial`] with the renderer.
///
/// This is purely setup — per-frame lighting update systems
/// will be added in ticket 09b.
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        // Register the custom material so Bevy knows about its shader
        // and bind-group layout.  The render-app counterpart is handled
        // automatically by `Material2dPlugin`.
        app.add_plugins(Material2dPlugin::<LightMaterial>::default());
    }
}
