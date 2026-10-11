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
/// - 0.7 = drifts most (foreground of the background, "closer")
///
/// Small/far stars use the highest factor, big/near stars the lowest.
#[derive(Component)]
pub struct ParallaxLayer {
    pub factor: f32,
}

/// Per-star data: `uv` is the star's position as a fraction of the
/// wrap box ([0,1) on each axis). The real position is recomputed
/// every frame by `update_parallax` so the starfield wraps around
/// the camera (infinite field, constant density).
#[derive(Component)]
pub struct Star {
    pub uv: Vec2,
    pub factor: f32,
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
    /// Star radius range in world units (zoom scales on-screen size
    /// like the rest of the scene).
    radius_min: f32,
    radius_max: f32,
    factor: f32,
    z: f32,
}

// NOTE on z: `sync_sprite_z` (firefly_bridge) writes planet sprites at
// `z = -distance from the star`, so planets routinely sit at z < -100.
// The starfield must live far behind any plausible planet: -500k is
// inside the custom far plane (±1e6, see `main_camera_projection`).
const LAYERS: [LayerConfig; 3] = [
    // Layer 1 (sfondo) — 12500 small gray stars, glued to the camera
    // (factor 1.0: they barely move on screen = very far away).
    // Density preserved from the 2000×2000 box (500 stars): 25x area → 25x count.
    LayerConfig {
        count: 12500,
        radius_min: 0.5,
        radius_max: 1.5,
        factor: 1.0,
        z: -500_000.0,
    },
    // Layer 2 (medio) — 5000 warm/cool stars, slight drift.
    LayerConfig {
        count: 5000,
        radius_min: 1.0,
        radius_max: 3.0,
        factor: 0.95,
        z: -499_000.0,
    },
    // Layer 3 (primo piano) — 1250 big white/yellow stars, drifts slightly more.
    LayerConfig {
        count: 1250,
        radius_min: 2.0,
        radius_max: 4.0,
        factor: 0.9,
        z: -498_000.0,
    },
];

