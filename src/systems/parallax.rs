use bevy::prelude::*;
use crate::systems::camera::MainCamera;
use bevy::camera::visibility::RenderLayers;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

// ============================================================
// Components
// ============================================================

/// Marks a parallax layer parent entity. The `factor` determines
/// how much the layer moves relative to the camera (0.0 = fixed).
#[derive(Component)]
pub struct ParallaxLayer {
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
    radius_min: f32,
    radius_max: f32,
    factor: f32,
    z: f32,
}

const LAYERS: [LayerConfig; 3] = [
    // Layer 1 (sfondo) — 500 gray stars, fixed
    LayerConfig {
        count: 500,
        radius_min: 0.5,
        radius_max: 1.5,
        factor: 0.0,
        z: -100.0,
    },
    // Layer 2 (medio) — 200 warm/cool stars, 20% parallax
    LayerConfig {
        count: 200,
        radius_min: 1.0,
        radius_max: 3.0,
        factor: 0.2,
        z: -90.0,
    },
    // Layer 3 (primo piano) — 50 white/yellow stars, 50% parallax
    LayerConfig {
        count: 50,
        radius_min: 2.0,
        radius_max: 4.0,
        factor: 0.5,
        z: -80.0,
    },
];

const EXTENT: f32 = 5000.0; // half-extent, so total area is 10_000 × 10_000

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
        // Parent entity — moving this offsets every star in the layer
        let parent_id = commands
            .spawn((
                Transform::from_xyz(0.0, 0.0, cfg.z),
                Visibility::default(),
                ParallaxLayer { factor: cfg.factor },
            ))
            .id();

        for _ in 0..cfg.count {
            let x = rng.gen_range(-EXTENT..EXTENT);
            let y = rng.gen_range(-EXTENT..EXTENT);
            let radius = rng.gen_range(cfg.radius_min..cfg.radius_max);

            let color = star_color(&mut rng, cfg.factor);

            // Build a circle mesh + material for this star
            let mesh = meshes.add(Circle::new(radius));
            let material = materials.add(ColorMaterial::from_color(color));

            commands
                .spawn((
                    Mesh2d(mesh),
                    MeshMaterial2d::<ColorMaterial>(material),
                    Transform::from_xyz(x, y, 0.0),
                    Visibility::default(),
                    RenderLayers::layer(1),
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
        Color::srgb(warmth, warmth * 0.9, warmth * 0.6)
            .with_alpha(rng.gen_range(0.7..1.0))
    }
}

// ============================================================
// Parallax movement
// ============================================================

/// Each frame, offset each layer parent by `-camera_pos * factor`.
/// Layer 1 (factor 0.0) stays fixed relative to the viewport.
fn update_parallax(
    mut cameras: ParamSet<(
        Query<&Transform, ((With<Camera2d>, With<MainCamera>), With<Projection>, With<MainCamera>)>,
        Query<(&mut Transform, &ParallaxLayer)>,
    )>,
) {
    crate::mark_system("update_parallax");

    let camera_pos = cameras.p0().single().unwrap().translation;
    for (mut transform, layer) in cameras.p1().iter_mut() {
        transform.translation.x = -camera_pos.x * layer.factor;
        transform.translation.y = -camera_pos.y * layer.factor;
    }
}
