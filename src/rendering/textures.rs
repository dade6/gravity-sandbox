use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::components::lighting::GlowCurve;

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
pub(crate) fn build_image(pixels: Vec<u8>, width: u32, height: u32) -> Image {
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

/// Disco bianco (RGBA, alpha = cerchio) con bordo ANTIALIASATO su 1 texel.
/// Usato come immagine base delle sprite dei corpi: la silhouette del corpo è
/// l'alpha di questa texture, quindi la sua risoluzione deve stare al passo
/// col raggio in unità mondo (`custom_size = radius*2`) — vedi
/// `texture_size_for_radius` in firefly_bridge.rs.
pub(crate) fn generate_disc_texture(size: usize) -> Image {
    generate_disc_texture_with(size, false)
}

/// Numero di spicchi del disco di DIAGNOSI (vedi `generate_disc_texture_with`).
pub(crate) const DIAG_SECTORS: usize = 16;

/// Disco con eventuale TEXTURE A SPICCHI per la diagnosi del bordo.
///
/// `spicchi = false` → disco bianco come sempre (la sprite è tinta col colore
/// del corpo, quindi in scena appare monocolore).
///
/// `spicchi = true` → 16 spicchi alternati BIANCO / ROSSO. Serve a rispondere a
/// una domanda sola: **il filo chiaro sul bordo del pianeta è albedo o luce?**
/// - Il colore degli spicchi arriva dal pixel della sprite e viene MOLTIPLICATO
///   dalla lightmap: è **albedo**.
/// - Il colore della stella arriva dalla lightmap (`light_color`): è **luce**.
/// Guardando il bordo: se il filo è ROSSO dove c'è lo spicchio rosso, il
/// colore viene dall'albedo (texture/maschera). Se resta GIALLO anche sopra lo
/// spicchio rosso, la luce non sta moltiplicando quell'albedo ma **si somma o
/// sovrascrive** sopra di essa.
///
/// La copertura dell'alpha è IDENTICA al disco normale (stessa `radius` e
/// stessa formula), così la silhouette e il bordo antialiasato non cambiano:
/// altrimenti il test confronterebbe due cose insieme.
pub(crate) fn generate_disc_texture_with(size: usize, spicchi: bool) -> Image {
    let mut pixels = vec![0u8; size * size * 4];
    let center = (size as f32 - 1.0) / 2.0;
    // v0.14.202 — NON toccare il disco: il bordo non coperto lo chiude
    // `occluder_radius` in firefly_bridge.rs, allargando l'occluder di 1.5
    // texel. Cambiare qui il raggio a `size/2` rendeva il disco asimmetrico
    // (un lato con copertura piena, l'altro no) e lasciava comunque 1-1.5
    // texel scoperti: il margine sull'occluder risolve su tutti i bucket.
    let radius = size as f32 / 2.0 - 1.0;
    let settore = std::f32::consts::TAU / DIAG_SECTORS as f32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            // Coverage su un texel: senza questo lo scalino della texture si
            // vede appena il disco supera ~100 px a schermo.
            let coverage = (radius + 0.5 - dist).clamp(0.0, 1.0);
            let i = (y * size + x) * 4;
            if spicchi {
                // atan2 in [0, TAU); gli spicchi pari sono bianchi, i dispari
                // rossi. Il colore resta pieno: la sprite lo moltiplica per la
                // tinta del corpo, quindi in scena si vede "corpo × bianco" e
                // "corpo × rosso" — due albedi ben distinti.
                let ang = dy.atan2(dx).rem_euclid(std::f32::consts::TAU);
                let idx = (ang / settore) as usize % DIAG_SECTORS;
                if idx % 2 == 0 {
                    pixels[i] = 255;
                    pixels[i + 1] = 255;
                    pixels[i + 2] = 255;
                } else {
                    pixels[i] = 255;
                    pixels[i + 1] = 0;
                    pixels[i + 2] = 0;
                }
            } else {
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
            }
            pixels[i + 3] = (coverage * 255.0) as u8;
        }
    }
    build_image(pixels, size as u32, size as u32)
}

