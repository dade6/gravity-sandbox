//! Starfield — "endless" procedural sky with parallax layers.
//!
//! Architecture follows Johan Helsing's *Cargo Space* devlog #7
//! ("Generating space backgrounds"):
//!
//! 1. **Deterministic seeds** — one `StarSeed` resource, derived per layer
//!    and per grid cell by `seed_for` (his `Seed::hash_with(grid_pos)`):
//!    the same seed always yields the same sky, never `thread_rng`.
//! 2. **Streamed sectors** — the sky is a grid of `SECTOR_SIZE` cells and
//!    only the cells covering the view exist as entities (plus a margin).
//!    The field is effectively endless: fly anywhere and there are stars,
//!    without ever paying for millions of entities. A cell's content is a
//!    pure function of its coordinates, so despawning and re-spawning it
//!    later reproduces it exactly (no flicker/popping on pan).
//! 3. **Varied sizes** — inside every sector most stars are faint
//!    pinpricks and a few are much bigger *and* brighter, like the
//!    reference astrophoto (a couple of bright stars among many faint).
//! 4. **Parallax layers** — factors 1.0 / 0.95 / 0.9: 1.0 is glued to the
//!    camera (= infinitely far), 0.9 drifts = nearer.
//! 5. **Weenies** — the near layer occasionally carries a distant galaxy:
//!    a dim landmark to navigate by (his navigation chapter).
//!
//! Everything lives in a per-layer *base space* that is independent of the
//! camera. The visible window in base space is `cam·drift ± half_view`
//! (drift = 1 − factor) and a sector's world transform is
//! `origin + cam·factor`, so a star at base position `b` renders at
//! `world = b + cam·factor` — it slides at `(1 − factor)` relative to the
//! camera. Star positions and sizes are world units and are NOT
//! counter-scaled, so zoom scales them like the rest of the scene.

use bevy::camera::visibility::RenderLayers;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy_firefly::prelude::SpriteHeight;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::systems::camera::MainCamera;

// ============================================================
// Components
// ============================================================

/// One streamed cell of the sky. `factor` is copied from the layer config
/// so the streaming system needs no layer lookup.
#[derive(Component)]
pub struct StarSector {
    pub layer: u8,
    pub grid: IVec2,
    pub factor: f32,
}

/// Marker on a star sprite (children of a sector root).
#[derive(Component)]
pub struct Star;

/// Marker on a distant galaxy / nebula sprite (the "weenie").
#[derive(Component)]
pub struct Galaxy;

// ============================================================
// Resources
// ============================================================

/// Global seed of the sky. Change it (`insert_resource`) to get a
/// completely different — but still deterministic — starfield.
#[derive(Resource)]
pub struct StarSeed(pub u64);

impl Default for StarSeed {
    fn default() -> Self {
        Self(42)
    }
}

/// Shared textures for the sky (built once at startup).
#[derive(Resource)]
struct StarfieldTextures {
    /// Flat hard-edged disc: the dust (most stars).
    dot: Handle<Image>,
    /// Disc + soft halo + 4-point diffraction cross: the rare big stars.
    spike: Handle<Image>,
    galaxy: Handle<Image>,
}

/// Live sectors: `(layer, grid) -> sector root entity`. The streaming
/// system keeps this map in sync with what the camera can see.
#[derive(Resource, Default)]
pub struct StarSectors(HashMap<(u8, IVec2), Entity>);

impl StarSectors {
    /// Number of live sectors (tests / debug badge).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ============================================================
// Plugin
// ============================================================

pub struct ParallaxPlugin;

impl Plugin for ParallaxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StarSeed>()
            .init_resource::<StarSectors>()
            .add_systems(Startup, setup_starfield)
            .add_systems(Update, stream_starfield);
    }
}

// ============================================================
// Layer configuration
// ============================================================

struct LayerConfig {
    /// Stars generated in every sector of this layer (density is what
    /// matters: `stars / SECTOR_SIZE²`).
    stars: usize,
    /// Radius range of the many small stars (world units).
    radius_min: f32,
    radius_mid: f32,
    /// Radius range of the few big stars.
    radius_max: f32,
    /// Probability that a given star is one of the big ones.
    big_chance: f64,
    /// 1.0 = glued to the camera (infinitely far), lower = nearer.
    factor: f32,
    /// Render depth: must stay far behind any planet (`sync_sprite_z`
    /// writes planets at `z = -distance`). -500k is inside the custom far
    /// plane (±1e6, see `main_camera_projection`).
    z: f32,
}

/// Stars per 1000×1000 sector = 1.25e-4 / 5e-5 / 1.3e-5 per unit², i.e. the
/// density of the previous 10000×10000 box (12500 / 5000 / 1250 stars), so
/// swapping the finite box for streamed sectors does not change the look.
const LAYERS: [LayerConfig; 3] = [
    LayerConfig {
        stars: 125,
        radius_min: 0.5,
        radius_mid: 0.95,
        radius_max: 1.5,
        big_chance: 0.12,
        factor: 1.0,
        z: -500_000.0,
    },
    LayerConfig {
        stars: 50,
        radius_min: 1.0,
        radius_mid: 1.8,
        radius_max: 3.0,
        big_chance: 0.12,
        factor: 0.95,
        z: -499_000.0,
    },
    LayerConfig {
        stars: 13,
        radius_min: 2.0,
        radius_mid: 2.8,
        radius_max: 4.0,
        big_chance: 0.15,
        factor: 0.9,
        z: -498_000.0,
    },
];

/// Side of one sky sector, in world units.
const SECTOR_SIZE: f32 = 1000.0;
/// World-space margin used when *spawning* sectors: keeps the seam
/// off-screen without paying for a whole extra ring of cells.
const SPAWN_MARGIN: f32 = 250.0;
/// World-space margin used when *keeping* sectors alive. Larger than the
/// spawn margin so a cell sitting exactly on the boundary is not
/// spawned/despawned every frame while the camera jitters (hysteresis).
const KEEP_MARGIN: f32 = SECTOR_SIZE;
/// Upper bound on live sectors per layer. Only binds at extreme zoom-out:
/// 100 sectors = a 10×10 grid = 10000×10000 units of sky and ~18.8k
/// entities total (the same budget as the old finite box). Zooming out
/// further shows empty sky past that radius instead of growing forever.
const MAX_SECTORS_PER_LAYER: usize = 100;
/// Probability that a sector of the near layer also gets a galaxy.
const GALAXY_CHANCE: f64 = 0.22;
/// Galaxy size range (world units).
const GALAXY_SIZE_MIN: f32 = 60.0;
const GALAXY_SIZE_MAX: f32 = 150.0;

