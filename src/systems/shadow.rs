//! Ticket 10 — Ombre proiettate (shadow cones)
//!
//! Every non-luminous body lit by a star casts a dark cone away from the star:
//! - geometry: tangent semi-angle α = asin(r/d); the cone base is the body's
//!   silhouette as seen from the star — the two tangent rays graze the body's
//!   SIDES (the true tangent points sit at (±r along the perpendicular), NOT
//!   at ±α from the axis — those would collapse to a ~1px-wide base for small
//!   α). The far end extends along the two diverging tangent rays for
//!   `clamp(distance × k, min, max)` units, so the cone widens as it moves
//!   away from the star and its width always scales with the body's diameter
//! - rendering: a black `ColorMaterial` quad at alpha ~0.3, spawned as a
//!   child of the body at z = -1 (below every body, which live at z = 0), so
//!   it follows the body for free; vertices are updated in place per frame
//!   (`Mesh::insert_attribute`) on a pre-allocated 4-vertex mesh — no
//!   per-frame allocations
//! - culling: stars never cast shadows; bodies with no valid light
//!   (`LightInfo.intensity <= 0`), degenerate geometry (star inside/touching
//!   the body) or radius below the visibility threshold are hidden
//!
//! The scene background is pure black (`ClearColor`), so a black cone would be
//! invisible on its own: a soft radial glow is drawn around each star
//! (z = -2, also a child) giving the cones a faint lit backdrop to darken.
//! This is the only deviation from the ticket spec and is tunable below.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::sprite_render::AlphaMode2d;

use crate::components::celestial::CelestialBody;
use crate::components::lighting::LightInfo;
use crate::rendering::textures::build_image;

// ============================================================
// Tuning
// ============================================================

/// Opacity of the shadow mesh (independent of distance).
const SHADOW_ALPHA: f32 = 0.3;
/// Cone length = distance_to_star × this factor.
const SHADOW_LENGTH_FACTOR: f32 = 1.0;
/// Cone length clamps.
const SHADOW_MIN_LENGTH: f32 = 30.0;
const SHADOW_MAX_LENGTH: f32 = 1500.0;
/// Bodies smaller than this (world units) cast no shadow (perf cull).
///
/// NOTE: the ticket says "raggio visibile < 50px → nessuna ombra". At default
/// zoom every preset body (radius 5-20) is under 50px, which would cull ALL
/// shadows and fail the acceptance test "cono d'ombra visibile dietro il
/// pianeta". The threshold is therefore interpreted as a tiny-body cull.
const SHADOW_MIN_RADIUS: f32 = 2.0;
/// Render depth of the shadow cone (bodies live at z = 0).
const SHADOW_Z: f32 = -1.0;
/// Render depth of the star glow (below the shadows).
const GLOW_Z: f32 = -2.0;
/// Glow radius = star radius × this factor.
const GLOW_RADIUS_FACTOR: f32 = 25.0;
/// Subtle alpha of the star glow.
const GLOW_ALPHA: f32 = 0.18;
/// Glow texture size (px, square).
const GLOW_TEX_SIZE: u32 = 128;

// ============================================================
// Components
// ============================================================

/// Marker on the shadow mesh entity (a child of the casting body).
#[derive(Component)]
pub struct ShadowCone;

/// Marker on bodies that already have a shadow child entity.
#[derive(Component)]
pub struct ShadowAttached;

/// Marker on stars that already have a glow child entity.
#[derive(Component)]
pub struct StarGlow;

// ============================================================
// Shared assets
// ============================================================

/// Shared assets for shadows + star glows (created at startup).
#[derive(Resource)]
pub struct ShadowAssets {
    /// Black semi-transparent material used by every shadow cone.
    pub material: Handle<ColorMaterial>,
    /// Soft radial-gradient texture (white core → transparent edge), tinted
    /// per-star through a dedicated `ColorMaterial`.
    pub glow_texture: Handle<Image>,
}

