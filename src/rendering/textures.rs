use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

// ============================================================================
// Value Noise 2D — deterministic, seeded, octave-supporting
// ============================================================================

const PERM_SIZE: usize = 256;

/// A 2D value-noise generator with fractal Brownian motion (octaves).
struct ValueNoise2D {
    perm: Vec<usize>, // 2× permutation table for easy wrapping
}

impl ValueNoise2D {
    fn new(seed: u32) -> Self {
        let mut table: Vec<usize> = (0..PERM_SIZE).collect();
        let mut rng = StdRng::seed_from_u64(seed as u64);
        // Fisher-Yates shuffle
        for i in (1..PERM_SIZE).rev() {
            let j = rng.gen_range(0..=i);
            table.swap(i, j);
        }
        // Duplicate for wrap-free indexing
        let mut perm = Vec::with_capacity(PERM_SIZE * 2);
        perm.extend_from_slice(&table);
        perm.extend_from_slice(&table);
        Self { perm }
    }

    /// Look up the pseudo-random value at integer lattice point (ix, iy).
    fn hash(&self, ix: i32, iy: i32) -> f32 {
        let idx = self.perm[(ix as usize) & 0xFF] + ((iy as usize) & 0xFF);
        self.perm[idx] as f32 / 255.0 // normalised to [0, 1]
    }

    /// Smoothstep for gradient-falloff interpolation.
    #[inline]
    fn fade(t: f32) -> f32 {
        t * t * (3.0 - 2.0 * t)
    }

    /// Single octave of value noise at (x, y).
    /// Returns a value in [0.0, 1.0].
    fn noise(&self, x: f32, y: f32) -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - ix as f32;
        let fy = y - iy as f32;

        let ux = Self::fade(fx);
        let uy = Self::fade(fy);

        let n00 = self.hash(ix, iy);
        let n10 = self.hash(ix + 1, iy);
        let n01 = self.hash(ix, iy + 1);
        let n11 = self.hash(ix + 1, iy + 1);

        // Bilinear interpolation
        let nx0 = n00 + ux * (n10 - n00);
        let nx1 = n01 + ux * (n11 - n01);
        nx0 + uy * (nx1 - nx0)
    }

    /// Fractal Brownian motion: sum of `octaves` noise layers.
    /// Returns a value in [0.0, 1.0].
    fn fbm(&self, x: f32, y: f32, octaves: u32) -> f32 {
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut max_value = 0.0;

        for _ in 0..octaves {
            value += amplitude * self.noise(x * frequency, y * frequency);
            max_value += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }

        value / max_value
    }
}

// ============================================================================
// Helpers
// ============================================================================

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

