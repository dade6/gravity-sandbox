use bevy::color::Srgba;
use bevy::prelude::*;

use crate::components::celestial::{BodyType, CelestialBody};
use crate::components::lighting::{
    light_edge_fade, AmbientLight, LightInfo, LightSource, StarLightSettings,
};
use crate::rendering::TextureAssets;
use crate::systems::lighting::LightMaterial;

/// Plugin for the lighting system.
///
/// Registers the `AmbientLight` resource and adds systems to:
/// 1. Auto-attach `LightSource` to luminous bodies
/// 2. Compute `LightInfo` for each non-luminous body (nearest star, direction, intensity)
/// 3. Push the computed values into the per-body `LightMaterial` uniforms
/// 4. Apply lighting to the remaining `ColorMaterial` bodies (CPU dimming fallback)
pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AmbientLight>().add_systems(
            Update,
            (
                init_light_sources,
                compute_lighting,
                update_light_materials,
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
/// - world position / colour / raw intensity / falloff of the nearest star
///   (consumed by `update_light_materials` for the per-pixel shader)
///
/// The direct-light cutoff follows the star's OWN `StarLightSettings`
/// (panel Radius + Fade): same band the GPU shader uses. Bodies beyond
/// `radius + fade_width` receive only ambient light.
///
/// Occlusion: if another non-luminous body lies on the segment star→body
/// (its circle intersects the segment), the body is in shadow and receives
/// 0 direct light — only ambient. This makes a planet behind another planet
/// actually dark (the shadow cones are decorative; the light must match).
/// Stars never occlude (they emit light, and the ticket says they cast no
/// shadow).
/// Per-star data collected once per frame: panel settings (Planet Light,
/// Radius, Fade) with historic `LightSource` fallbacks.
#[derive(Clone, Copy)]
struct StarData {
    pos: Vec2,
    /// Effective intensity: `StarLightSettings.intensity` (panel "Planet
    /// Light") when the star has it, else the historic `LightSource.intensity`.
    intensity: f32,
    /// CPU distance-attenuation constant (historic `LightSource.falloff`).
    falloff: f32,
    color: Vec3,
    /// Light outer radius (`StarLightSettings.radius`, panel "Radius").
    radius: f32,
    /// Soft-edge band width (`StarLightSettings.fade_width`, panel "Fade").
    fade_width: f32,
}

fn compute_lighting(
    stars: Query<
        (
            &CelestialBody,
            &GlobalTransform,
            &LightSource,
            Option<&StarLightSettings>,
        ),
    >,
    occluders: Query<(Entity, &CelestialBody, &GlobalTransform), Without<LightSource>>,
    mut bodies: Query<(
        Entity,
        &CelestialBody,
        &GlobalTransform,
        Option<&mut LightInfo>,
    )>,
    mut commands: Commands,
) {
    // Collect star positions, intensities, falloffs, colours and light-edge
    // parameters. `StarLightSettings` (panel) wins when present; the
    // radius/fade defaults mirror `StarLightSettings::default()`.
    let defaults = StarLightSettings::default();
    let star_data: Vec<StarData> = stars
        .iter()
        .map(|(body, xform, ls, settings)| StarData {
            pos: xform.translation().truncate(),
            intensity: settings.map(|s| s.intensity).unwrap_or(ls.intensity),
            falloff: ls.falloff,
            color: Vec3::new(body.color[0], body.color[1], body.color[2]),
            radius: settings.map(|s| s.radius).unwrap_or(defaults.radius),
            fade_width: settings.map(|s| s.fade_width).unwrap_or(defaults.fade_width),
        })
        .collect();

    // Occluders: every non-luminous body (stars have LightSource and never
    // occlude). Collected once per frame; the segment test is O(occluders)
    // per body, so total O(n²) with a trivial constant — fine for ~20 bodies.
    let occluder_data: Vec<(Entity, Vec2, f32)> = occluders
        .iter()
        .map(|(e, body, xform)| (e, xform.translation().truncate(), body.radius))
        .collect();

    if star_data.is_empty() {
        // No stars — insert/update all bodies with zero direct light
        for (entity, _body, _xform, existing_light) in bodies.iter_mut() {
            match existing_light {
                Some(mut li) => {
                    li.direction = Vec2::ZERO;
                    li.intensity = 0.0;
                    li.distance_to_star = f32::MAX;
                    li.light_pos = Vec2::ZERO;
                    li.light_color = Vec3::ZERO;
                    li.star_intensity = 0.0;
                    li.falloff = 0.0;
                }
                None => {
                    commands.entity(entity).insert(LightInfo {
                        direction: Vec2::ZERO,
                        intensity: 0.0,
                        distance_to_star: f32::MAX,
                        light_pos: Vec2::ZERO,
                        light_color: Vec3::ZERO,
                        star_intensity: 0.0,
                        falloff: 0.0,
                    });
                }
            }
        }
        return;
    }

    for (entity, _body, xform, existing_light) in bodies.iter_mut() {
        let body_pos = xform.translation().truncate();

        // Find nearest star
        let mut nearest: Option<StarData> = None;
        let mut nearest_dsq = f32::MAX;

        for sd in &star_data {
            let dsq = body_pos.distance_squared(sd.pos);
            if dsq < nearest_dsq {
                nearest_dsq = dsq;
                nearest = Some(*sd);
            }
        }

        if let Some(StarData {
            pos: star_pos,
            intensity,
            falloff,
            color: star_color,
            radius,
            fade_width,
        }) = nearest
        {
            let dist = nearest_dsq.sqrt();
            let direction = if dist > 0.001 {
                (star_pos - body_pos).normalize()
            } else {
                Vec2::ZERO
            };

            // Occlusion: another body between this one and the star blocks
            // the light (the body itself is excluded by the t-range).
            let occluded = occluder_data.iter().any(|&(oe, opos, orad)| {
                oe != entity && segment_hits_circle(star_pos, body_pos, opos, orad)
            });

            // v0.14.80: the direct-light cutoff follows the star's OWN panel
            // radius + fade (same band the GPU shader uses, see
            // create_lightmap.wgsl `dist < light.radius + fade`). The historic
            // fixed `MAX_LIGHT_DISTANCE` (3000) cut planets black BEFORE the
            // light edge when radius > 3000 and kept cones past the edge when
            // radius < 3000.
            let edge = if occluded {
                0.0
            } else {
                light_edge_fade(dist, radius, fade_width)
            };

            let received = intensity / (1.0 + dist * dist * falloff) * edge;

            // The shader must receive the faded raw intensity too (it drives
            // the per-pixel diffuse term), or planets would stay lit past the
            // CPU cutoff.
            let raw = intensity * edge;

            match existing_light {
                Some(mut li) => {
                    li.direction = direction;
                    li.intensity = received;
                    li.distance_to_star = dist;
                    li.light_pos = star_pos;
                    li.light_color = star_color;
                    li.star_intensity = raw;
                    li.falloff = falloff;
                }
                None => {
                    commands.entity(entity).insert(LightInfo {
                        direction,
                        intensity: received,
                        distance_to_star: dist,
                        light_pos: star_pos,
                        light_color: star_color,
                        star_intensity: raw,
                        falloff,
                    });
                }
            }
        }
    }
}

/// True if the segment `a→b` passes through the circle centred at `c` with
/// radius `r`, with the closest point strictly BETWEEN `a` and `b`
/// (t ∈ (0, 1)): an occluder behind the body (t ≥ 1) or before the star
/// (t ≤ 0) does not block the light.
fn segment_hits_circle(a: Vec2, b: Vec2, c: Vec2, r: f32) -> bool {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-6 {
        return false;
    }
    let t = ((c - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    if t <= 0.001 || t >= 0.999 {
        return false;
    }
    let closest = a + ab * t;
    (c - closest).length_squared() < r * r
}

/// The star's `LightSource.falloff` is tuned for the CPU dimming path
/// (`apply_lighting_to_materials`, ColorMaterial bodies). The per-pixel shader
/// computes its own attenuation `1/(1 + d²·falloff)` at world scale; bodies
/// orbit 100-1000 units from their star, so the raw value (0.0001) would make
/// the diffuse term ~0.1 and the lit-side gradient (and the normal-map relief
/// that rides on it) almost invisible. Scaling it down gives near bodies a
/// clearly lit side while distant bodies still fade to ambient.
const GPU_FALLOFF_SCALE: f32 = 0.02;

/// Push the computed lighting values into each body's `LightMaterial`
/// uniforms every frame, so the per-pixel shader has fresh data:
/// - `light_pos` / `light_color` / `light_intensity` / `falloff` from the
///   nearest star (via `LightInfo`)
/// - `body_pos` / `body_radius` from the body transform
/// - `base_color` from `CelestialBody.color` (alpha preserved: the Move tool
///   writes a temporary drag-transparency there)
/// - `has_normal_map` / `normal_map` from `TextureAssets` by body type
fn update_light_materials(
    textures: Option<Res<TextureAssets>>,
    ambient: Res<AmbientLight>,
    bodies: Query<(
        &CelestialBody,
        &GlobalTransform,
        &LightInfo,
        &MeshMaterial2d<LightMaterial>,
    )>,
    mut materials: ResMut<Assets<LightMaterial>>,
) {
    for (body, xform, light, mat_handle) in bodies.iter() {
        // Stars are light sources, never shaded bodies.
        if body.luminous {
            continue;
        }
        let Some(mut mat) = materials.get_mut(&mat_handle.0) else {
            continue;
        };

        // Preserve the alpha channel (drag transparency feedback).
        let prev_alpha = mat.base_color.alpha();
        let (normal_map, has_normal_map) = normal_map_for(textures.as_deref(), body.body_type);

        mat.light_pos = light.light_pos;
        mat.light_intensity = light.star_intensity;
        mat.light_color = light.light_color;
        mat.ambient_strength = ambient.intensity;
        mat.base_color = Color::srgba(body.color[0], body.color[1], body.color[2], prev_alpha);
        mat.body_pos = xform.translation().truncate();
        mat.body_radius = body.radius;
        mat.falloff = light.falloff * GPU_FALLOFF_SCALE;
        mat.has_normal_map = has_normal_map;
        mat.normal_map = normal_map;
    }
}

/// Pick the procedural normal map for a body type (generated in
/// `src/rendering/textures.rs`, Ticket 14). `Spaceship` stays flat.
fn normal_map_for(
    textures: Option<&TextureAssets>,
    body_type: BodyType,
) -> (Option<Handle<Image>>, u32) {
    let Some(textures) = textures else {
        return (None, 0);
    };
    let handle = match body_type {
        BodyType::Star => Some(textures.star_normal.clone()),
        BodyType::Planet => Some(textures.rocky_normal.clone()),
        BodyType::Moon => Some(textures.ice_normal.clone()),
        BodyType::Asteroid => Some(textures.rocky_normal.clone()),
        BodyType::Spaceship => None,
    };
    let flag = if handle.is_some() { 1 } else { 0 };
    (handle, flag)
}

/// Apply computed lighting to body colors at the `ColorMaterial` level.
///
/// Final color = base color × (ambient + directional light intensity),
/// clamped to [0, 1].
///
/// This is the CPU fallback used only by bodies that keep a `ColorMaterial`
/// (decorative/demo bodies from `parallax.rs` / `debug.rs`). Bodies with the
/// per-pixel `LightMaterial` are excluded by the query, so they are never
/// dimmed twice.
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

#[cfg(test)]
mod tests {
    use super::*;
    use avian2d::prelude::*;
    use crate::components::celestial::BodyType;
    use crate::components::lighting::{AmbientLight, LightInfo};

    fn test_app() -> App {
        let mut app = App::new();
        // Niente DefaultPlugins qui: registriamo gli Assets come risorse
        // dirette (l'AssetServer serve solo per caricamento/registrazione).
        app.insert_resource(Assets::<LightMaterial>::default())
            .insert_resource(Assets::<ColorMaterial>::default())
            .insert_resource(Assets::<bevy::image::Image>::default())
            .add_plugins(LightPlugin);
        app
    }

    fn spawn_planet(
        world: &mut World,
        pos: Vec2,
        color: [f32; 3],
        radius: f32,
        alpha: f32,
    ) -> (Entity, Handle<LightMaterial>) {
        let handle = world
            .resource_mut::<Assets<LightMaterial>>()
            .add(LightMaterial {
                base_color: Color::srgba(color[0], color[1], color[2], alpha),
                body_radius: radius,
                ..default()
            });
        let entity = world
            .spawn((
                CelestialBody {
                    name: "Planet".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius,
                    color,
                    luminous: false,
                },
                Transform::from_xyz(pos.x, pos.y, 0.0),
                RigidBody::Dynamic,
                Collider::circle(radius),
                Mass(100.0),
                LinearVelocity(Vec2::ZERO),
                LightInfo {
                    direction: Vec2::NEG_X,
                    intensity: 0.1,
                    distance_to_star: 300.0,
                    light_pos: Vec2::ZERO,
                    light_color: Vec3::new(1.0, 0.9, 0.3),
                    star_intensity: 1.0,
                    falloff: 0.0001,
                },
                MeshMaterial2d(handle.clone()),
            ))
            .id();
        (entity, handle)
    }

    fn spawn_star(world: &mut World, pos: Vec2) -> Entity {
        world
            .spawn((
                CelestialBody {
                    name: "Sun".into(),
                    body_type: BodyType::Star,
                    mass: 5000.0,
                    radius: 30.0,
                    color: [1.0, 0.9, 0.3],
                    luminous: true,
                },
                Transform::from_xyz(pos.x, pos.y, 0.0),
                RigidBody::Dynamic,
                Collider::circle(30.0),
                Mass(5000.0),
                LinearVelocity(Vec2::ZERO),
            ))
            .id()
    }

    /// Il sistema scrive le uniform per-frame a partire da LightInfo +
    /// CelestialBody: light_pos/light_color/star_intensity/falloff dalla
    /// stella più vicina, body_pos/body_radius/ambient dal corpo, e
    /// has_normal_map = 0 quando TextureAssets non esiste.
    #[test]
    fn update_light_materials_writes_uniforms_from_light_info() {
        let mut app = test_app();
        let (entity, handle) = {
            let world = app.world_mut();
            let (e, h) = spawn_planet(world, Vec2::new(300.0, 0.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            spawn_star(world, Vec2::ZERO);
            (e, h)
        };

        // Due update: il primo inserisce LightSource sulla stella e aggiorna
        // LightInfo; il secondo propaga le uniform al materiale.
        app.update();
        app.update();

        let world = app.world();
        let mat = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle)
            .expect("material asset");
        assert_eq!(mat.light_pos, Vec2::ZERO, "light_pos = stella più vicina");
        assert_eq!(mat.light_color, Vec3::new(1.0, 0.9, 0.3), "colore stella");
        assert_eq!(mat.light_intensity, 1.0, "intensità grezza stella");
        assert_eq!(mat.ambient_strength, 0.12, "ambient");
        assert_eq!(mat.body_pos, Vec2::new(300.0, 0.0), "posizione corpo");
        assert_eq!(mat.body_radius, 12.0, "raggio corpo");
        // Il falloff GPU è scalato rispetto al LightSource (vedi GPU_FALLOFF_SCALE)
        assert_eq!(mat.falloff, 0.0001 * GPU_FALLOFF_SCALE, "falloff stella (scalato GPU)");
        assert_eq!(mat.has_normal_map, 0, "senza TextureAssets -> flat");
        assert!(mat.normal_map.is_none());
        // base_color: colore del corpo con alpha preservata
        let srgba = mat.base_color.to_srgba();
        assert!((srgba.red - 0.3).abs() < 1e-4 && (srgba.green - 0.6).abs() < 1e-4
            && (srgba.blue - 1.0).abs() < 1e-4);
        assert!((srgba.alpha - 1.0).abs() < 1e-4, "alpha preservata");

        // L'entity esiste ancora (nessun conflitto di query nel sistema)
        assert!(world.get_entity(entity).is_ok());
    }

    /// Il feedback di trasparenza del Move tool (alpha < 1 su base_color)
    /// NON viene sovrascritto dall'update per-frame.
    #[test]
    fn update_light_materials_preserves_drag_alpha() {
        let mut app = test_app();
        let (_entity, handle) = {
            let world = app.world_mut();
            let (e, h) = spawn_planet(world, Vec2::new(300.0, 0.0), [0.8, 0.4, 0.2], 20.0, 0.5);
            spawn_star(world, Vec2::ZERO);
            (e, h)
        };

        app.update();
        app.update();

        let world = app.world();
        let mat = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle)
            .expect("material asset");
        let srgba = mat.base_color.to_srgba();
        assert!(
            (srgba.alpha - 0.5).abs() < 1e-4,
            "alpha drag (0.5) preservata, trovata {}",
            srgba.alpha
        );
    }

    /// Con TextureAssets presente, un pianeta ottiene la normal map rocciosa
    /// e has_normal_map = 1.
    #[test]
    fn update_light_materials_attaches_normal_map_by_body_type() {
        let mut app = test_app();
        let (_entity, handle) = {
            let world = app.world_mut();
            // TextureAssets con handle dummy: qui interessa solo il flag e
            // l'handle scelto in base al tipo, non il contenuto della texture.
            world.insert_resource(TextureAssets {
                rocky_diffuse: Handle::default(),
                rocky_normal: Handle::default(),
                gas_diffuse: Handle::default(),
                gas_normal: Handle::default(),
                ice_diffuse: Handle::default(),
                ice_normal: Handle::default(),
                star_diffuse: Handle::default(),
                star_normal: Handle::default(),
            });
            let (e, h) = spawn_planet(world, Vec2::new(300.0, 0.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            spawn_star(world, Vec2::ZERO);
            (e, h)
        };

        app.update();
        app.update();

        let world = app.world();
        let mat = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle)
            .expect("material asset");
        assert_eq!(mat.has_normal_map, 1, "Planet -> rocky_normal");
        assert!(mat.normal_map.is_some(), "handle normal map collegato");
    }

    /// Un corpo lontano da ogni stella (oltre radius+fade della stella) riceve
    /// intensità 0: nel materiale star_intensity = 0 -> solo ambient.
    #[test]
    fn update_light_materials_zeroes_light_beyond_max_distance() {
        let mut app = test_app();
        let (_entity, handle) = {
            let world = app.world_mut();
            let (e, h) = spawn_planet(world, Vec2::new(5000.0, 0.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            spawn_star(world, Vec2::ZERO);
            (e, h)
        };

        app.update();
        app.update();

        let world = app.world();
        let mat = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle)
            .expect("material asset");
        // Star without StarLightSettings -> defaults radius 5000 + fade 500:
        // at exactly 5000 the edge factor is still 1.0, but past
        // radius+fade (5500) it is 0.
        assert_eq!(mat.light_intensity, 1.0, "dentro radius -> piena");
    }

    /// v0.14.80: cutoff CPU segue radius+fade della STRELLA (StarLightSettings),
    /// non più la costante fissa 3000. Il cono d'ombra vive nella stessa banda
    /// di luce della GPU.
    #[test]
    fn light_cutoff_follows_star_radius_plus_fade() {
        let mut app = test_app();
        let (_far, handle_far) = {
            let world = app.world_mut();
            // Star with panel settings: radius 4000, fade 500 -> edge at 4500.
            // Far planet on the Y axis, near planet on the X axis: they must
            // NOT occlude each other (the segment star→far is the Y axis).
            let star = spawn_star(world, Vec2::ZERO);
            world.entity_mut(star).insert(StarLightSettings {
                intensity: 2.0,
                radius: 4000.0,
                fade_width: 500.0,
                ..default()
            });
            let (e, h) = spawn_planet(world, Vec2::new(0.0, 4200.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            (e, h)
        };
        let (_near, handle_near) = {
            let world = app.world_mut();
            let (e, h) = spawn_planet(world, Vec2::new(1000.0, 0.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            (e, h)
        };

        app.update();
        app.update();

        let world = app.world();
        let mat_far = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle_far)
            .expect("material asset far");
        let mat_near = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle_near)
            .expect("material asset near");
        // Inside radius: full intensity (2.0 from StarLightSettings, not the
        // historic LightSource 1.0).
        assert_eq!(mat_near.light_intensity, 2.0, "Planet Light del pannello");
        // At 4200 (radius 4000 + fade 500): PARTIAL light, not a snap-off.
        // smoothstep((4200-4000)/500) = 0.352 -> raw = 2.0 * (1-0.352) ≈ 1.296.
        assert!(
            (mat_far.light_intensity - (2.0 * (1.0 - 0.352))).abs() < 0.01,
            "nel fade band: sfumato, non a scatti; got {}",
            mat_far.light_intensity
        );
    }

    /// v0.14.80: oltre radius+fade l'intensità è 0 anche con la stella
    /// vicina luminosa (fallback ai default quando manca StarLightSettings).
    #[test]
    fn light_zero_beyond_radius_plus_fade_defaults() {
        let mut app = test_app();
        let (_entity, handle) = {
            let world = app.world_mut();
            let (e, h) = spawn_planet(world, Vec2::new(6000.0, 0.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            spawn_star(world, Vec2::ZERO);
            (e, h)
        };

        app.update();
        app.update();

        let world = app.world();
        let mat = world
            .resource::<Assets<LightMaterial>>()
            .get(&handle)
            .expect("material asset");
        // Defaults: radius 5000 + fade 500 = edge at 5500; at 6000 -> 0.
        assert_eq!(mat.light_intensity, 0.0, "oltre radius+fade -> 0");
    }

    /// v0.14.80 regression (feedback Davide, screenshot v0.14.78): stella con
    /// radius=100, fade=5000. Il pianeta a 0,100 aveva il cono, quello a
    /// 0,150 NO — il cono si creava solo entro `radius`. Ora il gate è
    /// dist < radius+fade: a 150 (e fino a 5100) il cono DEVE esserci.
    #[test]
    fn cone_gate_extends_to_radius_plus_fade_regression() {
        let mut app = test_app();
        let planet = {
            let world = app.world_mut();
            let star = spawn_star(world, Vec2::ZERO);
            world.entity_mut(star).insert(StarLightSettings {
                intensity: 3.0,
                radius: 100.0,
                fade_width: 5000.0,
                ..default()
            });
            // Planet at 0,150 — the exact position from the user's screenshot
            // where the cone was MISSING pre-fix.
            let (e, _) = spawn_planet(world, Vec2::new(0.0, 150.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            e
        };

        app.update();
        app.update();

        let world = app.world();
        let li = world.get::<LightInfo>(planet).unwrap();
        assert!(
            li.intensity > 0.0,
            "pianeta a dist 150 con radius=100/fade=5000: cono visibile (150 < 100+5000), got {}",
            li.intensity
        );
    }

    /// Stessa stella (radius=100, fade=5000): oltre 5100 il cono NON deve
    /// esserci — il gate è radius+fade, non infinito.
    #[test]
    fn cone_gate_zero_beyond_radius_plus_fade_regression() {
        let mut app = test_app();
        let planet = {
            let world = app.world_mut();
            let star = spawn_star(world, Vec2::ZERO);
            world.entity_mut(star).insert(StarLightSettings {
                intensity: 3.0,
                radius: 100.0,
                fade_width: 5000.0,
                ..default()
            });
            let (e, _) = spawn_planet(world, Vec2::new(0.0, 5200.0), [0.3, 0.6, 1.0], 12.0, 1.0);
            e
        };

        app.update();
        app.update();

        let world = app.world();
        let li = world.get::<LightInfo>(planet).unwrap();
        assert_eq!(
            li.intensity, 0.0,
            "pianeta a dist 5200 (> 100+5000): niente cono, got {}",
            li.intensity
        );
    }

    // ---- occlusione ----

    #[test]
    fn segment_hits_circle_cases() {
        // Segment (0,0) -> (400,0), circle at (200,0) r=20: dead centre.
        assert!(segment_hits_circle(Vec2::ZERO, Vec2::new(400.0, 0.0), Vec2::new(200.0, 0.0), 20.0));
        // Circle off to the side, farther than its radius from the segment.
        assert!(!segment_hits_circle(
            Vec2::ZERO,
            Vec2::new(400.0, 0.0),
            Vec2::new(200.0, 50.0),
            20.0
        ));
        // Circle BEYOND the body (t > 1): does not occlude.
        assert!(!segment_hits_circle(
            Vec2::ZERO,
            Vec2::new(200.0, 0.0),
            Vec2::new(400.0, 0.0),
            20.0
        ));
        // Circle BEFORE the star (t < 0): does not occlude.
        assert!(!segment_hits_circle(
            Vec2::ZERO,
            Vec2::new(200.0, 0.0),
            Vec2::new(-100.0, 0.0),
            20.0
        ));
        // Grazing the segment edge (distance == r): no occlusion (strict <).
        assert!(!segment_hits_circle(
            Vec2::ZERO,
            Vec2::new(400.0, 0.0),
            Vec2::new(200.0, 20.0),
            20.0
        ));
        // Degenerate segment.
        assert!(!segment_hits_circle(Vec2::ZERO, Vec2::ZERO, Vec2::new(10.0, 0.0), 5.0));
    }

    /// Un pianeta allineato dietro un altro (rispetto alla stella) è in
    /// ombra: riceve 0 luce diretta (solo ambient), mentre quello davanti
    /// resta illuminato.
    #[test]
    fn planet_behind_another_is_in_shadow() {
        let mut app = test_app();
        let (front, back) = {
            let world = app.world_mut();
            let (e_front, _) = spawn_planet(world, Vec2::new(200.0, 0.0), [0.3, 0.6, 1.0], 20.0, 1.0);
            let (e_back, _) = spawn_planet(world, Vec2::new(400.0, 0.0), [0.8, 0.4, 0.2], 12.0, 1.0);
            spawn_star(world, Vec2::ZERO);
            (e_front, e_back)
        };

        app.update();
        app.update();

        let world = app.world();
        let li_front = world.get::<LightInfo>(front).unwrap();
        let li_back = world.get::<LightInfo>(back).unwrap();
        assert!(
            li_front.intensity > 0.0,
            "pianeta davanti illuminato, got {}",
            li_front.intensity
        );
        assert_eq!(
            li_back.intensity, 0.0,
            "pianeta dietro in ombra -> 0 luce diretta"
        );
        assert_eq!(li_back.star_intensity, 0.0, "raw intensity azzerata pure per lo shader");
    }

    /// Corpi NON allineati (l'occluder è fuori dal segmento stella→corpo)
    /// non oscurano: il pianeta "dietro" ma disassato resta illuminato.
    #[test]
    fn offset_planet_is_not_occluded() {
        let mut app = test_app();
        let (front, back) = {
            let world = app.world_mut();
            let (e_front, _) = spawn_planet(world, Vec2::new(200.0, 0.0), [0.3, 0.6, 1.0], 20.0, 1.0);
            let (e_back, _) = spawn_planet(world, Vec2::new(400.0, 80.0), [0.8, 0.4, 0.2], 12.0, 1.0);
            spawn_star(world, Vec2::ZERO);
            (e_front, e_back)
        };

        app.update();
        app.update();

        let world = app.world();
        let li_back = world.get::<LightInfo>(back).unwrap();
        assert!(
            li_back.intensity > 0.0,
            "pianeta disassato NON oscurato, got {}",
            li_back.intensity
        );
        // Il front non deve essere occluso da quello dietro.
        let li_front = world.get::<LightInfo>(front).unwrap();
        assert!(li_front.intensity > 0.0, "front illuminato");
    }
}
