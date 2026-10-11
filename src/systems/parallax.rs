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

/// Luminous-star glow textures. TWO shared textures for the whole field:
///
/// 1. **Core** (`star_core_pixels`): a small saturated disc that stops
///    abruptly at `CORE_EDGE` of the quad, tinted per star. Its purpose
///    is the SHADOW EXEMPTION: firefly's stencil pass (`sprite.wgsl`)
///    writes stencil ONLY where the raw texture alpha is exactly 1.0, and
///    the shadow shader exempts background stars via stencil.g (their
///    z ≈ -500k). Everything that must never dim needs to be inside this
///    saturated disc — and nothing else needs to be, because the bloom
///    below carries the visible glow.
/// 2. **Bloom** (`star_bloom_pixels`): the wide soft halo that makes the
///    star look LUMINOUS, with alpha = (1-t)² fading to 0 at the rim.
///    Its alpha is < 1 everywhere, so it writes no stencil and therefore
///    rides along with the star's own glow look — inside a planet's cone
///    it dims like any other lit-background pixel, which is correct
///    (the cone darkens the halo the shadow falls on).
///
/// Why not one texture: a single texture either makes the saturated
/// (stencil-writing) part wide — killing the glow, since the halo would
/// be squeezed into the remaining sliver — or narrow, leaving the halo
/// un-exempt and dimmed by cones. Two textures give both properties.
///
/// Flipping firefly's stencil threshold instead is NOT an option: the
/// sun's huge outer-glow quad would then write stencil g ≈ 0 and z-skip
/// every planet occluder → cones invisible near the sun.
const STAR_GLOW_SIZE: u32 = 64;
/// Fraction of the CORE quad's radius that is saturated.
const GLOW_CORE: f32 = 0.85;
/// Core quad size = star diameter × this. Saturated disc radius ≈ 0.85
/// × 1.5 × radius ≈ 1.3× the configured radius: covers the whole bright
/// dot, tight enough to leave the bloom visible around it.
const CORE_SCALE: f32 = 1.5;
/// Bloom quad size = star diameter × this (the halo extends ~2
/// diameters past the dot, like the bloom in the astrophoto reference).
const HALO_SCALE: f32 = 5.0;

fn star_core_pixels() -> Vec<u8> {
    let w = STAR_GLOW_SIZE as usize;
    let half = STAR_GLOW_SIZE as f32 / 2.0;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let dx = x as f32 + 0.5 - half;
            let dy = y as f32 + 0.5 - half;
            let d = (dx * dx + dy * dy).sqrt() / half; // 0 centre → 1 rim
            let a = if d <= GLOW_CORE {
                1.0
            } else {
                // 1px-ish antialias band so the disc edge is not jagged,
                // then exactly 0 (no stencil outside the saturated disc).
                let t = ((d - GLOW_CORE) / (1.0 - GLOW_CORE)).min(1.0);
                (1.0 - t).max(0.0)
            };
            pixels.extend_from_slice(&[255u8, 255u8, 255u8, (a * 255.0) as u8]);
        }
    }
    pixels
}

/// Soft radial bloom: alpha (1-t)² from the centre to exactly 0 at the
/// rim (never 1.0, so it writes no stencil and stays a pure visual glow).
fn star_bloom_pixels() -> Vec<u8> {
    let w = STAR_GLOW_SIZE as usize;
    let half = STAR_GLOW_SIZE as f32 / 2.0;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let dx = x as f32 + 0.5 - half;
            let dy = y as f32 + 0.5 - half;
            let d = ((dx * dx + dy * dy).sqrt() / half).min(1.0);
            let a = (1.0 - d).powf(2.0);
            pixels.extend_from_slice(&[255u8, 255u8, 255u8, (a * 255.0) as u8]);
        }
    }
    pixels
}

// ============================================================
// Spawning
// ============================================================

fn spawn_stars(mut commands: Commands, mut images: ResMut<Assets<Image>>, seed: Res<StarSeed>) {
    let mut rng = StdRng::seed_from_u64(seed.0);

    // Two shared luminous-star textures: saturated core (shadow-exempt)
    // and soft bloom (the visible glow). See their generators above.
    let core_tex = images.add(crate::rendering::textures::build_image(
        star_core_pixels(),
        STAR_GLOW_SIZE,
        STAR_GLOW_SIZE,
    ));
    let bloom_tex = images.add(crate::rendering::textures::build_image(
        star_bloom_pixels(),
        STAR_GLOW_SIZE,
        STAR_GLOW_SIZE,
    ));

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
            // Luminous dot = TWO shared sprites at the SAME spot:
            //  - bloom (wide, soft, alpha < 1): the visible glow;
            //  - core (small, saturated): the stencil-writing disc that
            //    the shadow shader exempts via stencil.g (z ≈ -500k).
            // Order matters only visually; both are tinted per star. The
            // star entity itself is the wrapper carrying `Star` (its
            // children follow it in x/y since update_parallax writes only
            // the parent transform).
            let color = star_color(&mut rng, layer_idx);

            let star_entity = commands
                .spawn((
                    Transform::from_xyz(0.0, 0.0, 0.0),
                    Visibility::default(),
                    RenderLayers::layer(1),
                    Star {
                        uv,
                        factor: cfg.factor,
                    },
                ))
                .set_parent_in_place(parent_id)
                .id();

            commands.entity(star_entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        image: bloom_tex.clone(),
                        color,
                        custom_size: Some(Vec2::splat(radius * 2.0 * HALO_SCALE)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 0.0),
                    Visibility::default(),
                    RenderLayers::layer(1),
                ));
                parent.spawn((
                    Sprite {
                        image: core_tex.clone(),
                        color,
                        custom_size: Some(Vec2::splat(radius * 2.0 * CORE_SCALE)),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 0.001),
                    Visibility::default(),
                    RenderLayers::layer(1),
                ));
            });
        }
    }
}

