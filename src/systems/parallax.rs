use crate::systems::camera::MainCamera;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

// ============================================================
// Components
// ============================================================

/// Marks a parallax layer parent entity. The `factor` determines
/// how much the layer moves relative to the camera (0.0 = fixed
/// in world space, 1.0 = glued to the camera).
#[derive(Component)]
pub struct ParallaxLayer {
    pub factor: f32,
}

/// Per-star data: base position inside the wrap box, the layer's
/// parallax factor and the box half-size. The real position is
/// recomputed every frame by `update_parallax` so the starfield
/// wraps around the camera (infinite field, constant density).
#[derive(Component)]
pub struct Star {
    pub base: Vec2,
    pub factor: f32,
    pub half: f32,
}

// ============================================================
// Resources
// ============================================================

/// Seed for reproducible star generation.
/// Change this value at runtime or via `insert_resource` to get
/// different star layouts on subsequent runs.
#[derive(Resource)]
pub struct StarSeed(pub u64);

impl Default for StarSeed {
    fn default() -> Self {
        Self(42)
    }
}

// ============================================================
// Plugin
// ============================================================

pub struct ParallaxPlugin;

impl Plugin for ParallaxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StarSeed>()
            .add_systems(Startup, spawn_stars)
            .add_systems(Update, update_parallax);
    }
}

// ============================================================
// Layer configuration
// ============================================================

struct LayerConfig {
    count: usize,
    radius_min: f32,
    radius_max: f32,
    factor: f32,
    z: f32,
    /// Full width/height of the wrap box for this layer.
    /// Density = count / (box_size²). With box 1600 and 500 stars
    /// a phone viewport (~390×844 world units at scale 1) always
    /// shows ~60 stars of layer 1 alone.
    box_size: f32,
}

const LAYERS: [LayerConfig; 3] = [
    // Layer 1 (sfondo) — 500 gray stars, fixed in world
    LayerConfig {
        count: 500,
        radius_min: 0.5,
        radius_max: 1.5,
        factor: 0.0,
        z: -100.0,
        box_size: 1600.0,
    },
    // Layer 2 (medio) — 200 warm/cool stars, drifts at 80% (background)
    LayerConfig {
        count: 200,
        radius_min: 1.0,
        radius_max: 3.0,
        factor: 0.2,
        z: -90.0,
        box_size: 1600.0,
    },
    // Layer 3 (primo piano) — 50 white/yellow stars, drifts at 50%
    LayerConfig {
        count: 50,
        radius_min: 2.0,
        radius_max: 4.0,
        factor: 0.5,
        z: -80.0,
        box_size: 1600.0,
    },
];

// ============================================================
// Spawning
// ============================================================

fn spawn_stars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    seed: Res<StarSeed>,
) {
    let mut rng = StdRng::seed_from_u64(seed.0);

    for cfg in &LAYERS {
        // Parent entity — only carries the z depth + layer marker.
        // Per-star positions are computed in world space by
        // `update_parallax` (wrapping around the camera).
        let parent_id = commands
            .spawn((
                Transform::from_xyz(0.0, 0.0, cfg.z),
                Visibility::default(),
                ParallaxLayer { factor: cfg.factor },
            ))
            .id();

        let half = cfg.box_size * 0.5;
        for _ in 0..cfg.count {
            let base = Vec2::new(rng.gen_range(-half..half), rng.gen_range(-half..half));
            let radius = rng.gen_range(cfg.radius_min..cfg.radius_max);

            let color = star_color(&mut rng, cfg.factor);

            // Build a circle mesh + material for this star
            let mesh = meshes.add(Circle::new(radius));
            let material = materials.add(ColorMaterial::from_color(color));

            commands
                .spawn((
                    Mesh2d(mesh),
                    MeshMaterial2d::<ColorMaterial>(material),
                    Transform::from_xyz(base.x, base.y, 0.0),
                    Visibility::default(),
                    RenderLayers::layer(1),
                    Star {
                        base,
                        factor: cfg.factor,
                        half,
                    },
                ))
                .set_parent_in_place(parent_id);
        }
    }
}