// ============================================================
// Textures
// ============================================================

/// Sentinella `SpriteHeight` che dice alla shader sprite di firefly (vendored)
/// "questo è uno sprite dello starfield": scrive lo stencil su tutto il quad
/// e nessuna normal map. Negativa per non collidere MAI con le altezze reali
/// (pianeti 0, disco stella 1000).
const STARFIELD_HEIGHT: f32 = -999.0;
/// Luminanza (sRGB) sotto la quale una stella NON si distingue dal cielo: lo
/// sfondo è lavato dall'alone del sole (Halo Brightness ~1), quindi una tinta
/// più scura dello sfondo fa apparire la stella come un puntino NERO. Le tinte
/// stanno tutte sopra questa soglia.
const SKY_WASH_REFERENCE: f32 = 0.9;

// ---- Texture delle stelle (look "Cargo Space" / foto di riferimento) ----
//
// Il campo è BIMODALE, come nell'articolo e nella foto di Sirio:
//  * la polvere (la grande maggioranza) è fatta di PUNTINI PIATTI: un disco
//    saturo con bordo netto, nessun alone -> legge come punto netto e piccolo;
//  * le poche stelle grandi hanno un disco saturo + alone morbido + CROCE DI
//    DIFFRAZIONE a 4 punte, come i vetri dei telescopi (e come Sirio nella
//    foto che ha ispirato il look).
// Entrambe condividono la stessa tinta per stella (`Sprite::color`) e la
// sentinella SpriteHeight, quindi valgono le stesse regole su stencil e ombre.

const STAR_TEX_SIZE: u32 = 64;
/// Quad del puntino = diametro × questo: il disco coincide con il raggio
/// dichiarato, bordo netto.
const DOT_SCALE: f32 = 1.0;
/// Il disco saturo occupa questa frazione del raggio del quad (il resto è
/// alone/croce): con SPIKE_SCALE 3.2 il nucleo resta ≈ il raggio dichiarato.
const SPIKE_CORE: f32 = 0.18;
/// Quad della stella grande = diametro × questo (spazio per alone + croce).
const SPIKE_SCALE: f32 = 3.2;
/// Esponente del falloff dell'alone (nucleo -> bordo). Qui > 1: l'alone deve
/// essere STRETTO, perché lo spazio esterno lo devono occupare le punte: un
/// alone "grasso" affoga la croce e ogni stella grossa torna un blob.
const SPIKE_BLOOM_EXP: f32 = 1.6;
/// Larghezza della croce (frazione del raggio del quad): ~1px sullo schermo
/// per le stelle grandi (quad ~25px). Più sottile sfarfalla e sparisce.
const SPIKE_WIDTH: f32 = 0.080;
/// Intensità delle punte (piene: sono la firma della stella grande).
const SPIKE_STRENGTH: f32 = 0.8;
/// Decadimento delle punte verso il bordo: LENTO (< 1) così i bracci
/// arrivano lunghi fino al bordo del quad, come nella foto di Sirio.
const SPIKE_TAPER_EXP: f32 = 0.5;

/// Puntino piatto: disco saturo con bordo netto (1px di antialiasing).
fn star_dot_pixels() -> Vec<u8> {
    let w = STAR_TEX_SIZE as usize;
    let half = STAR_TEX_SIZE as f32 / 2.0;
    let aa = 1.5 / half;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let dx = x as f32 + 0.5 - half;
            let dy = y as f32 + 0.5 - half;
            let d = (dx * dx + dy * dy).sqrt() / half;
            let a = if d <= 1.0 - aa {
                1.0
            } else {
                ((1.0 - d) / aa).clamp(0.0, 1.0)
            };
            pixels.extend_from_slice(&[255u8, 255u8, 255u8, (a * 255.0) as u8]);
        }
    }
    pixels
}

/// Stella grande: nucleo saturo + alone morbido + croce di diffrazione a 4
/// punte. Le punte sfumano verso il bordo (le "spikes" lunghe della foto),
/// e la loro ampiezza è costante in unità normalizzate, così non si assottigliano
/// all'aumentare del raggio.
fn star_spike_pixels() -> Vec<u8> {
    let w = STAR_TEX_SIZE as usize;
    let half = STAR_TEX_SIZE as f32 / 2.0;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let ax = (x as f32 + 0.5 - half) / half;
            let ay = (y as f32 + 0.5 - half) / half;
            let d = (ax * ax + ay * ay).sqrt().min(1.0);
            // Nucleo + alone.
            let bloom = if d <= SPIKE_CORE {
                1.0
            } else {
                let t = (d - SPIKE_CORE) / (1.0 - SPIKE_CORE);
                (1.0 - t).powf(SPIKE_BLOOM_EXP)
            };
            // Croce: due lobi gaussiani lungo X e Y, che si attenuano con la
            // distanza dal centro (il massimo è al centro, come nella realtà).
            // Il decadimento è LENTO (SPIKE_TAPER_EXP < 1) così le punte
            // restano più luminose dell'alone verso il bordo del quad.
            let taper = (1.0 - d).powf(SPIKE_TAPER_EXP);
            let gauss = |o: f32| (-(o * o) / (SPIKE_WIDTH * SPIKE_WIDTH)).exp();
            let spike = SPIKE_STRENGTH * taper * gauss(ay).max(gauss(ax));
            let a = bloom.max(spike).clamp(0.0, 1.0);
            pixels.extend_from_slice(&[255u8, 255u8, 255u8, (a * 255.0) as u8]);
        }
    }
    pixels
}