/// Radial soft-glow texture (luminous source halo): tinted white (the sprite
/// `color` applies the star tint), with alpha `(1 - t)^falloff_exp` where
/// `t` is the normalised radius 0 (centre) → 1 (disc edge). The alpha ramps
/// to exactly 0 within `curve.soft_edge` of the rim, so no hard disc edge is
/// visible on a black background. Shared by every star (the curve is a GLOBAL
/// preset parameter, not per-star).
pub(crate) fn generate_radial_glow_texture(size: u32, curve: &GlowCurve) -> Image {
    let w = size as usize;
    let half = size as f32 / 2.0;
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let dx = x as f32 + 0.5 - half;
            let dy = y as f32 + 0.5 - half;
            let d = (dx * dx + dy * dy).sqrt() / half; // 0 centre, 1 disc edge
            let a = (curve.alpha_at(d) * 255.0) as u8;
            pixels.extend_from_slice(&[255u8, 255u8, 255u8, a]);
        }
    }
    build_image(pixels, size, size)
}

/// Pendenza massima della normale sul bordo del disco: `nz` non scende sotto
/// questa frazione di `√(dx²+dy²)`, cioè la normale non supera ~70° dalla
/// direzione di vista. Vedi `generate_sphere_normal_map`.
///
/// **v0.14.207**: è ciò che impedisce a `normal_multi` di spegnersi sul bordo
/// antialiasato (dove `dx → 1` e `nz → 0`), che era la causa del filo colorato
/// del pianeta. Valore scelto perché `dot(n, luce)` al bordo resti ≥ ~0.35
/// anche con `normal_attenuation = 0.2`.
pub(crate) const NORMAL_RIM_Z_MIN: f32 = 0.35;