// ============================================================
// Plugin
// ============================================================

pub struct ShadowPlugin;

impl Plugin for ShadowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, init_shadow_assets).add_systems(
            Update,
            (spawn_shadow_children, spawn_star_glows, update_shadows).chain(),
        );
    }
}

/// Startup: build the shared shadow material + the radial glow texture.
fn init_shadow_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let material = materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    });
    let glow_texture = images.add(generate_glow_texture());
    commands.insert_resource(ShadowAssets {
        material,
        glow_texture,
    });
}

/// Radial gradient texture: white core fading smoothly to transparent.
fn generate_glow_texture() -> Image {
    let w = GLOW_TEX_SIZE as usize;
    let half = GLOW_TEX_SIZE as f32 / 2.0;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let dx = x as f32 + 0.5 - half;
            let dy = y as f32 + 0.5 - half;
            let d = (dx * dx + dy * dy).sqrt() / half; // 0 at centre, 1 at disc edge
            let t = (d / 0.95).clamp(0.0, 1.0);
            let a = ((1.0 - t).powf(2.0) * 255.0) as u8;
            pixels.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    build_image(pixels, GLOW_TEX_SIZE, GLOW_TEX_SIZE)
}

// ============================================================
// Spawning
// ============================================================

/// Lazy spawn: give every non-luminous body a hidden shadow child + marker.
/// One pre-allocated quad mesh per body (reused for the body's whole life —
/// reset/load despawn the parent, which removes the child with it).
fn spawn_shadow_children(
    bodies: Query<(Entity, &CelestialBody), Without<ShadowAttached>>,
    shadow_assets: Res<ShadowAssets>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (entity, body) in bodies.iter() {
        if body.luminous {
            continue; // stars never cast shadows
        }
        let mesh = meshes.add(new_shadow_mesh());
        commands
            .entity(entity)
            .insert(ShadowAttached)
            .with_children(|parent| {
                parent.spawn((
                    ShadowCone,
                    Mesh2d(mesh),
                    MeshMaterial2d::<ColorMaterial>(shadow_assets.material.clone()),
                    Transform::from_xyz(0.0, 0.0, SHADOW_Z),
                    Visibility::Hidden,
                ));
            });
    }
}

/// Lazy spawn: give every star a soft radial glow child + marker. The glow
/// follows the star automatically (child at identity transform).
fn spawn_star_glows(
    stars: Query<(Entity, &CelestialBody), Without<StarGlow>>,
    shadow_assets: Res<ShadowAssets>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, body) in stars.iter() {
        if !body.luminous {
            continue;
        }
        let mesh = meshes.add(Circle::new(body.radius * GLOW_RADIUS_FACTOR));
        let material = materials.add(ColorMaterial {
            color: Color::srgba(body.color[0], body.color[1], body.color[2], GLOW_ALPHA),
            alpha_mode: AlphaMode2d::Blend,
            texture: Some(shadow_assets.glow_texture.clone()),
            ..default()
        });
        commands.entity(entity).insert(StarGlow).with_children(|parent| {
            parent.spawn((
                Mesh2d(mesh),
                MeshMaterial2d::<ColorMaterial>(material),
                Transform::from_xyz(0.0, 0.0, GLOW_Z),
                Visibility::default(),
            ));
        });
    }
}

// ============================================================
// Per-frame update
// ============================================================