/// Distant galaxy / nebula "weenie": an elongated soft blob with a denser
/// interior, tinted per sprite and rotated per instance.
const GALAXY_TEX_SIZE: u32 = 128;

fn galaxy_pixels() -> Vec<u8> {
    let w = GALAXY_TEX_SIZE as usize;
    let half = GALAXY_TEX_SIZE as f32 / 2.0;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - half) / half;
            let dy = (y as f32 + 0.5 - half) / half;
            // Slightly elongated ellipse (the per-instance rotation comes
            // from the Transform, so it is not baked in here).
            let d = (dx * dx + (dy / 0.6) * (dy / 0.6)).sqrt().min(1.0);
            let a = if d <= 0.12 {
                0.92
            } else {
                let t = (d - 0.12) / 0.88;
                0.92 * (1.0 - t).powf(0.9)
            };
            pixels.extend_from_slice(&[255u8, 255u8, 255u8, (a * 255.0) as u8]);
        }
    }
    pixels
}

// ============================================================
// Seed derivation (the article's `Seed::hash_with`)
// ============================================================

/// SplitMix64 finalizer: a cheap, high-quality bit mixer. Used to derive
/// per-sector seeds so that (a) the same inputs always give the same sky
/// and (b) neighbouring sectors are uncorrelated. Dependency-free and
/// deterministic on every platform — no `thread_rng`, ever.
fn mix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seed of one sector: `global ⟶ layer ⟶ grid cell`, mirroring the
/// article's `seed.hash_with(("stars", grid_pos))`.
pub fn seed_for(base: u64, layer: u8, grid: IVec2) -> u64 {
    let cell = ((grid.x as u32 as u64) << 32) ^ (grid.y as u32 as u64);
    mix64(mix64(mix64(base) ^ (layer as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ cell)
}

// ============================================================
// Setup
// ============================================================

fn setup_starfield(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let dot = images.add(crate::rendering::textures::build_image(
        star_dot_pixels(),
        STAR_TEX_SIZE,
        STAR_TEX_SIZE,
    ));
    let spike = images.add(crate::rendering::textures::build_image(
        star_spike_pixels(),
        STAR_TEX_SIZE,
        STAR_TEX_SIZE,
    ));
    let galaxy = images.add(crate::rendering::textures::build_image(
        galaxy_pixels(),
        GALAXY_TEX_SIZE,
        GALAXY_TEX_SIZE,
    ));
    commands.insert_resource(StarfieldTextures { dot, spike, galaxy });
}

// ============================================================
// Star look
// ============================================================

/// Radius + HDR tint of one star, plus whether it is one of the rare BIG
/// ones. Most stars are small pinpricks; a few per sector are much bigger
/// AND brighter — the article's "bigger stars with a randomized size", and
/// what makes a field read as a real sky. The big/small split also decides
/// the TEXTURE: dust = flat dot, big = halo + 4-point diffraction cross.
fn star_look(rng: &mut StdRng, layer: usize) -> (f32, Color, bool) {
    let cfg = &LAYERS[layer];
    if rng.gen_bool(cfg.big_chance) {
        (
            rng.gen_range(cfg.radius_mid..cfg.radius_max),
            star_color(rng, layer, 1.3),
            true,
        )
    } else {
        (
            rng.gen_range(cfg.radius_min..cfg.radius_mid),
            star_color(rng, layer, 1.0),
            false,
        )
    }
}

/// Generate a star colour based on which layer it belongs to
/// (0 = background, 1 = mid, 2 = foreground), scaled by `boost`.
///
/// Tints are OPAQUE (alpha 1.0) and in HDR (values > 1): the sky is washed
/// by the sun's halo (Halo Brightness ~1), so a dim tint renders DARKER
/// than the sky and the "star" looks like a black speck — that is exactly
/// how the field stopped looking luminous. Staying above
/// `SKY_WASH_REFERENCE` in every layer keeps the stars as light sources;
/// the camera is `Hdr` and tonemaps, so >1 saturates to a bright dot
/// (like the blown-out cores in the astrophoto reference).
fn star_color(rng: &mut StdRng, layer: usize, boost: f32) -> Color {
    match layer {
        0 => {
            // Layer 1: small, faint — cool white-blue pinpricks.
            let b = rng.gen_range(1.1..1.9) * boost;
            let cool = rng.gen_range(0.86..0.96);
            Color::srgb(b * cool, b * cool, b)
        }
        1 => {
            // Layer 2: mid — warm or cool hue.
            let b = rng.gen_range(1.5..2.5) * boost;
            if rng.gen_bool(0.5) {
                // Warm: red-orange-yellow
                let g = rng.gen_range(0.62..0.80);
                let bl = rng.gen_range(0.30..0.50);
                Color::srgb(b, b * g, b * bl)
            } else {
                // Cool: blue-white
                let r = rng.gen_range(0.70..0.86);
                let g = rng.gen_range(0.80..0.92);
                Color::srgb(b * r, b * g, b)
            }
        }
        _ => {
            // Layer 3: big, bright — white to warm yellow (Vega-like).
            let b = rng.gen_range(2.2..3.4) * boost;
            let warm = rng.gen_range(0.90..1.0);
            let bl = rng.gen_range(0.70..0.88);
            Color::srgb(b, b * warm, b * bl)
        }
    }
}

/// Violet / blue / magenta, HDR: a galaxy has to be brighter than the
/// washed sky to read as a distant object instead of a dark smudge.
fn galaxy_color(rng: &mut StdRng) -> Color {
    let b = rng.gen_range(1.7..2.9);
    match rng.gen_range(0..3) {
        0 => Color::srgb(b * 0.80, b * 0.60, b),
        1 => Color::srgb(b * 0.55, b * 0.72, b),
        _ => Color::srgb(b * 0.82, b * 0.52, b * 0.88),
    }
}

// ============================================================
// Sector spawning
// ============================================================

/// World position of a sector root for a camera at `cam`:
/// `origin + cam·factor`. Set at SPAWN time as well as every frame, so a
/// freshly streamed sector never flashes at the world origin before its
/// first update (the entity does not exist in the World yet on the frame
/// it is spawned — commands are deferred).
fn sector_origin_world(grid: IVec2, factor: f32, cam: Vec2) -> Vec2 {
    grid.as_vec2() * SECTOR_SIZE + cam * factor
}

/// Deterministically build one sector (stars, plus sometimes a galaxy on
/// the near layer) and return its root entity.
fn spawn_sector(
    commands: &mut Commands,
    layer: u8,
    grid: IVec2,
    base_seed: u64,
    cam: Vec2,
    textures: &StarfieldTextures,
) -> Entity {
    let cfg = &LAYERS[layer as usize];
    let mut rng = StdRng::seed_from_u64(seed_for(base_seed, layer, grid));

    let origin = sector_origin_world(grid, cfg.factor, cam);
    let root = commands
        .spawn((
            StarSector {
                layer,
                grid,
                factor: cfg.factor,
            },
            Transform::from_xyz(origin.x, origin.y, cfg.z),
            Visibility::default(),
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        for _ in 0..cfg.stars {
            let (radius, color, is_big) = star_look(&mut rng, layer as usize);
            let pos = Vec2::new(
                rng.gen_range(0.0..SECTOR_SIZE),
                rng.gen_range(0.0..SECTOR_SIZE),
            );
            // Bimodal look: dust is a flat dot the size of its radius; the
            // big ones get the halo + diffraction cross, with the quad
            // scaled so the saturated core still matches the radius.
            let (image, quad_scale) = if is_big {
                (textures.spike.clone(), SPIKE_SCALE)
            } else {
                (textures.dot.clone(), DOT_SCALE)
            };
            parent.spawn((
                Sprite {
                    image,
                    color,
                    custom_size: Some(Vec2::splat(radius * 2.0 * quad_scale)),
                    ..default()
                },
                // Marks it for firefly's sprite shader: whole-quad stencil,
                // no normal map (see STARFIELD_HEIGHT).
                SpriteHeight(STARFIELD_HEIGHT),
                Transform::from_xyz(pos.x, pos.y, 0.0),
                Visibility::default(),
                RenderLayers::layer(1),
                Star,
            ));
        }

        // Weenies: distant galaxies are navigation landmarks, so they live
        // on the NEAR layer, where they drift a little (they have some
        // parallax instead of being nailed to the screen).
        if layer as usize == LAYERS.len() - 1 && rng.gen_bool(GALAXY_CHANCE) {
            let size = rng.gen_range(GALAXY_SIZE_MIN..GALAXY_SIZE_MAX);
            let pos = Vec2::new(
                rng.gen_range(0.0..SECTOR_SIZE),
                rng.gen_range(0.0..SECTOR_SIZE),
            );
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            parent.spawn((
                Sprite {
                    image: textures.galaxy.clone(),
                    color: galaxy_color(&mut rng),
                    custom_size: Some(Vec2::splat(size)),
                    ..default()
                },
                SpriteHeight(STARFIELD_HEIGHT),
                Transform::from_xyz(pos.x, pos.y, 0.0).with_rotation(Quat::from_rotation_z(angle)),
                Visibility::default(),
                RenderLayers::layer(1),
                Galaxy,
            ));
        }
    });

    root
}

// ============================================================
// Streaming
// ============================================================

/// Grid range (inclusive) of the sectors that intersect the view expanded
/// by `margin` world units, clamped so the enumeration can never explode
/// when zoomed far out.
fn sector_range(center: Vec2, half_view: Vec2, margin: f32) -> (IVec2, IVec2) {
    let side = (MAX_SECTORS_PER_LAYER as f32).sqrt().ceil() as i32;
    let max_span = side * 2 + 1;
    let reach = half_view + Vec2::splat(margin);
    let lo_f = (center - reach) / SECTOR_SIZE;
    let hi_f = (center + reach) / SECTOR_SIZE;
    let mut lo = IVec2::new(lo_f.x.floor() as i32, lo_f.y.floor() as i32);
    let mut hi = IVec2::new(hi_f.x.floor() as i32, hi_f.y.floor() as i32);
    // Clamp the span (centred on the view) at extreme zoom-out.
    if hi.x - lo.x + 1 > max_span {
        let mid = (lo.x + hi.x) / 2;
        lo.x = mid - side;
        hi.x = mid + side;
    }
    if hi.y - lo.y + 1 > max_span {
        let mid = (lo.y + hi.y) / 2;
        lo.y = mid - side;
        hi.y = mid + side;
    }
    (lo, hi)
}

/// Sectors to keep or spawn this frame: the cells intersecting the view
/// expanded by `margin`, nearest-first and capped at
/// `MAX_SECTORS_PER_LAYER`, so a far zoom-out keeps the densest part of the
/// sky instead of exploding.
fn desired_sectors(center: Vec2, half_view: Vec2, margin: f32) -> Vec<IVec2> {
    let (lo, hi) = sector_range(center, half_view, margin);
    let mut cells: Vec<(f32, IVec2)> = Vec::new();
    for gy in lo.y..=hi.y {
        for gx in lo.x..=hi.x {
            let grid = IVec2::new(gx, gy);
            let d = grid.as_vec2() * SECTOR_SIZE + Vec2::splat(SECTOR_SIZE * 0.5) - center;
            cells.push((d.length_squared(), grid));
        }
    }
    cells.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    cells.truncate(MAX_SECTORS_PER_LAYER);
    cells.into_iter().map(|(_, g)| g).collect()
}

/// Half of the visible extent in world units: the window size (preferred,
/// always available in the app) or the camera's logical viewport, with a
/// 16:9 fallback for headless runs, times the zoom.
fn viewport_half(window: Option<Vec2>, camera: &Camera, zoom: f32) -> Vec2 {
    let size = window
        .or_else(|| camera.logical_viewport_size())
        .unwrap_or(Vec2::new(1280.0, 720.0));
    size * 0.5 * zoom
}

/// Stream the sky: spawn the sectors the camera can see, despawn the ones it
/// cannot, and place every live sector for this frame's parallax offset.
///
///   base-space window = cam·drift ± half_view   (drift = 1 − factor)
///   sector world x/y  = grid·SECTOR_SIZE + cam·factor
///   ⇒ a star at base b renders at world = b + cam·factor
///     (it slides at 1 − factor relative to the camera)
fn stream_starfield(
    mut commands: Commands,
    // `Without<StarSector>` keeps this disjoint from the sector query below:
    // two parameters touching `Transform` in one system is a B0001 panic
    // (the classic "Unreachable" WASM trap) unless the filters are disjoint.
    cameras: Query<
        (&Transform, &Projection, &Camera),
        (With<Camera2d>, With<MainCamera>, Without<StarSector>),
    >,
    windows: Query<&Window>,
    textures: Option<Res<StarfieldTextures>>,
    seed: Res<StarSeed>,
    mut sectors: ResMut<StarSectors>,
    mut roots: Query<&mut Transform, With<StarSector>>,
) {
    crate::mark_system("stream_starfield");

    let Ok((cam_tf, projection, camera)) = cameras.single() else {
        return;
    };
    let Some(textures) = textures else {
        return;
    };
    let cam = cam_tf.translation.truncate();
    let zoom = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let window_size = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()));
    let half_view = viewport_half(window_size, camera, zoom);

    // 1) Which sectors should stay alive (KEEP_MARGIN, hysteresis) and
    //    which of the visible ones (SPAWN_MARGIN) are still missing.
    let mut wanted: HashSet<(u8, IVec2)> = HashSet::new();
    let mut missing: Vec<(u8, IVec2)> = Vec::new();
    for (li, cfg) in LAYERS.iter().enumerate() {
        let layer = li as u8;
        let base = cam * (1.0 - cfg.factor);
        for grid in desired_sectors(base, half_view, KEEP_MARGIN) {
            wanted.insert((layer, grid));
        }
        for grid in desired_sectors(base, half_view, SPAWN_MARGIN) {
            let key = (layer, grid);
            wanted.insert(key);
            if !sectors.0.contains_key(&key) {
                missing.push(key);
            }
        }
    }

    // 2) Despawn everything outside the wanted set.
    sectors.0.retain(|key, entity| {
        if wanted.contains(key) {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });

    // 3) Spawn the missing sectors (deterministic per cell; positioned at
    //    spawn so they never flash at the origin).
    for (layer, grid) in missing {
        let entity = spawn_sector(&mut commands, layer, grid, seed.0, cam, &textures);
        sectors.0.insert((layer, grid), entity);
    }

    // 4) Place every live sector for this frame's parallax offset. Sectors
    //    spawned in step 3 are not in the World yet (deferred commands) and
    //    already carry the right Transform, so skipping them is correct.
    for ((li, grid), entity) in sectors.0.iter() {
        let cfg = &LAYERS[*li as usize];
        if let Ok(mut tf) = roots.get_mut(*entity) {
            let origin = sector_origin_world(*grid, cfg.factor, cam);
            tf.translation.x = origin.x;
            tf.translation.y = origin.y;
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_derivation_is_deterministic_and_distinct() {
        let a = seed_for(42, 0, IVec2::new(0, 0));
        assert_eq!(
            a,
            seed_for(42, 0, IVec2::new(0, 0)),
            "same input, same seed"
        );
        assert_ne!(
            a,
            seed_for(43, 0, IVec2::new(0, 0)),
            "global seed must matter"
        );
        assert_ne!(a, seed_for(42, 1, IVec2::new(0, 0)), "layer must matter");
        assert_ne!(a, seed_for(42, 0, IVec2::new(1, 0)), "grid x must matter");
        assert_ne!(a, seed_for(42, 0, IVec2::new(0, 1)), "grid y must matter");
        // Neighbouring cells must not be correlated (that would visibly
        // tile the sky): no duplicate seeds over a wide neighbourhood.
        let mut seen = HashSet::new();
        for gy in -8..=8 {
            for gx in -8..=8 {
                assert!(
                    seen.insert(seed_for(42, 0, IVec2::new(gx, gy))),
                    "duplicate sector seed at ({gx},{gy})"
                );
            }
        }
        // Negative cells must not collide with positive ones.
        assert_ne!(
            seed_for(42, 0, IVec2::new(-1, 0)),
            seed_for(42, 0, IVec2::new(1, 0))
        );
    }

    #[test]
    fn density_per_sector_matches_previous_box() {
        // The old field was 12500/5000/1250 stars over a 10000×10000 box.
        let old_area = 10_000.0 * 10_000.0;
        let old = [12500.0, 5000.0, 1250.0];
        let sector_area = SECTOR_SIZE * SECTOR_SIZE;
        for (i, cfg) in LAYERS.iter().enumerate() {
            let old_density = old[i] / old_area;
            let new_density = cfg.stars as f32 / sector_area;
            let ratio = new_density / old_density;
            assert!(
                (ratio - 1.0).abs() < 0.06,
                "layer {i}: density changed by {:.1}%",
                (ratio - 1.0) * 100.0
            );
        }
    }

    #[test]
    fn sector_range_covers_view_and_is_capped() {
        // Normal zoom: the range must cover the view plus the margin.
        let half = Vec2::new(640.0, 360.0);
        let margin = 250.0;
        let (lo, hi) = sector_range(Vec2::ZERO, half, margin);
        assert!(
            (lo.x as f32) * SECTOR_SIZE <= -(half.x + margin),
            "left edge not covered"
        );
        assert!(
            ((hi.x + 1) as f32) * SECTOR_SIZE >= half.x + margin,
            "right edge not covered"
        );
        assert!(
            (lo.y as f32) * SECTOR_SIZE <= -(half.y + margin),
            "bottom edge not covered"
        );
        assert!(
            ((hi.y + 1) as f32) * SECTOR_SIZE >= half.y + margin,
            "top edge not covered"
        );
        // A phone viewport must not over-provision: the spawn set stays small.
        let phone = desired_sectors(Vec2::ZERO, Vec2::new(195.0, 422.0), SPAWN_MARGIN);
        assert!(
            phone.len() <= 9,
            "phone view spawns {} sectors (expected a handful)",
            phone.len()
        );
        // Extreme zoom-out: the enumeration must stay bounded...
        let (lo, hi) = sector_range(Vec2::ZERO, Vec2::splat(1.0e6), SPAWN_MARGIN);
        let span_x = hi.x - lo.x + 1;
        let span_y = hi.y - lo.y + 1;
        assert!(
            span_x <= 21 && span_y <= 21,
            "range exploded: {span_x}×{span_y}"
        );
        // ...and the capped list never exceeds the budget, nearest first.
        let cells = desired_sectors(Vec2::ZERO, Vec2::splat(1.0e6), SPAWN_MARGIN);
        assert_eq!(cells.len(), MAX_SECTORS_PER_LAYER);
        let centre = |g: IVec2| g.as_vec2() * SECTOR_SIZE + Vec2::splat(SECTOR_SIZE * 0.5);
        assert!(
            centre(cells[0]).length() <= centre(cells[cells.len() - 1]).length(),
            "sectors must be kept nearest-first"
        );
        // Hysteresis: the keep set must be a superset of the spawn set.
        let spawn = desired_sectors(Vec2::ZERO, Vec2::new(640.0, 360.0), SPAWN_MARGIN);
        let keep = desired_sectors(Vec2::ZERO, Vec2::new(640.0, 360.0), KEEP_MARGIN);
        for g in &spawn {
            assert!(keep.contains(g), "keep set must contain {g:?}");
        }
        assert!(keep.len() > spawn.len(), "keep must be strictly larger");
    }

    #[test]
    fn sector_transform_follows_parallax_formula() {
        // A star at base position `b` must render at `world = b + cam·factor`:
        // its camera-relative slide is exactly (1 − factor), whatever sector
        // it lives in. Same formula as `stream_starfield`.
        let cam = Vec2::new(1234.0, -567.0);
        for (li, cfg) in LAYERS.iter().enumerate() {
            let grid = IVec2::new(-3, 7);
            let origin = grid.as_vec2() * SECTOR_SIZE;
            let root = sector_origin_world(grid, cfg.factor, cam);
            let local = Vec2::new(250.0, 800.0);
            let world = root + local;
            let base = origin + local;
            let expected = base + cam * cfg.factor;
            assert!(
                (world - expected).length() < 1e-2,
                "layer {li}: world {world:?} != base + cam·factor {expected:?}"
            );
            // On-screen motion is (1 − factor) × camera motion.
            let cam2 = cam + Vec2::new(100.0, 0.0);
            let world2 = sector_origin_world(grid, cfg.factor, cam2) + local;
            let slide = ((world2 - cam2) - (world - cam)).length();
            let expected_slide = 100.0 * (1.0 - cfg.factor);
            assert!(
                (slide - expected_slide).abs() < 1e-2,
                "layer {li}: slide {slide} != {expected_slide}"
            );
        }
    }

    #[test]
    fn big_stars_are_rare_but_present() {
        // Size variety: most stars small, some big AND brighter.
        let mut rng = StdRng::seed_from_u64(7);
        let cfg = &LAYERS[0];
        let total = 2000;
        let mut big = 0;
        for _ in 0..total {
            let (radius, color, is_big) = star_look(&mut rng, 0);
            assert!(radius >= cfg.radius_min && radius <= cfg.radius_max);
            assert_eq!(
                is_big,
                radius > cfg.radius_mid,
                "the big/small flag must agree with the radius band"
            );
            if radius > cfg.radius_mid {
                big += 1;
                let s = color.to_srgba();
                assert!(
                    s.red.max(s.green).max(s.blue) >= 1.3,
                    "big stars must also be brighter"
                );
            }
        }
        let ratio = big as f32 / total as f32;
        assert!(
            (ratio - cfg.big_chance as f32).abs() < 0.05,
            "big-star ratio {ratio} should be close to {}",
            cfg.big_chance
        );
    }

    #[test]
    fn galaxies_are_deterministic_and_sparse() {
        assert!(GALAXY_CHANCE > 0.0 && GALAXY_CHANCE < 0.5);
        assert!(GALAXY_SIZE_MIN < GALAXY_SIZE_MAX);
        // Deterministic per sector: the same cell gives the same coin flip.
        let mut rng_a = StdRng::seed_from_u64(seed_for(42, 2, IVec2::new(5, -2)));
        let mut rng_b = StdRng::seed_from_u64(seed_for(42, 2, IVec2::new(5, -2)));
        assert_eq!(rng_a.gen_bool(GALAXY_CHANCE), rng_b.gen_bool(GALAXY_CHANCE));
        // Over a wide sample the fraction stays in the expected ballpark.
        let samples = 600;
        let mut with_galaxy = 0;
        for i in 0..samples {
            let mut rng = StdRng::seed_from_u64(seed_for(42, 2, IVec2::new(i, 3)));
            if rng.gen_bool(GALAXY_CHANCE) {
                with_galaxy += 1;
            }
        }
        let ratio = with_galaxy as f64 / samples as f64;
        assert!(
            (ratio - GALAXY_CHANCE).abs() < 0.08,
            "galaxy ratio {ratio} should be close to {GALAXY_CHANCE}"
        );
    }

    #[test]
    fn star_dot_is_flat_with_a_hard_edge() {
        // The dust look: a saturated disc that stays at full alpha all the
        // way out and then drops to 0 — no halo, so it reads as a crisp
        // pinprick instead of a fuzzy blob.
        let px = star_dot_pixels();
        let w = STAR_TEX_SIZE as usize;
        let half = STAR_TEX_SIZE as f32 / 2.0;
        let row = w / 2;
        let center = px[(row * w + w / 2) * 4 + 3];
        assert_eq!(center, 255, "dot centre must be fully opaque");
        assert_eq!(px[3], 0, "outside the disc the alpha must be 0");
        // Flat: half radius and 80% radius are the same full alpha.
        let half_x = row + (half * 0.5) as usize;
        let far_x = row + (half * 0.8) as usize;
        assert_eq!(
            px[(row * w + half_x) * 4 + 3],
            255,
            "dot must be flat at r/2"
        );
        assert_eq!(
            px[(row * w + far_x) * 4 + 3],
            255,
            "dot must be flat at 0.8r"
        );
        // Hard edge: the corner of the quad is empty, the rim is not.
        assert_eq!(px[(0 * w + 0) * 4 + 3], 0, "corner must be empty");
    }

    #[test]
    fn spike_texture_has_a_four_point_cross() {
        // The big-star look: a saturated core, a soft halo AND — the point
        // of the whole thing — a 4-point diffraction cross that beats the
        // halo along the axes (otherwise it drowns in the glow).
        let px = star_spike_pixels();
        let w = STAR_TEX_SIZE as usize;
        let half = STAR_TEX_SIZE as f32 / 2.0;
        let c = w / 2;
        let at = |x: usize, y: usize| px[(y * w + x) * 4 + 3] as f32 / 255.0;
        assert_eq!(at(c, c), 1.0, "core centre must be fully opaque");
        assert_eq!(px[3], 0, "halo must fade to exactly 0 at the rim");
        // Along the axis at 0.7r vs the diagonal at the same radius.
        let r = half * 0.7;
        let diag = r / std::f32::consts::SQRT_2;
        let on_axis = at(c + r as usize, c);
        let on_diagonal = at(c + diag as usize, c + diag as usize);
        assert!(
            on_axis > on_diagonal * 1.15,
            "the cross must stand out from the halo (axis {on_axis} vs diagonal {on_diagonal})"
        );
        // The spikes must survive to the outer part of the quad.
        let outer = at(c + (half * 0.9) as usize, c);
        assert!(
            outer > 0.2,
            "spike must still be visible at 0.9r (a={outer})"
        );
        // And the halo must still be there for the off-axis directions.
        let halo = at(c + (half * 0.35) as usize, c);
        assert!(halo > 0.5, "halo must stay bright near the core (a={halo})");
        // Vertically symmetric: the cross has four arms, not two.
        let h_arm = at(c + r as usize, c);
        let v_arm = at(c, c + r as usize);
        assert!(
            (h_arm - v_arm).abs() < 0.06,
            "the cross must be symmetric (h {h_arm} vs v {v_arm})"
        );
        let v = at(c, c + (half * 0.8) as usize);
        assert!(v > 0.2, "the vertical arm must exist too (a={v})");
    }

    #[test]
    fn galaxy_texture_is_soft_and_never_fully_opaque() {
        // A galaxy must be a soft blob: if any texel reached alpha 1.0 the
        // sprite shader would treat it as an opaque surface (and the core
        // would look like a hard-edged dot in the sky).
        let px = galaxy_pixels();
        let w = GALAXY_TEX_SIZE as usize;
        let center = px[(w / 2 * w + w / 2) * 4 + 3];
        assert!(center < 255 && center > 180, "galaxy core alpha {center}");
        assert_eq!(px[3], 0, "galaxy must fade to 0 at the rim");
        let row = w / 2;
        let mut prev = center;
        for x in (w / 2)..w {
            let a = px[(row * w + x) * 4 + 3];
            assert!(a <= prev, "galaxy must fall off monotonically (x={x})");
            prev = a;
        }
    }

    /// The whole point of the star look: every star must be BRIGHTER than
    /// the sun-washed sky, otherwise it renders as a dark speck instead of
    /// a luminous point. Opaque alpha is required too (the tint, not
    /// translucency, carries the brightness).
    #[test]
    fn star_tints_outshine_the_washed_sky() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut faintest = f32::MAX;
        for layer in 0..LAYERS.len() {
            for _ in 0..200 {
                let (_, color, _) = star_look(&mut rng, layer);
                let s = color.to_srgba();
                assert!(
                    (s.alpha - 1.0).abs() < 1e-6,
                    "layer {layer}: star tint must be opaque (alpha={})",
                    s.alpha
                );
                faintest = faintest.min(s.red.max(s.green).max(s.blue));
            }
        }
        assert!(
            faintest > SKY_WASH_REFERENCE,
            "faintest star tint ({faintest}) must outshine the washed sky \
             ({SKY_WASH_REFERENCE})"
        );
        // Galaxies too (dimmer than stars, but never a dark smudge).
        for _ in 0..200 {
            let s = galaxy_color(&mut rng).to_srgba();
            assert!((s.alpha - 1.0).abs() < 1e-6);
            assert!(
                s.red.max(s.green).max(s.blue) > 1.0,
                "galaxy tint must be HDR (above the washed sky)"
            );
        }
        // Layer brightness ordering must survive (depth cue).
        let mut rng = StdRng::seed_from_u64(7);
        let peak_mean = |layer: usize, rng: &mut StdRng| -> f32 {
            let mut acc = 0.0;
            for _ in 0..200 {
                let (_, color, _) = star_look(rng, layer);
                let s = color.to_srgba();
                acc += s.red.max(s.green).max(s.blue);
            }
            acc / 200.0
        };
        let m0 = peak_mean(0, &mut rng);
        let m1 = peak_mean(1, &mut rng);
        let m2 = peak_mean(2, &mut rng);
        assert!(
            m0 < m1 && m1 < m2,
            "layer brightness must increase with depth ({m0} / {m1} / {m2})"
        );
    }

    /// Guards the contract with the vendored sprite shader: the starfield
    /// sprite must carry the NEGATIVE sentinel height (whole-quad stencil
    /// → shadow exemption) and never a real height (which would break the
    /// z-skip logic for planets).
    #[test]
    fn starfield_height_sentinel_is_negative_and_unique() {
        assert!(STARFIELD_HEIGHT < 0.0);
        // Real heights in the project: planets 0.0, star disc +1000.
        assert_ne!(STARFIELD_HEIGHT, 0.0);
        assert_ne!(STARFIELD_HEIGHT, 1000.0);
    }

    #[test]
    fn layers_are_ordered_far_to_near() {
        // The sky must sit behind any planet (`sync_sprite_z` writes
        // planets at z = -distance from the star).
        for cfg in &LAYERS {
            assert!(cfg.z < -100_000.0, "layer z {} not behind planets", cfg.z);
        }
        assert!(LAYERS[0].z < LAYERS[1].z && LAYERS[1].z < LAYERS[2].z);
        // Far layers are more glued to the camera than near ones.
        assert!(LAYERS[0].factor > LAYERS[1].factor && LAYERS[1].factor > LAYERS[2].factor);
        assert!((LAYERS[0].factor - 1.0).abs() < 1e-6);
    }

    /// Guards the invariant that broke v0.14.138: in firefly's vendored
    /// sprite shader the NORMAL-MAP write — the thing that gives planets
    /// their day/night terminator — must stay identical to the crate's
    /// original logic (`color.a >= 1.0`) and must NEVER depend on the
    /// starfield sentinel. If it does, a misbehaving condition makes every
    /// sprite write no normal at all: planets then lose the terminator and
    /// receive Halo Brightness instead of Planet Light (the "the lit part
    /// of the planets is gone" bug), while the shadow cones still work.
    #[test]
    fn sprite_shader_normal_write_stays_independent_of_starfield() {
        let src = std::fs::read_to_string("vendor/bevy_firefly/src/shaders/sprite.wgsl")
            .expect("vendored sprite shader must be present");
        let normal_start = src
            .find("if color.a >= 1.0 {")
            .expect("the normal write must keep the crate's original condition");
        let tail = &src[normal_start..];
        let normal_block = &tail[..tail.find("return res").unwrap_or(tail.len())];
        assert!(
            !normal_block.contains("is_starfield"),
            "the normal-map write must NOT depend on the starfield sentinel"
        );
        // The sentinel threshold must use standard WGSL (`select`), never an
        // if-expression: WGSL has no if-expression and naga tolerance is not
        // something to bet the planet lighting on.
        assert!(
            src.contains("select(1.0, 0.004, is_starfield)"),
            "the starfield stencil threshold must use WGSL `select`"
        );
    }

    #[test]
    fn viewport_half_prefers_window_and_falls_back_headless() {
        // Headless (no Window, no resolved target info): 16:9 fallback, so
        // the streaming system still spawns something in tests.
        let camera = Camera::default();
        let half = viewport_half(None, &camera, 2.0);
        assert!(
            (half - Vec2::new(1280.0, 720.0)).length() < 1e-3,
            "{half:?}"
        );
        // A window always wins, and zoom multiplies linearly.
        let half = viewport_half(Some(Vec2::new(400.0, 300.0)), &camera, 1.0);
        assert!((half - Vec2::new(200.0, 150.0)).length() < 1e-3, "{half:?}");
        let half = viewport_half(Some(Vec2::new(400.0, 300.0)), &camera, 0.5);
        assert!((half - Vec2::new(100.0, 75.0)).length() < 1e-3, "{half:?}");
    }

    /// End-to-end streaming on a headless app: sectors appear around the
    /// camera, follow the parallax offset, and panning streams NEW cells in
    /// while the live count stays bounded — that is what makes the field
    /// endless without an entity explosion.
    #[test]
    fn streaming_spawns_around_camera_and_recycles_on_pan() {
        use crate::main_camera_projection;

        fn count<D: bevy::ecs::query::QueryData>(app: &mut App) -> usize {
            let world = app.world_mut();
            let mut qs = world.query::<D>();
            qs.iter(world).count()
        }

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Image>()
            .init_resource::<StarSeed>()
            .init_resource::<StarSectors>()
            .add_systems(Startup, setup_starfield)
            .add_systems(Update, stream_starfield);

        let cam = app
            .world_mut()
            .spawn((
                Camera2d,
                MainCamera,
                main_camera_projection(),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        app.update();

        let first: Vec<(u8, IVec2)> = {
            let mut world = app.world_mut();
            world.resource::<StarSectors>().0.keys().copied().collect()
        };
        assert!(!first.is_empty(), "no sector spawned around the camera");
        assert_eq!(
            count::<&StarSector>(&mut app),
            first.len(),
            "bookkeeping out of sync with the live sector entities"
        );
        assert!(count::<&Star>(&mut app) > 0, "no stars spawned");

        // The far layer (factor 1.0) is glued to the camera: its cell set
        // must NOT change when panning — it is an infinitely distant backdrop.
        let far_before: HashSet<IVec2> = first
            .iter()
            .filter(|(lr, _)| *lr == 0)
            .map(|(_, g)| *g)
            .collect();
        assert!(!far_before.is_empty());

        // Pan far away: new cells must stream in for the drifting layers.
        let cam_now = Vec2::new(6000.0, -4000.0);
        app.world_mut()
            .entity_mut(cam)
            .insert(Transform::from_xyz(cam_now.x, cam_now.y, 0.0));
        app.update();

        let after: Vec<(u8, IVec2)> = {
            let mut world = app.world_mut();
            world.resource::<StarSectors>().0.keys().copied().collect()
        };
        assert!(
            after
                .iter()
                .any(|(lr, g)| *lr == 2 && !first.contains(&(2, *g))),
            "panning must stream in new sectors on the near layer"
        );
        let far_after: HashSet<IVec2> = after
            .iter()
            .filter(|(lr, _)| *lr == 0)
            .map(|(_, g)| *g)
            .collect();
        assert_eq!(far_before, far_after, "the glued layer must not stream");
        // Bounded and consistent: no leak, no explosion.
        assert!(after.len() <= LAYERS.len() * MAX_SECTORS_PER_LAYER);
        assert_eq!(
            count::<&StarSector>(&mut app),
            after.len(),
            "despawned sectors leaked entities"
        );

        // Every live sector must sit at its parallax offset for this frame.
        let mut world = app.world_mut();
        let mut qs = world.query::<(&Transform, &StarSector)>();
        let mut checked = 0;
        for (tf, sector) in qs.iter(world) {
            let expected =
                sector_origin_world(sector.grid, LAYERS[sector.layer as usize].factor, cam_now);
            assert!(
                (tf.translation.truncate() - expected).length() < 1.0,
                "sector {:?} placed at {:?}, expected {expected:?}",
                sector.grid,
                tf.translation.truncate()
            );
            checked += 1;
        }
        assert!(checked > 0);
    }
}
