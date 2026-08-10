use bevy::color::{ColorToComponents, LinearRgba};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy_shader::ShaderRef;

// ═══════════════════════════════════════════════════════════════════
//  LightMaterial — custom Material2d for per-pixel 2D lighting
// ═══════════════════════════════════════════════════════════════════

/// GPU layout of [`LightMaterial`] — must match `LightUniforms` in
/// `assets/shaders/light_material.wgsl` (same field order/types).
#[derive(Clone, Default, ShaderType)]
pub struct LightMaterialUniform {
    /// World-space position of the nearest star (fed per-frame).
    pub light_pos: Vec2,
    /// Raw intensity of the star (before distance falloff).
    pub light_intensity: f32,
    /// RGB colour of the star (from `CelestialBody.color`).
    pub light_color: Vec3,
    /// Minimum ambient light strength applied to the base colour.
    pub ambient_strength: f32,
    /// Base colour of the body in linear RGBA (alpha used for drag feedback).
    pub base_color: Vec4,
    /// World-space centre of the body (polar-UV fallback for the normal map).
    pub body_pos: Vec2,
    /// Radius of the body in world units (polar-UV fallback).
    pub body_radius: f32,
    /// Star falloff constant; shader attenuation is `1/(1 + d²·falloff)`.
    pub falloff: f32,
    /// 1 when the normal map should be sampled, 0 for flat shading.
    pub has_normal_map: u32,
}

/// Custom 2D material that applies per-pixel lighting from a
/// [`LightSource`](crate::components::lighting::LightSource).
///
/// The fragment shader (`assets/shaders/light_material.wgsl`) receives
/// these uniforms and texture bindings via Bevy's `AsBindGroup` derive.
/// The uniforms are refreshed every frame by
/// [`update_light_materials`](crate::systems::light::update_light_materials).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[uniform(0, LightMaterialUniform)]
pub struct LightMaterial {
    /// World-space position of the light source (fed per-frame).
    pub light_pos: Vec2,

    /// Light intensity / brightness multiplier (raw star intensity).
    pub light_intensity: f32,

    /// RGB colour of the emitted light.
    pub light_color: Vec3,

    /// Minimum ambient light strength applied to the base colour.
    pub ambient_strength: f32,

    /// Base colour of the body (RGB from `CelestialBody.color`; the alpha
    /// channel carries the Move-tool drag transparency feedback).
    pub base_color: Color,

    /// World-space centre of the body (used for the polar-UV fallback).
    pub body_pos: Vec2,

    /// Radius of the body in world units (polar-UV fallback).
    pub body_radius: f32,

    /// Star falloff constant (shader attenuation `1/(1 + d²·falloff)`).
    pub falloff: f32,

    /// 1 when a normal map is bound and should be sampled, 0 for flat shading.
    pub has_normal_map: u32,

    /// Optional normal map for per-pixel directional lighting.
    /// When `None` (or `has_normal_map == 0`) the shader falls back to a
    /// front-facing normal, producing a flat radial gradient.
    #[texture(1)]
    #[sampler(2)]
    pub normal_map: Option<Handle<Image>>,
}

impl Default for LightMaterial {
    fn default() -> Self {
        Self {
            light_pos: Vec2::ZERO,
            light_intensity: 1.0,
            light_color: Vec3::ONE,
            ambient_strength: 0.12,
            base_color: Color::WHITE,
            body_pos: Vec2::ZERO,
            body_radius: 1.0,
            falloff: 0.0001,
            has_normal_map: 0,
            normal_map: None,
        }
    }
}

impl From<&LightMaterial> for LightMaterialUniform {
    fn from(material: &LightMaterial) -> Self {
        Self {
            light_pos: material.light_pos,
            light_intensity: material.light_intensity,
            light_color: material.light_color,
            ambient_strength: material.ambient_strength,
            base_color: LinearRgba::from(material.base_color).to_f32_array().into(),
            body_pos: material.body_pos,
            body_radius: material.body_radius,
            falloff: material.falloff,
            has_normal_map: material.has_normal_map,
        }
    }
}

impl Material2d for LightMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/light_material.wgsl".into()
    }

    /// The Move tool writes a temporary transparency on `base_color.a`;
    /// the pipeline must switch to alpha blending while it is < 1.0.
    fn alpha_mode(&self) -> AlphaMode2d {
        if self.base_color.alpha() < 1.0 {
            AlphaMode2d::Blend
        } else {
            AlphaMode2d::Opaque
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  LightingPlugin — registers the material and its asset pipeline
// ═══════════════════════════════════════════════════════════════════

/// Plugin that registers [`LightMaterial`] with the renderer.
///
/// This is purely setup — per-frame lighting update systems live in
/// [`LightPlugin`](crate::systems::light::LightPlugin).
pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        // Register the custom material so Bevy knows about its shader
        // and bind-group layout.  The render-app counterpart is handled
        // automatically by `Material2dPlugin`.
        app.add_plugins(Material2dPlugin::<LightMaterial>::default());
    }
}
