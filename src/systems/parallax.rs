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
    /// Screen-constant star radius range (world units at zoom 1;
    /// `update_parallax` counter-scales by the zoom so stars keep
    /// the same pixel size at any zoom).
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
    // Layer 1 (sfondo) — 500 small gray stars, glued to the camera
    // (factor 1.0: they barely move on screen = very far away).
    LayerConfig {
        count: 500,
        radius_min: 0.5,
        radius_max: 1.5,
        factor: 1.0,
        z: -500_000.0,
    },
    // Layer 2 (medio) — 200 warm/cool stars, slight drift.
    LayerConfig {
        count: 200,
        radius_min: 1.0,
        radius_max: 3.0,
        factor: 0.95,
        z: -499_000.0,
    },
    // Layer 3 (primo piano) — 50 big white/yellow stars, drifts slightly more.
    LayerConfig {
        count: 50,
        radius_min: 2.0,
        radius_max: 4.0,
        factor: 0.9,
        z: -498_000.0,
    },
];

/// Biggest star radius across layers (screen px at zoom 1).
const MAX_STAR_RADIUS: f32 = 4.0;
/// Extra wrap margin beyond the viewport (world units at zoom 1),
/// so stars pop in/out off-screen instead of at the edge.
const WRAP_MARGIN: f32 = 8.0;

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

        for _ in 0..cfg.count {
            let uv = Vec2::new(rng.gen_range(0.0..1.0), rng.gen_range(0.0..1.0));
            let radius = rng.gen_range(cfg.radius_min..cfg.radius_max);

            let color = star_color(&mut rng, layer_idx);

            // Mesh radius is baked at zoom-1 size; `update_parallax`
            // counter-scales the transform by the zoom so the on-screen
            // size stays constant.
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

/// Fixed half-size (world units) of the wrap box in the normal zoom
/// range. Inside this range star world positions are zoom-independent,
/// so zooming spreads/gathers stars on screen exactly like the rest of
/// the scene (screen_px = world/zoom) — while `update_parallax` keeps
/// each star's on-screen SIZE constant via counter-scaling.
const FIXED_HALF: f32 = 1000.0;

/// World-space size of the wrap box on one axis: a fixed minimum
/// (`FIXED_HALF`, the regime where zoom behaves like the scene) grown
/// only as far as needed to cover the viewport at extreme zoom-out,
/// plus a zoom-scaled margin so the wrap seam stays off-screen.
fn wrap_box_axis(viewport_px: f32, zoom: f32) -> (f32, f32) {
    let half = FIXED_HALF.max(viewport_px * 0.5 * zoom + (MAX_STAR_RADIUS + WRAP_MARGIN) * zoom);
    (half * 2.0, half)
}

/// Each frame, place every star in a wrap box centered on the camera,
/// drifting with the layer factor:
///
///   world  = cam + wrap(uv·size − cam·(1 − factor)·zoom) (box-wrapped)
///   scale  = zoom (constant on-screen star size)
///
/// The box is zoom-independent in the normal range, so zooming spreads
/// / gathers stars on screen like the rest of the scene (screen_px =
/// world/zoom) — but each star keeps the same pixel size. The drift
/// term carries `·zoom` so zooming never drags stars toward/away from
/// the SCENE origin (v0.14.116): the field breathes around the CAMERA
/// center. Past the fixed box the wrap grows with the viewport instead
/// of leaving edge voids.
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
    crate::mark_system("update_parallax");

    let Ok((camera, projection)) = cameras.single() else {
        return;
    };
    let cam = camera.translation;
    let zoom = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let (box_w, half_w) = wrap_box_axis(window.width(), zoom);
    let (box_h, half_h) = wrap_box_axis(window.height(), zoom);

    for (mut transform, star) in stars.iter_mut() {
        let drift = 1.0 - star.factor;
        let sx = wrap_coord(star.uv.x * box_w - cam.x * drift * zoom, half_w);
        let sy = wrap_coord(star.uv.y * box_h - cam.y * drift * zoom, half_h);
        // Parent sits at origin in x/y (only z depth), so local == world − z.
        transform.translation.x = cam.x + sx;
        transform.translation.y = cam.y + sy;
        // Counter-scale the baked mesh radius: world radius r·zoom
        // renders at r·zoom/zoom = r screen px at any zoom.
        transform.scale = Vec3::splat(zoom);
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
    fn wrap_box_covers_viewport_at_any_zoom() {
        // Box must always exceed the visible extent (viewport·zoom).
        for (vp_px, zoom) in [(390.0, 0.1), (390.0, 1.0), (1920.0, 1.0), (800.0, 50.0)] {
            let (size, half) = wrap_box_axis(vp_px, zoom);
            assert!(
                size > vp_px * zoom,
                "box {size} < viewport {}",
                vp_px * zoom
            );
            assert!((half * 2.0 - size).abs() < 1e-3);
        }
        // Normal range: fixed box (zoom-independent world positions →
        // the background zooms like the scene).
        let (a, _) = wrap_box_axis(390.0, 0.5);
        let (b, _) = wrap_box_axis(390.0, 1.0);
        assert!(
            (a - b).abs() < 1e-3,
            "box should be fixed in normal range: {a} vs {b}"
        );
        assert!((a - FIXED_HALF * 2.0).abs() < 1e-3);
        // Extreme zoom-out: box grows with the viewport (no edge voids).
        let (c, _) = wrap_box_axis(800.0, 20.0);
        let (d, _) = wrap_box_axis(800.0, 40.0);
        assert!(
            (d - c * 2.0).abs() < 1e-3,
            "grown box must scale with zoom: {c} vs {d}"
        );
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

    /// Zooming spreads/gathers stars on screen like the rest of the
    /// scene (but star SIZE stays constant via counter-scaling): the
    /// screen gap between two stars scales as 1/zoom.
    #[test]
    fn zoom_spreads_stars_on_screen() {
        // Mirror of `update_parallax`: screen_px = wrapped/zoom.
        fn screen_px(uv: f32, cam: f32, factor: f32, vp_px: f32, zoom: f32) -> f32 {
            let (size, half) = wrap_box_axis(vp_px, zoom);
            wrap_coord(uv * size - cam * (1.0 - factor) * zoom, half) / zoom
        }
        // Centered camera: no wrap interference, gap ratio is exact.
        for factor in [1.0, 0.95, 0.9] {
            let a0 = screen_px(0.30, 0.0, factor, 390.0, 0.5);
            let b0 = screen_px(0.35, 0.0, factor, 390.0, 0.5);
            let a1 = screen_px(0.30, 0.0, factor, 390.0, 2.0);
            let b1 = screen_px(0.35, 0.0, factor, 390.0, 2.0);
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