/// Asset sharing: with ~19k stars, one mesh + one material per star
/// would mean ~38k GPU assets. Instead each layer pre-builds a small
/// palette of quantized meshes (radius steps) and materials (color
/// variants); stars share handles, so batching still works.
const RADIUS_STEPS: usize = 8;
const COLOR_VARIANTS: usize = 16;

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

        // Pre-build shared asset palettes for this layer: quantized
        // radii × color variants (see RADIUS_STEPS/COLOR_VARIANTS).
        let mut mesh_palette = Vec::with_capacity(RADIUS_STEPS);
        for s in 0..RADIUS_STEPS {
            let t = if RADIUS_STEPS > 1 {
                s as f32 / (RADIUS_STEPS - 1) as f32
            } else {
                0.0
            };
            let radius = cfg.radius_min + (cfg.radius_max - cfg.radius_min) * t;
            mesh_palette.push(meshes.add(Circle::new(radius)));
        }
        let mut mat_palette = Vec::with_capacity(COLOR_VARIANTS);
        for _ in 0..COLOR_VARIANTS {
            mat_palette
                .push(materials.add(ColorMaterial::from_color(star_color(&mut rng, layer_idx))));
        }

        for _ in 0..cfg.count {
            let uv = Vec2::new(rng.gen_range(0.0..1.0), rng.gen_range(0.0..1.0));

            // Share assets: pick a pre-built mesh + material at random.
            // Meshes keep world size: zoom scales on-screen size like
            // everything else in the scene.
            let mesh = mesh_palette[rng.gen_range(0..RADIUS_STEPS)].clone();
            let material = mat_palette[rng.gen_range(0..COLOR_VARIANTS)].clone();

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

/// Fixed half-size (world units) of the wrap box. Star world positions
/// and sizes are zoom-independent, so zooming spreads/gathers stars on
/// screen AND scales their size exactly like the rest of the scene.
/// Absolute size: zooming far out may leave sky regions empty (the
/// field is finite), like any distant object.
const FIXED_HALF: f32 = 5000.0;

/// World-space size of the wrap box on one axis: constant.
fn wrap_box_axis() -> (f32, f32) {
    (FIXED_HALF * 2.0, FIXED_HALF)
}

/// Each frame, place every star in a wrap box centered on the camera,
/// drifting with the layer factor:
///
///   world  = cam + wrap(uv·size − cam·(1 − factor)·zoom) (box-wrapped)
///   scale  = 1 (star size zooms with the scene like everything else)
///
/// The box is a constant world size: the starfield behaves like a
/// distant object with absolute size (zooming far out may show empty
/// sky past its edge). The drift term carries `·zoom` so zooming never
/// drags stars toward/away from the SCENE origin (v0.14.116): the field
/// breathes around the CAMERA center.
///
/// - factor 1.0 (small/far stars) → drift 0: glued to the camera,
///   they barely move = "already very far away".
/// - factor 0.9 (big/near stars) → drift 0.1: they sweep past faster.
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
    let _ = windows;
    crate::mark_system("update_parallax");

    let Ok((camera, projection)) = cameras.single() else {
        return;
    };
    let cam = camera.translation;
    let zoom = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let (box_w, half_w) = wrap_box_axis();
    let (box_h, half_h) = wrap_box_axis();

    for (mut transform, star) in stars.iter_mut() {
        let drift = 1.0 - star.factor;
        let sx = wrap_coord(star.uv.x * box_w - cam.x * drift * zoom, half_w);
        let sy = wrap_coord(star.uv.y * box_h - cam.y * drift * zoom, half_h);
        // Parent sits at origin in x/y (only z depth), so local == world − z.
        // No counter-scaling: star meshes keep world size, so zoom scales
        // their on-screen size like everything else in the scene.
        transform.translation.x = cam.x + sx;
        transform.translation.y = cam.y + sy;
        transform.scale = Vec3::ONE;
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
        // screen = uv·size − cam·(1−factor).
        // L1 (factor 1.0): drift 0 → camera-independent (glued).
        // L2 (factor 0.95): drift 0.05 → slight drift.
        // L3 (factor 0.9): drift 0.1 → drifts slightly more.
        let size = 1600.0;
        let half = 800.0;
        let uv = 0.3;
        let s1a = wrap_coord(uv * size - 50.0 * (1.0 - 1.0), half);
        let s1b = wrap_coord(uv * size - 200.0 * (1.0 - 1.0), half);
        assert!((s1a - s1b).abs() < 1e-4, "far layer moved: {s1a} vs {s1b}");
        let s2a = wrap_coord(uv * size - 50.0 * (1.0 - 0.95), half);
        let s2b = wrap_coord(uv * size - 200.0 * (1.0 - 0.95), half);
        assert!(
            (s2a - s2b).abs() > 1.0,
            "mid layer should drift slightly: {s2a} vs {s2b}"
        );
        let s3a = wrap_coord(uv * size - 50.0 * (1.0 - 0.9), half);
        let s3b = wrap_coord(uv * size - 200.0 * (1.0 - 0.9), half);
        assert!(
            (s3a - s3b).abs() > (s2a - s2b).abs(),
            "L3 should drift more than L2: {s3a} vs {s3b}"
        );
    }

    #[test]
    fn density_preserved_after_box_growth() {
        // Box grew 2000×2000 → 10000×10000 (25x area); counts scaled
        // 25x (500/200/50 → 12500/5000/1250) to keep density identical.
        let old_area = 2000.0 * 2000.0;
        let new_area = FIXED_HALF * 2.0 * (FIXED_HALF * 2.0);
        assert!((new_area / old_area - 25.0).abs() < 1e-6);
        let old_counts = [500.0, 200.0, 50.0];
        for (i, cfg) in LAYERS.iter().enumerate() {
            let old_density = old_counts[i] / old_area;
            let new_density = cfg.count as f32 / new_area;
            assert!(
                (old_density - new_density).abs() < 1e-9,
                "layer {i}: density changed ({} vs {})",
                old_density,
                new_density
            );
        }
    }
    #[test]
    fn wrap_box_is_constant_absolute_size() {
        // The starfield is a constant world size: same box at any zoom —
        // zooming far out may reveal empty sky past its edge.
        let (a, ha) = wrap_box_axis();
        assert!((a - FIXED_HALF * 2.0).abs() < 1e-3);
        assert!((ha * 2.0 - a).abs() < 1e-3);
        // Sanity: the fixed box no longer depends on viewport/zoom args.
    }

    #[test]
    fn starfield_sits_behind_planets() {
        // Planets sit at z = -distance (sync_sprite_z); starfield must
        // be far behind any plausible planet distance.
        for cfg in &LAYERS {
            assert!(cfg.z < -100_000.0, "layer z {} not behind planets", cfg.z);
        }
        assert!(LAYERS[0].z < LAYERS[1].z && LAYERS[1].z < LAYERS[2].z);
    }

    #[test]
    fn factors_span_one_to_point_nine() {
        assert!((LAYERS[0].factor - 1.0).abs() < 1e-6);
        assert!((LAYERS[1].factor - 0.95).abs() < 1e-6);
        assert!((LAYERS[2].factor - 0.9).abs() < 1e-6);
        assert!(LAYERS[0].factor > LAYERS[1].factor && LAYERS[1].factor > LAYERS[2].factor);
    }

    /// Zooming spreads/gathers stars on screen AND scales their size like
    /// the rest of the scene: the screen gap between two stars scales as
    /// 1/zoom, and so does each star's on-screen radius.
    #[test]
    fn zoom_spreads_stars_on_screen() {
        // Mirror of `update_parallax`: screen_px = wrapped/zoom.
        fn screen_px(uv: f32, cam: f32, factor: f32, zoom: f32) -> f32 {
            let (size, half) = wrap_box_axis();
            wrap_coord(uv * size - cam * (1.0 - factor) * zoom, half) / zoom
        }
        // Centered camera: no wrap interference, gap ratio is exact.
        for factor in [1.0, 0.95, 0.9] {
            let a0 = screen_px(0.30, 0.0, factor, 0.5);
            let b0 = screen_px(0.35, 0.0, factor, 0.5);
            let a1 = screen_px(0.30, 0.0, factor, 2.0);
            let b1 = screen_px(0.35, 0.0, factor, 2.0);
            let gap_out = (b0 - a0).abs();
            let gap_in = (b1 - a1).abs();
            // Zoom 0.5 -> 2.0 is 4x closer: gap must shrink ~4x.
            let ratio = gap_out / gap_in;
            assert!(
                (ratio - 4.0).abs() < 1e-3,
                "factor {factor}: gap ratio {ratio}, expected ~4"
            );
        }
    }
}