/// Genera i pixel RGBA (non-sRGB) della normal map di una sfera: per ogni
/// pixel dentro il disco, normale = normalize(x, y, √(1−x²−y²)) codificata
/// come RGB = normal·0.5+0.5 (convenzione standard, luce da alto-sinistra).
/// Fuori dal disco: alpha 0 (nessuna superficie). Non-sRGB per costruzione
/// (Rgba8Unorm) — caricare una normal map da PNG introduceva gamma errori
/// e crash async su WASM, quindi la generiamo in codice.
pub(crate) fn generate_sphere_normal_map(size: usize) -> Vec<u8> {
    let mut pixels = vec![0u8; size * size * 4];
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 / 2.0 - 1.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - center) / radius;
            // dy con segno ORIGINALE (ny crescente verso il basso nel file):
            // il test diagnostico v0.14.52 ha dimostrato che la texture arriva
            // RIBALTATA verticalmente in GPU (con ny=+1 in alto nel PNG il
            // ciano compare in BASSO sullo schermo). Con ny=+1 nelle righe
            // basse, dopo il flip lo schermo mostra ny=+1 in ALTO -> il lato
            // verso la stella (in alto) si illumina, ombra in basso.
            let dy = (y as f32 - center) / radius;
            let i = (y * size + x) * 4;
            // v0.14.203 — LA NORMAL MAP DEVE COPIRE TUTTO IL DISCO.
            // Prima `r2 > 1.0` (test binario sulla sfera) lasciava fuori i
            // texel di bordo che il DISCO invece disegna: il disco usa
            // `coverage = radius + 0.5 - dist`, quindi copre anche l'ultimo
            // texel con alpha parziale. Misurato a size=128: 152 texel hanno
            // disco > 0 ma nessuna normale (coverage 0.045-0.456).
            // Nella shader un pixel senza superficie (normal.b == 0) salta
            // l'ombra e resta illuminato: quei 152 texel formavano l'anello
            // chiaro visibile sul bordo del pianeta.
            // Ora la normal map si estende fin dove arriva il disco.
            // ATTENZIONE: `dx` e `dy` sono GIÀ divisi per `radius`, quindi per
            // allargare il disco fino a `radius + 0.5` si scala per il RAPPORTO
            // dei raggi, non si divide per `rr²` (che non è normalizzato: dava
            // r2 = 0.000124 e rendeva opaco tutto il buffer).
            let r2_disco = {
                let scala = radius / (radius + 0.5);
                (dx * dx + dy * dy) * scala * scala
            };
            if r2_disco > 1.0 {
                // Fuori dal disco: nessuna normale (nero, alpha 0)
                pixels[i + 3] = 0;
                continue;
            }
            let r2c = r2_disco.min(1.0);
            // v0.14.207 — LA NORMALE SUL BORDO NON DEVE ESSERE DI TAGLIO.
            //
            // `normalize(dx, dy, z)` con `dx → 1` dà `nz → 0`: la normale giace
            // nel piano e `dot(n, luce) ≈ 0`. Sul bordo antialiasato questo
            // spegneva `normal_multi` (che MOLTIPLICA la lightmap), lasciando
            // il pixel fuori dalla moltiplicazione: la texture tornava visibile
            // come filo colorato (riga 144 su fondo 19, con `Planet Light = 0`).
            //
            // Pavimento su `nz`: la normale non supera ~70° dalla direzione di
            // vista, quindi il bordo riceve luce come una superficie sferica
            // reale (non come un bordo infinitamente sottile).
            //
            // Quantità MOLTIPLICATIVA, non additiva: `z_naive` è già in [0,1] e
            // il pavimento è `NORMAL_RIM_Z_MIN · √r2`, quindi `max(...)`
            // mantiene il valore sempre <= 1 e la normale resta un versore
            // (i canali RGB restano nella convenzione ×0.5+0.5).
            let z = (1.0 - r2c).max(0.0).sqrt().max(NORMAL_RIM_Z_MIN * (dx * dx + dy * dy).sqrt());
            let len = (dx * dx + dy * dy + z * z).max(0.0001).sqrt();
            let (nx, ny, nz) = (dx / len, dy / len, z / len);
            pixels[i] = ((nx * 0.5 + 0.5) * 255.0) as u8;
            pixels[i + 1] = ((ny * 0.5 + 0.5) * 255.0) as u8;
            pixels[i + 2] = ((nz * 0.5 + 0.5) * 255.0) as u8;
            pixels[i + 3] = 255;
        }
    }
    pixels
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
                (139, 94, 60) // brown
            } else if n < 0.6 {
                (193, 68, 14) // red-orange
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

// ============================================================================
// Test generazione normal map sfera (salva il PNG per ispezione visiva)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// v0.14.207 — LA NORMAL MAP DEVE COPIRE OGNI TEXEL CHE ENTRA NELLO STENCIL.
    ///
    /// Il difetto: lo stencil dei corpi scrive dal .190 con soglia 0.004
    /// (bordo antialiasato incluso), ma la normal map restava a `color.a >= 1.0`.
    /// Sui texel di bordo `res.normal` era `vec4(0)` → la shader trattava il
    /// pixel come "senza superficie", la lightmap non lo moltiplicava, e la
    /// texture del pianeta tornava visibile come filo colorato (a spicchi).
    ///
    /// Il test usa la STESSA soglia dello stencil (0.004) e pretende che ogni
    /// texel sopra soglia abbia anche una normale non nulla.
    #[test]
    fn normal_map_covers_every_stencil_texel() {
        use super::generate_disc_texture;
        for size in [256usize, 512] {
            let disc = generate_disc_texture(size);
            let d = disc.data.as_ref().expect("disco").clone();
            let n = generate_sphere_normal_map(size);
            // Lo stencil scrive se `color.a >= 0.004` (STENCIL_MIN_ALPHA in
            // sprite.wgsl). alpha 0.004 su 255 = 1.
            let mut stencil_senza_normale = 0;
            let mut primo = None;
            for y in 0..size {
                for x in 0..size {
                    let i = (y * size + x) * 4;
                    let a_disc = d[i + 3];
                    let a_norm = n[i + 3];
                    if a_disc >= 1 && a_norm == 0 {
                        stencil_senza_normale += 1;
                        primo.get_or_insert((x, y, a_disc));
                    }
                }
            }
            assert_eq!(
                stencil_senza_normale, 0,
                "size {size}: {stencil_senza_normale} texel entrano nello stencil \
                 ma non hanno la normal map (primo {primo:?}) — la lightmap non li \
                 moltiplica e la texture torna visibile sul bordo"
            );
        }
    }

    /// La normal map al bordo NON deve essere di taglio: `nz` deve restare
    /// sopra il pavimento, altrimenti `dot(n, luce) = 0` e `normal_multi`
    /// spegne la lightmap proprio sul bordo.
    #[test]
    fn rim_normal_is_not_edge_on() {
        let size = 256usize;
        let n = generate_sphere_normal_map(size);
        let center = (size as f32 - 1.0) / 2.0;
        let radius = size as f32 / 2.0 - 1.0;
        // Versante verso la stella: riga centrale, colonna a sinistra.
        let y = size / 2;
        let mut minimo = 1.0f32;
        let mut trovati = 0;
        for x in 0..size {
            let i = (y * size + x) * 4;
            if n[i + 3] == 0 {
                continue;
            }
            let nz = (n[i + 2] as f32 / 255.0) * 2.0 - 1.0;
            minimo = minimo.min(nz);
            trovati += 1;
        }
        assert!(trovati > 0, "nessun texel di disco sulla riga centrale");
        // `nz` codificato: 0.35/sqrt(1+0.35²) = 0.33 -> ~170 -> 0.33 in [-1,1]
        assert!(
            minimo > 0.25,
            "la normale scende a nz={minimo:.3} sul bordo: è di taglio e \
             normal_multi spegne la luce esattamente lì"
        );
        let _ = (center, radius);
    }
    /// a spicchi: se cambiasse, il test diagnostico confronterebbe due cose
    /// insieme (colore E silhouette) e non direbbe niente sul bordo.
    #[test]
    fn spicchi_preserve_disc_coverage() {
        for size in [64usize, 128, 256] {
            let normale = generate_disc_texture_with(size, false);
            let spicchi = generate_disc_texture_with(size, true);
            let a = normale.data.expect("disco normale");
            let b = spicchi.data.expect("disco spicchi");
            let mut diversi = 0;
            for i in (3..a.len()).step_by(4) {
                if a[i] != b[i] {
                    diversi += 1;
                }
            }
            assert_eq!(diversi, 0, "size {size}: {diversi} texel con alpha diverso");
        }
    }

    /// Ogni spicchio deve cambiare colore e devono essercene esattamente
    /// `DIAG_SECTORS` alternati: un disco tutto bianco non discriminerebbe
    /// albedo da luce.
    #[test]
    fn spicchi_alternate_white_and_red_sixteen_times() {
        let size = 256usize;
        let img = generate_disc_texture_with(size, true);
        let px = img.data.expect("spicchi");
        let center = (size as f32 - 1.0) / 2.0;
        // Anello a metà raggio: campiona un colore per settore.
        let r = size as f32 / 4.0;
        let mut settori: Vec<(u8, u8)> = Vec::new();
        for k in 0..DIAG_SECTORS {
            let ang = std::f32::consts::TAU * (k as f32 + 0.5) / DIAG_SECTORS as f32;
            let x = (center + r * ang.cos()) as usize;
            let y = (center + r * ang.sin()) as usize;
            let i = (y * size + x) * 4;
            settori.push((px[i + 1], px[i + 2])); // G, B (R è 255 in entrambi)
        }
        for k in 0..DIAG_SECTORS {
            let (g, b) = settori[k];
            let atteso_bianco = k % 2 == 0;
            let is_bianco = g > 200 && b > 200;
            let is_rosso = g < 60 && b < 60;
            assert!(
                if atteso_bianco { is_bianco } else { is_rosso },
                "settore {k}: rgb(G={g},B={b}) atteso {}",
                if atteso_bianco { "bianco" } else { "rosso" }
            );
        }
    }

    /// Verifica che la normal map della sfera sia matematicamente corretta:
    /// - centro: punta verso l'osservatore (RGB ≈ (128,128,255), blu puro)
    /// - bordo sinistro: punta a sinistra (R=0, G≈128, B≈128)
    /// - bordo destro: punta a destra (R≈255, G≈128, B≈128)
    /// - fuori dal disco: alpha 0 (trasparente)
    /// - alpha 255 dentro la sfera
    /// Salva anche `sphere_normal_generated.png` accanto al target dir
    /// perché Davide possa ispezionarla visivamente.
    #[test]
    fn sphere_normal_map_is_correct_and_saves_png() {
        let size = 256usize;
        let pixels = generate_sphere_normal_map(size);

        assert_eq!(pixels.len(), size * size * 4, "buffer RGBA completo");

        // Centro: normale ≈ (0,0,1) -> RGB ≈ (128,128,255), alpha 255.
        // Tolleranza ±2: il pixel centrale è a mezzo pixel dal centro esatto
        // (discretizzazione), quindi nz ≈ 0.99998 -> 254.99 -> 254.
        let c = size / 2;
        let i = (c * size + c) * 4;
        assert!(
            (pixels[i] as i32 - 128).abs() <= 2,
            "centro R≈128 (nx=0), got {}",
            pixels[i]
        );
        assert!(
            (pixels[i + 1] as i32 - 128).abs() <= 2,
            "centro G≈128 (ny=0), got {}",
            pixels[i + 1]
        );
        assert!(
            (pixels[i + 2] as i32 - 255).abs() <= 2,
            "centro B≈255 (nz=1), got {}",
            pixels[i + 2]
        );
        assert_eq!(pixels[i + 3], 255, "centro alpha pieno");

        // Gradiente R (normale x): cresce da sinistra a destra.
        // Non si testano valori assoluti sui bordi: il bordo esatto (dx=±1)
        // non cade su un pixel intero, quindi un pixel "di bordo" ha già
        // z>0 (es. B=138 invece di 128) — comportamento corretto.
        let l = (c * size + (c - 30)) * 4;
        let r = (c * size + (c + 30)) * 4;
        assert!(
            pixels[l] < pixels[r],
            "R sinistra ({}) < R destra ({})",
            pixels[l],
            pixels[r]
        );

        // Gradiente G (normale y): cresce dall'alto in basso nel FILE — la texture
        // viene ribaltata verticalmente in GPU (flip di sampling), quindi
        // sullo schermo ny=+1 (ciano) finisce in ALTO: polo illuminato verso
        // la stella. (Confermato dal test diagnostico v0.14.52: ciano in alto
        // nel file -> ciano in basso sullo schermo.)
        let t = ((c - 30) * size + c) * 4;
        let b = ((c + 30) * size + c) * 4;
        assert!(
            pixels[t + 1] < pixels[b + 1],
            "G sopra ({}) < G sotto ({})",
            pixels[t + 1],
            pixels[b + 1]
        );

        // B (normale z) massimo al centro, minore ai bordi.
        let e = (c * size + (c - 30)) * 4;
        assert!(
            pixels[e + 2] < pixels[i + 2],
            "B bordo ({}) < B centro ({})",
            pixels[e + 2],
            pixels[i + 2]
        );

        // Angolo alto-sinistra: fuori dal disco -> alpha 0
        let corner = 0usize;
        assert_eq!(pixels[corner + 3], 0, "angolo fuori sfera trasparente");

        // Angolo dentro al disco (es. (size/4, size/4)): alpha 255
        let q = size / 4;
        let qi = (q * size + q) * 4;
        assert_eq!(pixels[qi + 3], 255, "quadrante dentro sfera opaco");

        // Salva il PNG per ispezione visiva (accanto al workspace spike)
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sphere_normal_generated.png");
        image::save_buffer(
            &path,
            &pixels,
            size as u32,
            size as u32,
            image::ExtendedColorType::Rgba8,
        )
        .expect("salvataggio PNG");
        eprintln!("NORMAL MAP SALVATA: {}", path.display());
    }
}
