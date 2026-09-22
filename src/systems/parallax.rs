use crate::systems::camera::MainCamera;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

// ============================================================
// Components
// ============================================================

/// Marks a parallax layer parent entity. The `factor` determines
/// how glued the layer is to the camera:
///
/// - 1.0 = glued (background: barely moves on screen, "far away")
/// - 0.3 = mostly world-fixed (foreground: drifts fast, "close")
///
/// Small/far stars use the highest factor, big/near stars the lowest.
#[derive(Component)]
pub struct ParallaxLayer {
    pub factor: f32,
}

/// Per-star data: `uv` is the star's position as a fraction of the
/// wrap box ([0,1) on each axis), so density stays uniform whatever
/// the effective box size is. The real position is recomputed every
/// frame by `update_parallax` so the starfield wraps around the
/// camera (infinite field, constant density).
#[derive(Component)]
pub struct Star {
    pub uv: Vec2,
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
    /// Full width/height of the wrap box for this layer at zoom 1.
    /// Density = count / (box_size²). With box 1600 and 500 stars
    /// a phone viewport (~390×844 world units at scale 1) always
    /// shows ~60 stars of layer 1 alone.
    box_size: f32,
}

const LAYERS: [LayerConfig; 3] = [
    // Layer 1 (sfondo) — 500 small gray stars, glued to the camera
    // (factor 1.0: they barely move on screen = very far away).
    LayerConfig {
        count: 500,
        radius_min: 0.5,
        radius_max: 1.5,
        factor: 1.0,
        z: -100.0,
        box_size: 1600.0,
    },
    // Layer 2 (medio) — 200 warm/cool stars, mid drift.
    LayerConfig {
        count: 200,
        radius_min: 1.0,
        radius_max: 3.0,
        factor: 0.65,
        z: -90.0,
        box_size: 1600.0,
    },
    // Layer 3 (primo piano) — 50 big white/yellow stars, drifts most.
    LayerConfig {
        count: 50,
        radius_min: 2.0,
        radius_max: 4.0,
        factor: 0.3,
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

    for (layer_idx, cfg) in LAYERS.iter().enumerate() {
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
            let uv = Vec2::new(rng.gen_range(0.0..1.0), rng.gen_range(0.0..1.0));
            let radius = rng.gen_range(cfg.radius_min..cfg.radius_max);

            let color = star_color(&mut rng, layer_idx);

            // Build a circle mesh + material for this star
            let mesh = meshes.add(Circle::new(radius));
            let material = materials.add(ColorMaterial::from_color(color));

            commands
                .spawn((
                    Mesh2d(mesh),
                    MeshMaterial2d::<ColorMaterial>(material),
                    Transform::from_xyz(0.0, 0.0, 0.0),
                    Visibility::default(),
                    RenderLayers::layer(1),
                    Star {
                        uv,
                        factor: cfg.factor,
                        half,
                    },
                ))
                .set_parent_in_place(parent_id);
        }
    }
}

/// Generate a star colour based on which layer it belongs to
/// (0 = background, 1 = mid, 2 = foreground).
fn star_color(rng: &mut StdRng, layer: usize) -> Color {
    if layer == 0 {
        // Layer 1: gray-ish, opacity 0.3–0.6
        let gray = rng.gen_range(0.3..0.8);
        Color::srgb(gray, gray, gray).with_alpha(rng.gen_range(0.3..0.6))
    } else if layer == 1 {
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

/// Zoom-aware half-size of the wrap box: the designed box scaled by
/// the camera zoom, but never smaller than the actual viewport
/// (plus a margin for the biggest star radius). This keeps the sky
/// fully covered at any zoom — zooming out widens the box instead
/// of leaving voids at the edges.
fn effective_half(base_half: f32, zoom_scale: f32, viewport_half_max: f32) -> f32 {
    (base_half * zoom_scale).max(viewport_half_max + 4.0)
}

/// Each frame, place every star in a box centered on the camera,
/// drifting with the layer factor:
///
///   screen = uv·size − cam·(1 − factor)   (wrapped into the box)
///   world  = cam + screen
///
/// - factor 1.0 (small/far stars) → screen = uv·size: glued to the
///   camera, they barely move = "already very far away".
/// - factor 0.3 (big/near stars) → drift 0.7: they sweep past fast.
///
/// Positions are stored as UV fractions so density stays uniform at
/// any effective box size (zoom-independent). Wrapping keeps density
/// constant everywhere: panning never leaves an empty void.
fn update_parallax(
    cameras: Query<
        (&Transform, &Projection),
        (
            (With<Camera2d>, With<MainCamera>),
            With<Projection>,
            With<MainCamera>,
        ),
    >,
    windows: Query<&Window>,
    mut stars: Query<(&mut Transform, &Star), Without<MainCamera>>,
) {
    crate::mark_system("update_parallax");

    let Ok((camera, projection)) = cameras.single() else {
        return;
    };
    let cam = camera.translation;
    let zoom_scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let viewport_half_max = windows
        .single()
        .map(|w| (w.width().max(w.height())) * 0.5 * zoom_scale)
        .unwrap_or(800.0);

    for (mut transform, star) in stars.iter_mut() {
        let half = effective_half(star.half, zoom_scale, viewport_half_max);
        let size = half * 2.0;
        let drift = 1.0 - star.factor;
        let sx = wrap_coord(star.uv.x * size - cam.x * drift, half);
        let sy = wrap_coord(star.uv.y * size - cam.y * drift, half);
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
    fn far_layer_is_glued_and_near_layer_drifts() {
        // screen = uv·size − cam·(1−factor); uv=0.5, size=1600, cam=50.
        // L1 (factor 1.0): screen = 800 − 0 = 800 (camera-independent).
        // L3 (factor 0.3): screen = 800 − 35 = 765 (drifts with camera).
        let size = 1600.0;
        let half = 800.0;
        let uv = 0.5;
        let cam = 50.0;
        let s1 = wrap_coord(uv * size - cam * (1.0 - 1.0), half);
        assert!((s1 - 800.0).abs() < 1e-3 || (s1 + 800.0).abs() < 1e-3);
        // Moving the camera must NOT move L1 on screen...
        let s1b = wrap_coord(uv * size - 200.0 * (1.0 - 1.0), half);
        assert!((s1 - s1b).abs() < 1e-4, "far layer moved: {s1} vs {s1b}");
        // ...but MUST move L3.
        let s3a = wrap_coord(uv * size - cam * (1.0 - 0.3), half);
        let s3b = wrap_coord(uv * size - 200.0 * (1.0 - 0.3), half);
        assert!(
            (s3a - s3b).abs() > 1.0,
            "near layer should drift: {s3a} vs {s3b}"
        );
    }

    #[test]
    fn effective_half_covers_viewport_at_any_zoom() {
        // Zoomed far out: designed box (800·50) already huge — kept.
        assert!((effective_half(800.0, 50.0, 21000.0) - 40000.0).abs() < 1e-3);
        // Desktop window at zoom 1: viewport bigger than designed box
        // → viewport wins so no edge voids.
        assert!((effective_half(800.0, 1.0, 964.0) - 968.0).abs() < 1e-3);
        // Zoomed in: box shrinks with zoom, density preserved via UV.
        assert!((effective_half(800.0, 0.1, 96.0) - 100.0).abs() < 1e-3);
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