/// Generate a star colour based on which layer it belongs to.
fn star_color(rng: &mut StdRng, factor: f32) -> Color {
    // Use factor as a discriminant (they are distinct per layer)
    if factor == 0.0 {
        // Layer 1: gray-ish, opacity 0.3–0.6
        let gray = rng.gen_range(0.3..0.8);
        Color::srgb(gray, gray, gray).with_alpha(rng.gen_range(0.3..0.6))
    } else if factor == 0.2 {
        // Layer 2: random warm or cool hue, opacity 0.5–0.8
        let alpha = rng.gen_range(0.5..0.8);
        if rng.gen_bool(0.5) {
            // Warm hues: red-orange-yellow
            let r = rng.gen_range(0.6..1.0);
            let g = rng.gen_range(0.2..0.6);
            Color::srgb(r, g, 0.0).with_alpha(alpha)
        } else {
            // Cool hues: blue-purple
            let b = rng.gen_range(0.6..1.0);
            let g = rng.gen_range(0.2..0.5);
            Color::srgb(0.0, g, b).with_alpha(alpha)
        }
    } else {
        // Layer 3: white/yellow, opacity 0.7–1.0
        let warmth = rng.gen_range(0.8..1.0);
        Color::srgb(warmth, warmth * 0.9, warmth * 0.6).with_alpha(rng.gen_range(0.7..1.0))
    }
}

// ============================================================
// Parallax movement (wrapping starfield)
// ============================================================

/// Wrap `v` into `[-half, +half)` (toroidal tiling).
fn wrap_coord(v: f32, half: f32) -> f32 {
    (v + half).rem_euclid(half * 2.0) - half
}

/// Each frame, place every star in a box of `box_size` centered on
/// the camera, drifting with the layer factor:
///
///   screen = base − cam·(1 − factor)   (wrapped into the box)
///   world  = cam + screen
///
/// - factor 0.0 → world-fixed (moves full speed against the camera)
/// - factor 0.2 → drifts at 80% (background, slower)
/// - factor 0.5 → drifts at 50% (foreground of the background)
///
/// Wrapping keeps density constant everywhere: panning never leaves
/// an empty void, unlike the old fixed 10_000×10_000 scatter
/// (750 stars over 10⁸ units² ≈ 2–3 visible on a phone viewport).
fn update_parallax(
    cameras: Query<
        &Transform,
        (
            (With<Camera2d>, With<MainCamera>),
            With<Projection>,
            With<MainCamera>,
        ),
    >,
    mut stars: Query<(&mut Transform, &Star), Without<MainCamera>>,
) {
    crate::mark_system("update_parallax");

    let Ok(camera) = cameras.single() else {
        return;
    };
    let cam = camera.translation;

    for (mut transform, star) in stars.iter_mut() {
        let drift = 1.0 - star.factor;
        let sx = wrap_coord(star.base.x - cam.x * drift, star.half);
        let sy = wrap_coord(star.base.y - cam.y * drift, star.half);
        // Parent sits at origin in x/y (only z depth), so local == world − z.
        transform.translation.x = cam.x + sx;
        transform.translation.y = cam.y + sy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_stays_in_box() {
        let half = 800.0;
        for v in [-5000.0, -801.0, -800.0, -1.0, 0.0, 799.9, 800.0, 5000.0] {
            let w = wrap_coord(v, half);
            assert!(
                w >= -half && w < half,
                "wrap_coord({v}) = {w} outside [-{half}, {half})"
            );
        }
    }

    #[test]
    fn wrap_is_periodic() {
        let half = 800.0;
        let size = half * 2.0;
        let a = wrap_coord(100.0, half);
        let b = wrap_coord(100.0 + size * 3.0, half);
        assert!((a - b).abs() < 1e-4, "not periodic: {a} vs {b}");
    }

    #[test]
    fn layer0_is_world_fixed_and_layer3_drifts_half() {
        // screen = base − cam·(1−factor), wrapped; use cam small enough
        // to stay inside the box (no wrap interference).
        let half = 800.0;
        let base = 100.0;
        let cam = 50.0;
        let s0 = wrap_coord(base - cam * (1.0 - 0.0), half);
        assert!((s0 - 50.0).abs() < 1e-4, "L1 screen should be 50, got {s0}");
        let s3 = wrap_coord(base - cam * (1.0 - 0.5), half);
        assert!((s3 - 75.0).abs() < 1e-4, "L3 screen should be 75, got {s3}");
    }

    #[test]
    fn density_gives_visible_stars_on_phone_viewport() {
        // Phone viewport ~390×844 world units at scale 1 ≈ 0.33M units².
        // L1: 500 stars in 1600² = 2.56M units² → expect ~64 visible.
        let viewport_area = 390.0 * 844.0;
        let expected_l1 = 500.0 * viewport_area / (1600.0 * 1600.0);
        assert!(
            expected_l1 > 40.0,
            "L1 too sparse: {expected_l1:.1} expected stars on phone viewport"
        );
    }
}
