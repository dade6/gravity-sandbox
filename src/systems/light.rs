use bevy::color::Srgba;
use bevy::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::components::lighting::{AmbientLight, LightInfo, LightSource};

/// Maximum distance beyond which a body receives only ambient light (no direct light).
const MAX_LIGHT_DISTANCE: f32 = 3000.0;

/// Plugin for the lighting system.
///
/// Registers the `AmbientLight` resource and adds systems to:
/// 1. Auto-attach `LightSource` to luminous bodies
/// 2. Compute `LightInfo` for each non-luminous body (nearest star, direction, intensity)
/// 3. Apply lighting to body colors via `ColorMaterial`
pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AmbientLight>().add_systems(
            Update,
            (
                init_light_sources,
                compute_lighting,
                apply_lighting_to_materials,
            )
                .chain(),
        );
    }
}

/// Auto-add `LightSource` to luminous bodies that don't have one yet.
/// This runs every frame but is a no-op after the first insertion per entity.
fn init_light_sources(
    query: Query<(Entity, &CelestialBody), (Without<LightSource>,)>,
    mut commands: Commands,
) {
    for (entity, body) in query.iter() {
        if body.luminous {
            commands.entity(entity).insert(LightSource::default());
        }
    }
}

/// For each non-luminous body, find the nearest star and compute:
/// - light direction vector (normalized toward the star)
/// - received intensity (with distance falloff)
/// - distance to star
///
/// Bodies beyond `MAX_LIGHT_DISTANCE` from any star receive 0 direct light
/// (only ambient light from `AmbientLight` resource).
fn compute_lighting(
    stars: Query<(&GlobalTransform, &LightSource)>,
    mut bodies: Query<(
        Entity,
        &CelestialBody,
        &GlobalTransform,
        Option<&mut LightInfo>,
    )>,
    mut commands: Commands,
) {
    // Collect star positions, intensities, falloffs
    let star_data: Vec<(Vec2, f32, f32)> = stars
        .iter()
        .map(|(xform, ls)| (xform.translation().truncate(), ls.intensity, ls.falloff))
        .collect();

    if star_data.is_empty() {
        // No stars — insert/update all bodies with zero direct light
        for (entity, _body, _xform, existing_light) in bodies.iter_mut() {
            match existing_light {
                Some(mut li) => {
                    li.direction = Vec2::ZERO;
                    li.intensity = 0.0;
                    li.distance_to_star = f32::MAX;
                }
                None => {
                    commands.entity(entity).insert(LightInfo {
                        direction: Vec2::ZERO,
                        intensity: 0.0,
                        distance_to_star: f32::MAX,
                    });
                }
            }
        }
        return;
    }

    for (entity, _body, xform, existing_light) in bodies.iter_mut() {
        let body_pos = xform.translation().truncate();

        // Find nearest star
        let mut nearest: Option<(Vec2, f32, f32)> = None;
        let mut nearest_dsq = f32::MAX;

        for &(sp, si, sf) in &star_data {
            let dsq = body_pos.distance_squared(sp);
            if dsq < nearest_dsq {
                nearest_dsq = dsq;
                nearest = Some((sp, si, sf));
            }
        }

        if let Some((star_pos, intensity, falloff)) = nearest {
            let dist = nearest_dsq.sqrt();
            let direction = if dist > 0.001 {
                (star_pos - body_pos).normalize()
            } else {
                Vec2::ZERO
            };

            let received = if dist > MAX_LIGHT_DISTANCE {
                0.0
            } else {
                intensity / (1.0 + dist * dist * falloff)
            };

            match existing_light {
                Some(mut li) => {
                    li.direction = direction;
                    li.intensity = received;
                    li.distance_to_star = dist;
                }
                None => {
                    commands.entity(entity).insert(LightInfo {
                        direction,
                        intensity: received,
                        distance_to_star: dist,
                    });
                }
            }
        }
    }
}

/// Apply computed lighting to body colors at the `ColorMaterial` level.
///
/// Final color = base color × (ambient + directional light intensity),
/// clamped to [0, 1].
///
/// Note: This is the simple rendering approach without a custom WGSL shader.
/// The shader-based approach (custom `Material2d`) is set up as a predisposition
/// in `assets/shaders/light_material.wgsl` and can be activated when normal
/// mapping support (Ticket M5) is added.
fn apply_lighting_to_materials(
    query: Query<(&CelestialBody, &LightInfo, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    ambient: Res<AmbientLight>,
) {
    for (body, light, mat_handle) in query.iter() {
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            let base = Srgba::new(body.color[0], body.color[1], body.color[2], 1.0);
            let factor = (ambient.intensity + light.intensity).min(1.0);
            let linear: bevy::color::LinearRgba = base.into();
            let dimmed: Srgba = (linear * factor).into();
            mat.color = Color::from(dimmed);
        }
    }
}