/// Recompute the cone geometry for every shadow and toggle visibility
/// following the culling rules. Only the POSITION attribute is rewritten
/// (normal/UV never change); visibility is touched only on actual changes.
///
/// The cone vertices are computed in the body's LOCAL space (the shadow mesh
/// is a child of the body). `LightInfo.direction` is world-space, so it is
/// first transformed into the body's local frame (`rot⁻¹ · dir`): otherwise a
/// body rotated by a collision (Avian leaves a non-identity rotation on the
/// transform) would drag its shadow cone with it, pointing it away from the
/// star.
fn update_shadows(
    mut shadows: Query<(&ChildOf, &Mesh2d, &mut Visibility), With<ShadowCone>>,
    bodies: Query<(&CelestialBody, &Transform, Option<&LightInfo>)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (child_of, mesh2d, mut vis) in shadows.iter_mut() {
        let (body, transform, light_info) = match bodies.get(child_of.0) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let cone = if body.luminous {
            None
        } else if let Some(li) = light_info {
            if li.intensity > 0.0
                && li.direction.length_squared() > 0.5
                && body.radius >= SHADOW_MIN_RADIUS
                && li.distance_to_star > body.radius
            {
                // World direction -> body-local frame so the cone stays
                // oriented away from the star regardless of the body's
                // rotation (e.g. after a collision).
                let local_dir = (transform.rotation.inverse() * li.direction.extend(0.0)).truncate();
                shadow_cone_vertices(body.radius, local_dir, li.distance_to_star)
            } else {
                None
            }
        } else {
            None
        };

        let want = if cone.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }

        if let Some(verts) = cone {
            if let Some(mut mesh) = meshes.get_mut(&mesh2d.0) {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    verts.map(|v| v.to_array()).to_vec(),
                );
            }
        }
    }
}

// ============================================================
// Geometry
// ============================================================

/// Compute the 4 shadow-cone vertices in body-local space for a body of
/// `radius` lit by a star at distance `dist`, with `light_dir` the NORMALIZED
/// direction FROM the body TOWARD the star.
///
/// Tangent semi-angle α = asin(r/d). The cone base is the body's silhouette
/// as seen from the star: the two tangent rays graze the body's SIDES, so the
/// base points are `±radius` along the perpendicular to the light axis (NOT
/// the circle points at ±α from the axis, which collapse to a ~1px-wide base
/// for small α). The far end extends along the two diverging tangent rays for
/// `clamp(dist × SHADOW_LENGTH_FACTOR, min, max)` units, so the cone widens
/// away from the star and its width always scales with the body's diameter.
/// Returns `None` for degenerate cases (zero radius, star inside/touching the
/// body).
fn shadow_cone_vertices(radius: f32, light_dir: Vec2, dist: f32) -> Option<[Vec3; 4]> {
    if radius <= 0.0 || dist <= radius {
        return None;
    }
    let sin_a = radius / dist; // sin(α)
    let cos_a = (1.0 - sin_a * sin_a).sqrt();
    // Cone axis: away from the star.
    let axis = -light_dir;
    let perp = Vec2::new(-axis.y, axis.x);
    // Diverging tangent-ray directions (axis rotated ±α).
    let dir_plus = axis * cos_a + perp * sin_a;
    let dir_minus = axis * cos_a - perp * sin_a;
    // Base: the body's silhouette — tangent rays graze the body's sides, so
    // the base spans the full diameter perpendicular to the light axis.
    let t_plus = perp * radius;
    let t_minus = -perp * radius;
    // Far end: extend along the diverging rays.
    let len = (dist * SHADOW_LENGTH_FACTOR).clamp(SHADOW_MIN_LENGTH, SHADOW_MAX_LENGTH);
    let f_plus = t_plus + dir_plus * len;
    let f_minus = t_minus + dir_minus * len;
    // CCW winding: t+ -> t- -> f- -> f+.
    Some([
        t_plus.extend(0.0),
        t_minus.extend(0.0),
        f_minus.extend(0.0),
        f_plus.extend(0.0),
    ])
}