/// Generate a star colour based on which layer it belongs to
/// (0 = background, 1 = mid, 2 = foreground).
///
/// Colors are ALWAYS opaque (alpha 1.0): faintness is baked into dark
/// RGB instead of translucency. Rationale: firefly's stencil pass
/// (`sprite.wgsl`) samples the raw texture alpha, IGNORING the sprite
/// tint — the saturated texture core already writes stencil.a = 1 with
/// a dummy normal (no NormalMap on stars). Opaque tint keeps the main
/// pass consistent with the stencil pass, so the v0.14.132 shader gate
/// (unlit-sprite exemption) reliably recognizes star cores. On the
/// pure-black sky opaque-dark looks identical to translucent-bright.
fn star_color(rng: &mut StdRng, layer: usize) -> Color {
    if layer == 0 {
        // Layer 1: faint gray pinpricks.
        let gray = rng.gen_range(0.3..0.8);
        let dim = rng.gen_range(0.3..0.6);
        Color::srgb(gray * dim, gray * dim, gray * dim)
    } else if layer == 1 {
        // Layer 2: random warm or cool hue, mid brightness.
        let dim = rng.gen_range(0.5..0.8);
        if rng.gen_bool(0.5) {
            // Warm hues: red-orange-yellow
            let r = rng.gen_range(0.6..1.0);
            let g = rng.gen_range(0.2..0.6);
            Color::srgb(r * dim, g * dim, 0.0)
        } else {
            // Cool hues: blue-purple
            let b = rng.gen_range(0.6..1.0);
            let g = rng.gen_range(0.2..0.5);
            Color::srgb(0.0, g * dim, b * dim)
        }
    } else {
        // Layer 3: bright white/yellow.
        let warmth = rng.gen_range(0.8..1.0);
        let dim = rng.gen_range(0.7..1.0);
        Color::srgb(warmth * dim, warmth * 0.9 * dim, warmth * 0.6 * dim)
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
    fn star_core_is_saturated_with_clean_edge() {
        // The core's job: saturated where the star is bright (so firefly's
        // stencil writes and the shadow exemption applies), then 0.
        let px = star_core_pixels();
        let w = STAR_GLOW_SIZE as usize;
        let half = STAR_GLOW_SIZE as f32 / 2.0;
        let center = px[(w / 2 * w + w / 2) * 4 + 3];
        assert_eq!(center, 255, "core centre must be fully opaque");
        let corner = px[3];
        assert_eq!(corner, 0, "core must be 0 outside the disc");
        // Saturated well inside GLOW_CORE, and monotonic to the rim.
        let inner_x = w / 2 + (half * GLOW_CORE * 0.8) as usize;
        assert_eq!(
            px[(w / 2 * w + inner_x) * 4 + 3],
            255,
            "core must stay saturated out to 80% of GLOW_CORE"
        );
        let row = w / 2;
        let mut prev = 255u8;
        for x in (w / 2)..w {
            let a = px[(row * w + x) * 4 + 3];
            assert!(a <= prev, "core edge must be monotonic (x={x})");
            prev = a;
        }
    }

    #[test]
    fn star_bloom_is_soft_and_never_saturated() {
        // The bloom is the visible glow: it must fade smoothly and NEVER
        // reach alpha 1.0, otherwise it would write stencil and the sun's
        // huge glow quad would z-skip planet occluders (cones vanish).
        let px = star_bloom_pixels();
        let w = STAR_GLOW_SIZE as usize;
        let center = px[(w / 2 * w + w / 2) * 4 + 3];
        assert!(center < 255, "bloom must never be fully opaque");
        assert!(center > 200, "bloom centre must still be nearly saturated");
        assert_eq!(px[3], 0, "bloom must fade to exactly 0 at the rim");
        let row = w / 2;
        let mut prev = center;
        for x in (w / 2)..w {
            let a = px[(row * w + x) * 4 + 3];
            assert!(a <= prev, "bloom must fall off monotonically (x={x})");
            prev = a;
        }
        // The bloom is visible around the core: at the core edge (0.85 of
        // the SQUARE) the bloom still has meaningful alpha.
        let edge_x = w / 2 + (STAR_GLOW_SIZE as f32 / 2.0 * GLOW_CORE) as usize;
        let at_edge = px[(row * w + edge_x) * 4 + 3];
        assert!(at_edge > 4, "bloom must still glow at the core edge");
    }
    /// Star colors are opaque with baked-in dimming (v0.14.132): alpha
    /// must stay 1.0 so the main pass matches firefly's stencil pass
    /// (which ignores tint), and L3 must stay brighter than L1.
    #[test]
    fn star_colors_are_opaque_with_baked_brightness() {
        let mut rng = StdRng::seed_from_u64(42);
        for layer in 0..3 {
            for _ in 0..50 {
                let c = star_color(&mut rng, layer);
                let s = c.to_srgba();
                assert!(
                    (s.alpha - 1.0).abs() < 1e-6,
                    "layer {layer}: star tint must be opaque (alpha={})",
                    s.alpha
                );
            }
        }
        // Brightness ordering across layers (sample means).
        let mut rng = StdRng::seed_from_u64(7);
        let mean = |layer: usize, rng: &mut StdRng| -> f32 {
            let mut acc = 0.0;
            for _ in 0..200 {
                let s = star_color(rng, layer).to_srgba();
                acc += s.red + s.green + s.blue;
            }
            acc / 200.0
        };
        let m0 = mean(0, &mut rng);
        let m2 = mean(2, &mut rng);
        assert!(
            m2 > m0 * 2.0,
            "L3 should be much brighter than L1 ({m2} vs {m0})"
        );
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