/// Linear interpolate between two RGB colours.
fn lerp_color(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (f32, f32, f32) {
    (
        lerp(a.0 as f32, b.0 as f32, t),
        lerp(a.1 as f32, b.1 as f32, t),
        lerp(a.2 as f32, b.2 as f32, t),
    )
}

/// Build a Bevy `Image` from a flat RGBA pixel buffer.
fn build_image(pixels: Vec<u8>, width: u32, height: u32) -> Image {
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let mut image = Image {
        data: Some(pixels),
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: ImageSampler::linear(),
        ..default()
    };
    image
}

// ============================================================================
// Diffuse texture generators (256×256 each)
// ============================================================================

const TEX_SIZE: u32 = 256;

/// Rocky / terrestrial planet — fractal noise + crater overlay.
fn generate_rocky_diffuse() -> Vec<u8> {
    let w = TEX_SIZE as usize;
    let noise = ValueNoise2D::new(42);
    let mut rng = StdRng::seed_from_u64(12_345);

    // Pre-generate random craters
    let crater_count = 14;
    let craters: Vec<(f32, f32, f32)> = (0..crater_count)
        .map(|_| {
            (
                rng.gen::<f32>() * TEX_SIZE as f32,
                rng.gen::<f32>() * TEX_SIZE as f32,
                rng.gen_range(4.0..22.0),
            )
        })
        .collect();

    let mut pixels = Vec::with_capacity(w * w * 4);

    for y in 0..w {
        for x in 0..w {
            let nx = x as f32 / TEX_SIZE as f32;
            let ny = y as f32 / TEX_SIZE as f32;

            // Fractal detail
            let n = noise.fbm(nx * 5.0, ny * 5.0, 6);

            // Palette: brown #8B5E3C → red #C1440E → ocher #D4A24C
            let (r, g, b) = if n < 0.35 {
                (139, 94, 60)  // brown
            } else if n < 0.6 {
                (193, 68, 14)  // red-orange
            } else {
                (212, 162, 76) // ocher
            };

            let mut fr = r as f32;
            let mut fg = g as f32;
            let mut fb = b as f32;

            // Darken inside craters
            for &(cx, cy, cr) in &craters {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < cr {
                    let t = 1.0 - (dist / cr);
                    let darken = 1.0 - t * 0.55;
                    fr *= darken;
                    fg *= darken;
                    fb *= darken;
                }
            }

            // Add fine noise variation
            let noise_detail = noise.noise(nx * 20.0, ny * 20.0) * 12.0 - 6.0;

            pixels.push((fr + noise_detail).clamp(0.0, 255.0) as u8);
            pixels.push((fg + noise_detail).clamp(0.0, 255.0) as u8);
            pixels.push((fb + noise_detail).clamp(0.0, 255.0) as u8);
            pixels.push(255);
        }
    }

    pixels
}

/// Gas giant — horizontal bands with sine-wave undulation.
fn generate_gas_diffuse() -> Vec<u8> {
    let w = TEX_SIZE as usize;
    let noise = ValueNoise2D::new(99);

    let palette: [(u8, u8, u8); 4] = [
        (200, 139, 58),  // #C88B3A  marrone chiaro
        (232, 213, 163), // #E8D5A3  beige
        (245, 240, 225), // #F5F0E1  bianco
        (160, 82, 45),   // #A0522D  marrone rossiccio
    ];
    let plen = palette.len() as f32;

    let mut pixels = Vec::with_capacity(w * w * 4);

    for y in 0..w {
        for x in 0..w {
            let nx = x as f32 / TEX_SIZE as f32;
            let ny = y as f32 / TEX_SIZE as f32;

            // 8 horizontal bands with sine waviness
            let wave = (nx * 6.0).sin() * 0.15;
            let band = (ny * 8.0 + wave).rem_euclid(1.0); // [0, 1) within a band

            // Which two palette entries we're between
            let idx = (band * plen).floor() as usize % palette.len();
            let next = (idx + 1) % palette.len();
            let t = (band * plen).fract();

            let (r1, g1, b1) = palette[idx];
            let (r2, g2, b2) = palette[next];

            let mut r = lerp(r1 as f32, r2 as f32, t);
            let mut g = lerp(g1 as f32, g2 as f32, t);
            let mut b = lerp(b1 as f32, b2 as f32, t);

            // Subtle noise for atmospheric texture
            let n = noise.noise(nx * 8.0, ny * 8.0);
            let nudge = (n - 0.5) * 14.0;
            r = (r + nudge).clamp(0.0, 255.0);
            g = (g + nudge).clamp(0.0, 255.0);
            b = (b + nudge).clamp(0.0, 255.0);

            pixels.push(r as u8);
            pixels.push(g as u8);
            pixels.push(b as u8);
            pixels.push(255);
        }
    }

    pixels
}

/// Ice planet — white/blue base with bright crystalline veins.
fn generate_ice_diffuse() -> Vec<u8> {
    let w = TEX_SIZE as usize;
    let noise = ValueNoise2D::new(77);

    let mut pixels = Vec::with_capacity(w * w * 4);

    for y in 0..w {
        for x in 0..w {
            let nx = x as f32 / TEX_SIZE as f32;
            let ny = y as f32 / TEX_SIZE as f32;

            // Base colour: mix of white (#F0F8FF) and light blue (#B0D4F1)
            let base = noise.fbm(nx * 3.0, ny * 3.0, 4);
            let r = lerp(176.0, 240.0, base);
            let g = lerp(212.0, 248.0, base);
            let b = lerp(241.0, 255.0, base);

            // Crystal vein patterns — sheared high-frequency noise
            let vein1 = noise.fbm(nx * 12.0 + ny * 8.0, ny * 12.0 - nx * 8.0, 3);
            let vein2 = noise.fbm(nx * 8.0 - ny * 6.0, nx * 6.0 + ny * 8.0, 3);
            let vein3 = noise.fbm(nx * 16.0 + ny * 4.0, nx * 4.0 - ny * 16.0, 2);
            let is_vein = vein1 > 0.62 || vein2 > 0.68 || (vein3 > 0.72 && vein1 > 0.45);

            let (fr, fg, fb) = if is_vein {
                // Bright icy vein
                (248.0, 252.0, 255.0)
            } else {
                (r, g, b)
            };

            pixels.push(fr as u8);
            pixels.push(fg as u8);
            pixels.push(fb as u8);
            pixels.push(255);
        }
    }

    pixels
}

/// Star — radial gradient from hot core to cool edge + sunspots.
fn generate_star_diffuse() -> Vec<u8> {
    let w = TEX_SIZE as usize;
    let noise = ValueNoise2D::new(33);
    let mut rng = StdRng::seed_from_u64(7777);

    // Sun spots
    let spot_count = 10;
    let spots: Vec<(f32, f32, f32)> = (0..spot_count)
        .map(|_| {
            (
                rng.gen::<f32>() * TEX_SIZE as f32,
                rng.gen::<f32>() * TEX_SIZE as f32,
                rng.gen_range(3.0..14.0),
            )
        })
        .collect();

    let mut pixels = Vec::with_capacity(w * w * 4);
    let half = TEX_SIZE as f32 / 2.0;

    for y in 0..w {
        for x in 0..w {
            let cx = x as f32 - half;
            let cy = y as f32 - half;
            let dist = (cx * cx + cy * cy).sqrt() / half; // 0 at center, 1 at edge

            // Radial gradient
            let (r, g, b) = if dist < 0.3 {
                // Core: yellow-white #FFF8DC → orange #FF8C00
                let t = dist / 0.3;
                lerp_color((255, 248, 220), (255, 140, 0), t)
            } else if dist < 0.6 {
                // Mid: orange → dark red
                let t = (dist - 0.3) / 0.3;
                lerp_color((255, 140, 0), (200, 50, 0), t)
            } else {
                // Edge: dark red → very dark red #8B0000
                let t = ((dist - 0.6) / 0.4).min(1.0);
                lerp_color((200, 50, 0), (139, 0, 0), t)
            };

            let mut fr = r;
            let mut fg = g;
            let mut fb = b;

            // Darken sun spots
            for &(sx, sy, sr) in &spots {
                let dx = x as f32 - sx;
                let dy = y as f32 - sy;
                let d = (dx * dx + dy * dy).sqrt();
                if d < sr {
                    let intensity = 1.0 - d / sr;
                    let darken = 1.0 - intensity * 0.6;
                    fr *= darken;
                    fg *= darken;
                    fb *= darken;
                }
            }

            // Surface noise (granulation)
            let grain = noise.noise(x as f32 * 0.05, y as f32 * 0.05) * 0.12;

            pixels.push((fr * (1.0 + grain)).clamp(0.0, 255.0) as u8);
            pixels.push((fg * (1.0 + grain)).clamp(0.0, 255.0) as u8);
            pixels.push((fb * (1.0 + grain)).clamp(0.0, 255.0) as u8);
            pixels.push(255);
        }
    }

    pixels
}

// ============================================================================
// Normal map generation (Sobel 3×3 filter)
// ============================================================================

/// Generate a normal-map RGBA buffer from the corresponding diffuse buffer.
///
/// `strength` controls how pronounced the normal effect is (default ~ 2.0).
fn generate_normal_map(diffuse: &[u8], width: u32, height: u32, strength: f32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;

    // Convert to grayscale height map using luminance weights
    let mut height = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let r = diffuse[idx] as f32;
            let g = diffuse[idx + 1] as f32;
            let b = diffuse[idx + 2] as f32;
            let lum = 0.299 * r + 0.587 * g + 0.114 * b;
            height.push(lum);
        }
    }

    // Sobel convolution kernels
    // Gx = [[-1,0,1],[-2,0,2],[-1,0,1]]
    // Gy = [[-1,-2,-1],[0,0,0],[1,2,1]]
    let kernel_gx: &[i32; 9] = &[-1, 0, 1, -2, 0, 2, -1, 0, 1];
    let kernel_gy: &[i32; 9] = &[-1, -2, -1, 0, 0, 0, 1, 2, 1];

    let wy = width as i32;
    let ht = h as i32;

    let mut normal = Vec::with_capacity(w * h * 4);

    for y in 0..ht {
        for x in 0..wy {
            // Sample the 3×3 neighbourhood with clamping at borders
            let sample = |dx: i32, dy: i32| -> f32 {
                let px = (x + dx).clamp(0, wy - 1);
                let py = (y + dy).clamp(0, ht - 1);
                height[(py * wy + px) as usize]
            };

            // Convolve
            let mut gx = 0.0;
            let mut gy = 0.0;
            for ky in 0..3 {
                for kx in 0..3 {
                    let k = (ky * 3 + kx) as usize;
                    let hval = sample(kx - 1, ky - 1);
                    gx += hval * kernel_gx[k] as f32;
                    gy += hval * kernel_gy[k] as f32;
                }
            }

            // Scale by strength and map from [-1,1] to [0,1]
            let nx = (gx / 255.0 * strength * 0.5 + 0.5).clamp(0.0, 1.0);
            let ny = (gy / 255.0 * strength * 0.5 + 0.5).clamp(0.0, 1.0);
            // Z normal always points out of the screen (flat surface)
            let nz = 1.0;

            normal.push((nx * 255.0) as u8);
            normal.push((ny * 255.0) as u8);
            normal.push((nz * 255.0) as u8);
            normal.push(255);
        }
    }

    normal
}