/// Pre-allocated 4-vertex quad (2 triangles, CCW) used as the shadow mesh.
/// Position is rewritten in place every frame; normal/UV never change.
fn new_shadow_mesh() -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; 4]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::celestial::BodyType;

    fn test_app() -> App {
        let mut app = App::new();
        app.insert_resource(Assets::<ColorMaterial>::default())
            .insert_resource(Assets::<Image>::default())
            .insert_resource(Assets::<Mesh>::default())
            .add_plugins(ShadowPlugin);
        app
    }

    fn spawn_planet(world: &mut World, pos: Vec2, radius: f32, intensity: f32) -> Entity {
        world
            .spawn((
                CelestialBody {
                    name: "Planet".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius,
                    color: [0.3, 0.6, 1.0],
                    luminous: false,
                },
                Transform::from_xyz(pos.x, pos.y, 0.0),
                LightInfo {
                    direction: -pos.normalize_or_zero(),
                    intensity,
                    distance_to_star: pos.length(),
                    light_pos: Vec2::ZERO,
                    light_color: Vec3::new(1.0, 0.9, 0.3),
                    star_intensity: 1.0,
                    falloff: 0.0001,
                },
            ))
            .id()
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
            ))
            .id()
    }

    // ---- pure geometry ----

    #[test]
    fn cone_points_away_from_star_and_widens() {
        // Star to the right (light_dir = +X), body at origin, radius 12, d=300.
        let v = shadow_cone_vertices(12.0, Vec2::X, 300.0).expect("cone");
        // Base (t1/t2) sits on the body: |t| == radius.
        assert!((v[0].truncate().length() - 12.0).abs() < 1e-4);
        assert!((v[1].truncate().length() - 12.0).abs() < 1e-4);
        // Far end (f-/f+) is beyond the base (away from the star, -x).
        assert!(v[2].x < v[0].x && v[3].x < v[1].x);
        // The cone widens: far width > base width.
        let base_width = v[0].truncate().distance(v[1].truncate());
        let far_width = v[2].truncate().distance(v[3].truncate());
        assert!(far_width > base_width, "base {base_width} far {far_width}");
        // Far end is at distance ~ L beyond the base.
        let len = (300.0 * SHADOW_LENGTH_FACTOR).clamp(SHADOW_MIN_LENGTH, SHADOW_MAX_LENGTH);
        assert!((v[3].truncate() - v[0].truncate()).length() > len * 0.99);
    }

    #[test]
    fn cone_length_scales_with_distance() {
        let v_near = shadow_cone_vertices(12.0, Vec2::X, 300.0).unwrap();
        let v_far = shadow_cone_vertices(12.0, Vec2::X, 600.0).unwrap();
        let len_near = (v_near[3].truncate() - v_near[0].truncate()).length();
        let len_far = (v_far[3].truncate() - v_far[0].truncate()).length();
        assert!(
            len_far > len_near * 1.5,
            "far {len_far} should be much longer than near {len_near}"
        );
    }

    #[test]
    fn cone_base_spans_body_diameter_and_scales_with_radius() {
        // Base width == 2r (the body's full diameter), regardless of distance:
        // a 12-radius body at d=300 (α≈2.3°) must NOT collapse to a ~1px base.
        let v = shadow_cone_vertices(12.0, Vec2::X, 300.0).unwrap();
        let base_width = v[0].truncate().distance(v[1].truncate());
        assert!(
            (base_width - 24.0).abs() < 1e-3,
            "base {base_width} should be 2r = 24"
        );
        // Wider body -> wider base (proportional, not constant).
        let v_big = shadow_cone_vertices(40.0, Vec2::X, 300.0).unwrap();
        let base_big = v_big[0].truncate().distance(v_big[1].truncate());
        assert!((base_big - 80.0).abs() < 1e-3, "base {base_big} should be 80");
        // Base points sit ON the body silhouette (|t| == radius) and the cone
        // still widens away from the star.
        assert!((v[0].truncate().length() - 12.0).abs() < 1e-4);
        let far_width = v[2].truncate().distance(v[3].truncate());
        assert!(far_width > base_width, "far {far_width} > base {base_width}");
    }

    #[test]
    fn cone_length_respects_clamps() {
        // Very close star -> min length; very far -> max length.
        let v_close = shadow_cone_vertices(10.0, Vec2::X, 20.0).unwrap();
        let l_close = (v_close[3].truncate() - v_close[0].truncate()).length();
        assert!(l_close >= SHADOW_MIN_LENGTH - 1e-3);
        let v_far = shadow_cone_vertices(10.0, Vec2::X, 100_000.0).unwrap();
        let l_far = (v_far[3].truncate() - v_far[0].truncate()).length();
        assert!(l_far <= SHADOW_MAX_LENGTH + 1e-3);
    }

    #[test]
    fn cone_orientation_flips_with_light_side() {
        // Star to the LEFT (light_dir = -X): cone must point +x (away).
        let v = shadow_cone_vertices(12.0, Vec2::NEG_X, 300.0).unwrap();
        assert!(v[2].x > v[0].x && v[3].x > v[1].x);
        // Star above (light_dir = +Y): cone points -y... wait, away from star
        // means -Y? No: light_dir +Y (star above) -> away = -Y.
        let v2 = shadow_cone_vertices(12.0, Vec2::Y, 300.0).unwrap();
        assert!(v2[2].y < v2[0].y && v2[3].y < v2[1].y);
    }

    #[test]
    fn cone_none_for_degenerate_cases() {
        assert!(shadow_cone_vertices(0.0, Vec2::X, 300.0).is_none());
        assert!(shadow_cone_vertices(12.0, Vec2::X, 12.0).is_none()); // touching
        assert!(shadow_cone_vertices(12.0, Vec2::X, 5.0).is_none()); // inside
    }

    // ---- system behaviour ----

    #[test]
    fn lit_planet_gets_visible_shadow() {
        let mut app = test_app();
        let planet = {
            let world = app.world_mut();
            let e = spawn_planet(world, Vec2::new(300.0, 0.0), 12.0, 0.5);
            spawn_star(world, Vec2::ZERO);
            e
        };

        app.update(); // startup assets + spawn children
        app.update(); // LightInfo exists -> update positions/visibility

        let mut world = app.world_mut();
        assert!(world.get_entity(planet).unwrap().contains::<ShadowAttached>());

        // Find the shadow child of the planet (entity WITH ChildOf -> planet).
        let mut query = world.query::<(Entity, &ChildOf, &Visibility, &Mesh2d)>();
        let (shadow_entity, vis, mesh_handle) = query
            .iter(world)
            .find(|(_, co, _, _)| co.0 == planet)
            .map(|(e, _, v, m)| (e, *v, m.0.clone()))
            .expect("shadow child exists");
        assert!(world.get_entity(shadow_entity).is_ok());
        assert_eq!(vis, Visibility::Visible, "lit planet shadow is visible");

        // The mesh carries 4 position vertices (the cone quad).
        let mesh = world.resource::<Assets<Mesh>>().get(&mesh_handle).unwrap();
        assert_eq!(mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().len(), 4);
    }

    #[test]
    fn unlit_planet_shadow_hidden() {
        let mut app = test_app();
        let planet = {
            let world = app.world_mut();
            let e = spawn_planet(world, Vec2::new(300.0, 0.0), 12.0, 0.0); // intensity 0
            spawn_star(world, Vec2::ZERO);
            e
        };

        app.update();
        app.update();

        let mut world = app.world_mut();
        let mut found = false;
        let mut query = world.query::<(&ChildOf, &Visibility)>();
        for (co, vis) in query.iter(world) {
            if co.0 == planet {
                assert_eq!(*vis, Visibility::Hidden);
                found = true;
            }
        }
        assert!(found, "shadow child exists for unlit planet");
    }

    #[test]
    fn star_gets_glow_but_no_shadow() {
        let mut app = test_app();
        let star = {
            let world = app.world_mut();
            spawn_star(world, Vec2::ZERO)
        };

        app.update();
        app.update();

        let mut world = app.world_mut();
        // Star has the glow marker (glow child spawned).
        assert!(world.get_entity(star).unwrap().contains::<StarGlow>());
        // Star never gets ShadowAttached (no shadow child).
        assert!(!world.get_entity(star).unwrap().contains::<ShadowAttached>());
        // And none of the star's children is a shadow cone.
        let mut query = world.query::<(Entity, &ChildOf, &ShadowCone)>();
        let has_shadow_child = query.iter(world).any(|(_, co, _)| co.0 == star);
        assert!(!has_shadow_child, "star children must not include shadow cones");
    }

    #[test]
    fn tiny_body_casts_no_shadow() {
        let mut app = test_app();
        let planet = {
            let world = app.world_mut();
            let e = spawn_planet(world, Vec2::new(100.0, 80.0), 1.0, 0.5); // below SHADOW_MIN_RADIUS
            spawn_star(world, Vec2::ZERO);
            e
        };

        app.update();
        app.update();

        let mut world = app.world_mut();
        let mut found = false;
        let mut query = world.query::<(&ChildOf, &Visibility)>();
        for (co, vis) in query.iter(world) {
            if co.0 == planet {
                assert_eq!(*vis, Visibility::Hidden);
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn rotated_body_cone_still_points_away_from_star_in_world() {
        // A collision can leave the body's Transform rotated (Avian). The cone
        // vertices live in the body's local frame (the mesh is a child), so the
        // system must compensate: rotate the world light direction by rot⁻¹
        // before building the cone. Star at origin, planet at +X -> the cone
        // must point +X in WORLD space even with the body rotated 90°.
        let rot = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mut app = test_app();
        let planet = {
            let world = app.world_mut();
            let e = world
                .spawn((
                    CelestialBody {
                        name: "Planet".into(),
                        body_type: BodyType::Planet,
                        mass: 100.0,
                        radius: 12.0,
                        color: [0.3, 0.6, 1.0],
                        luminous: false,
                    },
                    Transform::from_xyz(300.0, 0.0, 0.0).with_rotation(rot),
                    LightInfo {
                        direction: Vec2::NEG_X, // toward star at origin
                        intensity: 0.5,
                        distance_to_star: 300.0,
                        light_pos: Vec2::ZERO,
                        light_color: Vec3::new(1.0, 0.9, 0.3),
                        star_intensity: 1.0,
                        falloff: 0.0001,
                    },
                ))
                .id();
            spawn_star(world, Vec2::ZERO);
            e
        };

        app.update();
        app.update();

        let mut world = app.world_mut();
        let mut query = world.query::<(&ChildOf, &Visibility, &Mesh2d)>();
        let mesh_handle = query
            .iter(world)
            .find(|(co, _, _)| co.0 == planet)
            .map(|(_, _, m)| m.0.clone())
            .expect("shadow child exists");
        assert_eq!(
            query.iter(world).find(|(co, _, _)| co.0 == planet).unwrap().1,
            &Visibility::Visible
        );

        // Read the cone vertices (body-local), rotate them into world space.
        let mesh = world.resource::<Assets<Mesh>>().get(&mesh_handle).unwrap();
        let bevy::render::mesh::VertexAttributeValues::Float32x3(pos) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("expected Float32x3 positions");
        };
        let verts: Vec<Vec3> = pos.iter().map(|v| rot * Vec3::from_array(*v)).collect();
        // Cone base = verts[0..2] (two points on the body), far end = verts[2..4].
        // The cone must extend toward +X (away from the star at origin).
        let base_mid = (verts[0] + verts[1]) / 2.0;
        let far_mid = (verts[2] + verts[3]) / 2.0;
        let dir = (far_mid - base_mid).truncate().normalize();
        assert!(
            dir.dot(Vec2::X) > 0.99,
            "cone must point +X in world, got dir {dir:?} (rotated body bug)"
        );
        // Base still spans the body diameter (2r) in world space.
        let base_width = verts[0].truncate().distance(verts[1].truncate());
        assert!((base_width - 24.0).abs() < 1e-3, "base {base_width}");
    }
}