// ============================================================================
// TextureAssets resource
// ============================================================================

/// Holds handles to all generated diffuse + normal map textures.
#[derive(Resource)]
pub struct TextureAssets {
    pub rocky_diffuse: Handle<Image>,
    pub rocky_normal: Handle<Image>,
    pub gas_diffuse: Handle<Image>,
    pub gas_normal: Handle<Image>,
    pub ice_diffuse: Handle<Image>,
    pub ice_normal: Handle<Image>,
    pub star_diffuse: Handle<Image>,
    pub star_normal: Handle<Image>,
}

// ============================================================================
// Startup system
// ============================================================================

fn generate_texture_system(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // --- Rocky ---
    let rocky_pixels = generate_rocky_diffuse();
    let rocky_img = build_image(rocky_pixels, TEX_SIZE, TEX_SIZE);
    let rocky_diffuse = images.add(rocky_img);

    let rocky_normal_pixels = generate_normal_map(
        images.get(&rocky_diffuse).unwrap().data.as_deref().unwrap(),
        TEX_SIZE,
        TEX_SIZE,
        2.0,
    );
    let rocky_normal_img = build_image(rocky_normal_pixels, TEX_SIZE, TEX_SIZE);
    let rocky_normal = images.add(rocky_normal_img);

    // --- Gas ---
    let gas_pixels = generate_gas_diffuse();
    let gas_img = build_image(gas_pixels, TEX_SIZE, TEX_SIZE);
    let gas_diffuse = images.add(gas_img);

    let gas_normal_pixels = generate_normal_map(
        images.get(&gas_diffuse).unwrap().data.as_deref().unwrap(),
        TEX_SIZE,
        TEX_SIZE,
        2.0,
    );
    let gas_normal_img = build_image(gas_normal_pixels, TEX_SIZE, TEX_SIZE);
    let gas_normal = images.add(gas_normal_img);

    // --- Ice ---
    let ice_pixels = generate_ice_diffuse();
    let ice_img = build_image(ice_pixels, TEX_SIZE, TEX_SIZE);
    let ice_diffuse = images.add(ice_img);

    let ice_normal_pixels = generate_normal_map(
        images.get(&ice_diffuse).unwrap().data.as_deref().unwrap(),
        TEX_SIZE,
        TEX_SIZE,
        2.0,
    );
    let ice_normal_img = build_image(ice_normal_pixels, TEX_SIZE, TEX_SIZE);
    let ice_normal = images.add(ice_normal_img);

    // --- Star ---
    let star_pixels = generate_star_diffuse();
    let star_img = build_image(star_pixels, TEX_SIZE, TEX_SIZE);
    let star_diffuse = images.add(star_img);

    let star_normal_pixels = generate_normal_map(
        images.get(&star_diffuse).unwrap().data.as_deref().unwrap(),
        TEX_SIZE,
        TEX_SIZE,
        2.0,
    );
    let star_normal_img = build_image(star_normal_pixels, TEX_SIZE, TEX_SIZE);
    let star_normal = images.add(star_normal_img);

    // Store all handles in a resource
    commands.insert_resource(TextureAssets {
        rocky_diffuse,
        rocky_normal,
        gas_diffuse,
        gas_normal,
        ice_diffuse,
        ice_normal,
        star_diffuse,
        star_normal,
    });
}

// ============================================================================
// TexturePlugin
// ============================================================================

pub struct TexturePlugin;

impl Plugin for TexturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, generate_texture_system);
    }
}
