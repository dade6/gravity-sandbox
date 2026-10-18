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

/// Polvere: la stella SENZA alone+croce, cioè quella che a zoom out estremo
/// diventa sotto-pixel. Porta l'indice nella cella, così il livello di
/// dettaglio (`SkyLod`) può tenerne una su N con lo stesso criterio sia allo
/// spawn sia quando lo zoom cambia.
#[derive(Component, Clone, Copy, Debug)]
pub struct Dust {
    pub index: u32,
}

/// Livello di dettaglio del cielo, deciso dallo ZOOM.
///
/// A zoom out estremo un settore copre pochi pixel: la polvere (punti di
/// 0.6-5 unità mondo) è sotto il pixel e non si vede, ma il cielo passa da
/// ~1.100 a ~18.800 sprite — 17× il lavoro (estra­zione, pass stencil, pass
/// sprite) per zero pixel visibili. Questo è il motivo per cui un Mac a zoom
/// out estremo scatta: le stesse entità costano di più a risoluzione Retina.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SkyLod(pub u8);

/// Sotto il pixel: un settore così piccolo a schermo rende invisibile la
/// polvere. Soglie sul lato del settore A SCHERMO (`sector_size / zoom`).
const LOD_DUST_QUARTER_PX: f32 = 400.0;
const LOD_BIG_ONLY_PX: f32 = 120.0;

/// 0 = tutto, 1 = una polvere su 4, 2 = solo le stelle grandi.
fn sky_lod(zoom: f32, sector_size: f32) -> u8 {
    let px = sector_size / zoom.max(1e-4);
    if px <= LOD_BIG_ONLY_PX {
        2
    } else if px <= LOD_DUST_QUARTER_PX {
        1
    } else {
        0
    }
}

/// Questa polvere si vede a questo livello di dettaglio? Indice STABILE
/// (`index % 4`) così cambiando zoom non si rimescola il cielo.
fn dust_visible(index: u32, lod: u8) -> bool {
    match lod {
        0 => true,
        1 => index % 4 == 0,
        _ => false,
    }
}

/// Parallasse PER STELLA: una stella non si muove col fattore del layer ma con
/// il suo. `base` è la posizione in spazio-base del layer (dentro il settore),
/// `factor` la sua parallasse e `layer_factor` quella del settore che la
/// contiene; la posizione locale è `base + cam·(factor − layer_factor)`, così
/// il mondo vale `base + cam·factor` — la stella scivola dentro il settore
/// mentre la camera si sposta, come se fosse a una profondità sua.
#[derive(Component, Clone, Copy, Debug)]
pub struct StarDrift {
    pub base: Vec2,
    pub factor: f32,
    pub layer_factor: f32,
}

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
            .init_resource::<StarfieldTuning>()
            .init_resource::<SkyLod>()
            .init_resource::<StarSectors>()
            .add_systems(Startup, setup_starfield)
            // L'ordine conta: applicare il tuning (che rigenera le texture e
            // butta giù i settori) PRIMA di streammare, così il nuovo cielo
            // compare nello stesso frame in cui si conferma un valore. Il
            // drift per stella viene per ultimo (le stelle nuove esistono
            // solo dopo il sync dei comandi).
            .add_systems(
                Update,
                (
                    refresh_starfield_on_tuning_change,
                    stream_starfield,
                    apply_sky_lod,
                    update_star_drift,
                )
                    .chain(),
            );
    }
}

// ============================================================
// Tuning — ogni parametro dello sfondo, regolabile a caldo
// ============================================================

/// Tunables of ONE layer.
///
/// Ogni layer è un cielo COMPLETO: contiene sia polvere (puntini piatti) sia
/// stelle grandi (alone + croce), con dimensione, colore e parallasse che
/// variano stella per stella. I layer si distinguono per DENSITÀ e per la
/// profondità media (parallasse), non per il tipo di stella — "il livello
/// delle piccole" e "il livello delle grandi" è sbagliato: in cielo le stelle
/// grandi stanno a tutte le distanze. `z` non è qui: la profondità di render
/// è strutturale (vedi `LAYER_Z`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerTuning {
    /// Stars generated in every sector of this layer. Density is what
    /// matters (`stars / sector_size²`), so resizing the sectors requires
    /// rescaling this.
    pub stars: u32,
    /// Estremo PICCOLO della gamma di dimensione (la polvere).
    pub radius_min: f32,
    /// Estremo GRANDE della gamma (le stelle più grandi del layer).
    pub radius_max: f32,
    /// Sbilanciamento della distribuzione delle dimensioni: > 1 concentra le
    /// stelle verso `radius_min` (tanta polvere, poche stelle grandi).
    pub size_power: f32,
    /// Quota di stelle che riceve alone + croce di diffrazione (0..1). Sono
    /// sempre le più grandi della gamma.
    pub big_chance: f32,
    /// Parallasse MEDIA del layer (1.0 = incollato alla camera).
    pub factor: f32,
    /// Oscillazione della parallasse attorno a `factor`, stella per stella
    /// (0 = tutte alla stessa profondità). È ciò che evita che un layer sia
    /// "a una sola distanza": nel cielo le stelle hanno profondità diverse.
    pub factor_spread: f32,
    /// Multiplier on this layer's HDR tints (1.0 = reference look).
    pub brightness: f32,
}

/// Every knob of the sky, live-editable from the "Sfondo" panel.
///
/// The whole field is a pure function of these values plus the sector
/// coordinates, so a change simply rebuilds the sky: sectors are dropped and
/// the streaming system respawns them with the new numbers (same seed → same
/// stars, new look). Defaults are the shipped reference look — see
/// `defaults_match_the_reference_look`.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct StarfieldTuning {
    pub layers: [LayerTuning; 3],
    /// Side of one sky sector, world units.
    pub sector_size: f32,
    /// World-space margin used when *spawning* sectors: keeps the seam
    /// off-screen without paying for a whole extra ring of cells.
    pub spawn_margin: f32,
    /// Margin used when *keeping* sectors alive. Larger than the spawn
    /// margin so a cell sitting exactly on the boundary is not
    /// spawned/despawned every frame while the camera jitters (hysteresis).
    pub keep_margin: f32,
    /// Upper bound on live sectors per layer. Only binds at extreme
    /// zoom-out: 100 sectors = a 10×10 grid = 10000×10000 units of sky.
    /// Zooming out further shows empty sky past that radius.
    pub max_sectors: usize,
    /// Probability that a sector of the near layer also gets a galaxy.
    pub galaxy_chance: f32,
    /// Galaxy size range (world units).
    pub galaxy_size_min: f32,
    pub galaxy_size_max: f32,
    /// Quad of a dust dot = its diameter × this.
    pub dot_scale: f32,
    /// Quad of a big star = its diameter × this (room for halo + cross).
    pub spike_scale: f32,
    /// Saturated core, as a fraction of the big star's quad radius.
    pub spike_core: f32,
    /// Esponente del falloff dell'alone (nucleo -> bordo). Valori alti = alone
    /// STRETTO attorno al nucleo: con 1.6 l'alone si allargava fino a ~2.2
    /// volte il raggio della stella e "mangiava" la croce; a 4.0 il bagliore
    /// resta adeso al nucleo e i bracci si leggono netti.
    pub spike_bloom_exp: f32,
    /// Cross width AT THE CORE, fraction of the quad radius (~1px on screen
    /// for big stars). Thinner than that shimmers while panning and disappears.
    pub spike_width: f32,
    /// Rastremazione dei bracci verso la punta (0..0.95): la larghezza cala
    /// di `spike_width * (1 - spike_shrink * d)`. A 0 i bracci sono barre a
    /// larghezza COSTANTE (sbagliato: una punta di diffrazione nasce larga al
    /// nucleo e finisce a spillo).
    pub spike_shrink: f32,
    /// Spike intensity.
    pub spike_strength: f32,
    /// Spike taper towards the rim: < 1 gives long arms to the quad edge.
    pub spike_taper_exp: f32,
}

/// Render depth per layer: far behind any planet (`sync_sprite_z` writes
/// planets at `z = -distance`), and inside the custom far plane (±1e6,
/// see `main_camera_projection`).
pub(crate) const LAYER_Z: [f32; 3] = [-500_000.0, -499_000.0, -498_000.0];

/// Stars per 1000×1000 sector = 1.25e-4 / 5e-5 / 1.3e-5 per unit², i.e. the
/// density of the previous 10000×10000 box (12500 / 5000 / 1250 stars), so
/// swapping the finite box for streamed sectors does not change the look.
///
/// Le gamme di dimensione si SOVRAPPONGONO (tutti i layer hanno polvere E
/// stelle grandi): il layer lontano è denso di polvere, quello vicino è più
/// rado ma con stelle mediamente più grandi e brillanti.
impl Default for StarfieldTuning {
    fn default() -> Self {
        Self {
            layers: [
                LayerTuning {
                    stars: 125,
                    radius_min: 0.30,
                    radius_max: 2.4,
                    size_power: 2.2,
                    big_chance: 0.05,
                    factor: 1.0,
                    factor_spread: 0.02,
                    brightness: 1.0,
                },
                LayerTuning {
                    stars: 50,
                    radius_min: 0.40,
                    radius_max: 3.0,
                    size_power: 2.0,
                    big_chance: 0.07,
                    factor: 0.95,
                    factor_spread: 0.04,
                    brightness: 1.2,
                },
                LayerTuning {
                    stars: 13,
                    radius_min: 0.55,
                    radius_max: 4.0,
                    size_power: 1.8,
                    big_chance: 0.12,
                    factor: 0.9,
                    factor_spread: 0.06,
                    brightness: 1.45,
                },
            ],
            sector_size: 1000.0,
            spawn_margin: 250.0,
            keep_margin: 1000.0,
            max_sectors: 100,
            galaxy_chance: 0.22,
            galaxy_size_min: 60.0,
            galaxy_size_max: 150.0,
            dot_scale: 1.0,
            spike_scale: 3.2,
            spike_core: 0.18,
            spike_bloom_exp: 4.0,
            spike_width: 0.080,
            spike_shrink: 0.65,
            spike_strength: 0.8,
            spike_taper_exp: 0.5,
        }
    }
}

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

/// Puntino piatto: disco saturo con bordo netto (1px di antialiasing).
/// Nessun alone, quindi nessun parametro: il suo quad coincide col diametro
/// dichiarato (`dot_scale` nel tuning vale 1.0).
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

/// Stella grande: nucleo saturo + alone + croce di diffrazione a 4 punte.
///
/// Ogni numero della forma viene dal tuning (`spike_core`, `spike_bloom_exp`,
/// `spike_width`, `spike_strength`, `spike_taper_exp`): sono le manopole del
/// pannello "Sfondo". Vedi `StarfieldTuning` per il significato e le
/// trappole (alone grasso = croce invisibile).
fn star_spike_pixels(t: &StarfieldTuning) -> Vec<u8> {
    let w = STAR_TEX_SIZE as usize;
    let half = STAR_TEX_SIZE as f32 / 2.0;
    let core = t.spike_core.clamp(0.01, 0.9);
    let mut pixels = Vec::with_capacity(w * w * 4);
    for y in 0..w {
        for x in 0..w {
            let ax = (x as f32 + 0.5 - half) / half;
            let ay = (y as f32 + 0.5 - half) / half;
            let d = (ax * ax + ay * ay).sqrt().min(1.0);
            // Nucleo + alone.
            let bloom = if d <= core {
                1.0
            } else {
                let k = (d - core) / (1.0 - core);
                (1.0 - k).powf(t.spike_bloom_exp.max(0.05))
            };
            // Croce: due lobi gaussiani lungo X e Y, che si attenuano con la
            // distanza dal centro (il massimo è al centro, come nella realtà).
            // Il decadimento è LENTO (spike_taper_exp < 1) così le punte
            // restano più luminose dell'alone verso il bordo del quad.
            let taper = (1.0 - d).powf(t.spike_taper_exp.max(0.05));
            // Larghezza del braccio: massima al nucleo, rastremata verso il
            // bordo (mai sotto ~0.12 per non finire sotto il pixel).
            let width = (t.spike_width.clamp(0.005, 0.5)
                * (1.0 - t.spike_shrink.clamp(0.0, 0.95) * d))
            .max(0.012);
            let gauss = |o: f32| (-(o * o) / (width * width)).exp();
            let spike = t.spike_strength * taper * gauss(ay).max(gauss(ax));
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

/// Build (or rebuild) the shared sky textures from the current tuning.
/// Cheap: two 64×64 and one 128×128 procedural image, ~40KB of work.
fn build_starfield_textures(
    images: &mut Assets<Image>,
    tuning: &StarfieldTuning,
) -> StarfieldTextures {
    StarfieldTextures {
        dot: images.add(crate::rendering::textures::build_image(
            star_dot_pixels(),
            STAR_TEX_SIZE,
            STAR_TEX_SIZE,
        )),
        spike: images.add(crate::rendering::textures::build_image(
            star_spike_pixels(tuning),
            STAR_TEX_SIZE,
            STAR_TEX_SIZE,
        )),
        galaxy: images.add(crate::rendering::textures::build_image(
            galaxy_pixels(),
            GALAXY_TEX_SIZE,
            GALAXY_TEX_SIZE,
        )),
    }
}

fn setup_starfield(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    tuning: Res<StarfieldTuning>,
) {
    let textures = build_starfield_textures(&mut images, &tuning);
    commands.insert_resource(textures);
}

/// Live-apply a tuning change: rebuild the shared textures and drop every
/// live sector so the streaming system respawns them with the new numbers
/// (same seeds → same stars, new look). Cheap to get wrong, hence the
/// explicit ordering before `stream_starfield` in the plugin.
fn refresh_starfield_on_tuning_change(
    mut commands: Commands,
    tuning: Res<StarfieldTuning>,
    mut images: ResMut<Assets<Image>>,
    mut sectors: ResMut<StarSectors>,
) {
    if !tuning.is_changed() {
        return;
    }
    crate::mark_system("refresh_starfield_on_tuning_change");
    // Textures first: new sprites must not be built against stale handles.
    let textures = build_starfield_textures(&mut images, &tuning);
    commands.insert_resource(textures);
    // Then the sky itself.
    for (_, entity) in sectors.0.drain() {
        commands.entity(entity).despawn();
    }
}

// ============================================================
// Star look
// ============================================================

/// Come appare UNA stella: dimensione, tinta, se ha alone+croce, e la SUA
/// parallasse.
#[derive(Clone, Copy, Debug)]
struct StarSpec {
    radius: f32,
    color: Color,
    is_big: bool,
    factor: f32,
}

/// Estrae una stella dalla gamma del layer.
///
/// Regola di fondo: la gamma di dimensione copre TUTTO, da polvere a stella
/// grande, e `size_power` decide quanto è sbilanciata verso il piccolo (nel
/// cielo vero: tanta polvere, poche stelle grandi). `big_chance` non sceglie
/// una classe a parte: decide quante stelle ricevono alone + croce, e quelle
/// stanno sempre in cima alla gamma.
///
/// OGNI stella ha inoltre la sua parallasse (`factor` ± `factor_spread`) e la
/// sua tinta (`payload` di temperatura pesato), così un layer non è mai "a una
/// sola distanza e un solo colore".
///
/// Il NUMERO di estrazioni RNG è FISSO (7) qualunque siano i parametri: così
/// cambiare dimensione/colore/parallasse dal pannello NON sposta le stelle
/// (le posizioni vengono estratte dopo), e il confronto A/B è possibile.
fn star_look(rng: &mut StdRng, layer: usize, t: &StarfieldTuning) -> StarSpec {
    let cfg = &t.layers[layer];
    let rmin = cfg.radius_min.max(0.01);
    let rmax = cfg.radius_max.max(rmin + 0.05);
    // 7 estrazioni, sempre nello stesso ordine.
    let u = rng.gen_range(0.0f32..1.0);
    let big_roll = rng.gen_bool(cfg.big_chance.clamp(0.0, 1.0) as f64);
    let level_jitter = rng.gen_range(0.9f32..1.1);
    let pick = rng.gen_range(0.0f32..1.0);
    let j1 = rng.gen_range(0.0f32..1.0);
    let j2 = rng.gen_range(0.0f32..1.0);
    let factor_jitter = rng.gen_range(-1.0f32..1.0);

    let mut size_norm = u.powf(cfg.size_power.clamp(0.05, 8.0));
    if big_roll {
        // Le stelle con croce stanno in cima alla gamma di dimensione.
        size_norm = size_norm.max(0.72);
    }
    let radius = rmin * (rmax / rmin).powf(size_norm);
    let level = cfg.brightness * (1.05 + 1.5 * size_norm) * level_jitter;
    let color = star_color(pick, j1.max(j2), level);
    let factor = if cfg.factor_spread > 0.0 {
        (cfg.factor + factor_jitter * cfg.factor_spread).clamp(0.0, 1.0)
    } else {
        cfg.factor
    };
    StarSpec {
        radius,
        color,
        is_big: big_roll,
        factor,
    }
}

/// Tinta HDR di una stella, data una temperatura pesata (`pick`), un
/// parametro di saturazione (`sat`) e la luminanza (`level`).
///
/// Le tinte sono OPACHE (alpha 1.0) e in HDR: il cielo è lavato dall'alone
/// del sole (Halo Brightness ~1), quindi una tinta più scura dello sfondo
/// rende la stella un puntino NERO — è esattamente così che il campo aveva
/// smesso di sembrare luminoso. Il canale più alto vale sempre `level`, che
/// resta sopra `SKY_WASH_REFERENCE` (test: `star_tints_outshine_the_washed_sky`).
///
/// La temperatura NON dipende dal layer (una stella blu può essere vicina o
/// lontana): la distribuzione è quella reale — molte fredde/blu-bianche,
/// parecchie bianche, poche gialle, rare arancioni.
fn star_color(pick: f32, sat: f32, level: f32) -> Color {
    // sat 0..1 → 0.90..1.10 sulla saturazione (variazione di colore).
    let s = 0.90 + 0.20 * sat.clamp(0.0, 1.0);
    let cool = |k: f32| (k * s).clamp(0.0, 1.0);
    if pick < 0.45 {
        // Blu-bianco (le più comuni).
        Color::srgb(level * cool(0.84), level * cool(0.91), level)
    } else if pick < 0.80 {
        // Bianco.
        Color::srgb(level, level * cool(0.96), level * cool(0.92))
    } else if pick < 0.95 {
        // Giallo-caldo.
        Color::srgb(level, level * cool(0.80), level * cool(0.55))
    } else {
        // Arancione-rossa (le più rare).
        Color::srgb(level, level * cool(0.68), level * cool(0.42))
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
fn sector_origin_world(grid: IVec2, factor: f32, cam: Vec2, sector_size: f32) -> Vec2 {
    grid.as_vec2() * sector_size + cam * factor
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
    t: &StarfieldTuning,
    lod: u8,
) -> Entity {
    let li = layer as usize;
    let cfg = t.layers[li];
    let mut rng = StdRng::seed_from_u64(seed_for(base_seed, layer, grid));

    let origin = sector_origin_world(grid, cfg.factor, cam, t.sector_size);
    let root = commands
        .spawn((
            StarSector {
                layer,
                grid,
                factor: cfg.factor,
            },
            Transform::from_xyz(origin.x, origin.y, LAYER_Z[li]),
            Visibility::default(),
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        for index in 0..cfg.stars {
            let spec = star_look(&mut rng, li, t);
            // Posizione in spazio-base del layer: il settore è solo un
            // contenitore, la stella ha una parallasse propria (StarDrift).
            let base = Vec2::new(
                rng.gen_range(0.0..t.sector_size),
                rng.gen_range(0.0..t.sector_size),
            );
            // Bimodal look: dust is a flat dot the size of its radius; the
            // big ones get the halo + diffraction cross, with the quad
            // scaled so the saturated core still matches the radius.
            let (image, quad_scale) = if spec.is_big {
                (textures.spike.clone(), t.spike_scale)
            } else {
                (textures.dot.clone(), t.dot_scale)
            };
            // La polvere nasce già col livello di dettaglio corrente: i comandi
            // sono deferred, quindi `apply_sky_lod` non la vedrebbe in tempo.
            let visible = spec.is_big || dust_visible(index, lod);
            // Posizionata già corretta al frame di spawn (i comandi sono
            // deferred: senza questo la cella nuova lampeggerebbe).
            let local = base + cam * (spec.factor - cfg.factor);
            let mut entity = parent.spawn((
                Sprite {
                    image,
                    color: spec.color,
                    custom_size: Some(Vec2::splat(spec.radius * 2.0 * quad_scale)),
                    ..default()
                },
                // Marks it for firefly's sprite shader: whole-quad stencil,
                // no normal map (see STARFIELD_HEIGHT).
                SpriteHeight(STARFIELD_HEIGHT),
                Transform::from_xyz(local.x, local.y, 0.0),
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                RenderLayers::layer(1),
                StarDrift {
                    base,
                    factor: spec.factor,
                    layer_factor: cfg.factor,
                },
                Star,
            ));
            if !spec.is_big {
                entity.insert(Dust { index });
            }
        }

        // Weenies: distant galaxies are navigation landmarks, so they live
        // on the NEAR layer, where they drift a little (they have some
        // parallax instead of being nailed to the screen).
        if li == t.layers.len() - 1 && rng.gen_bool(t.galaxy_chance as f64) {
            let size =
                rng.gen_range(t.galaxy_size_min..t.galaxy_size_max.max(t.galaxy_size_min + 1.0));
            let pos = Vec2::new(
                rng.gen_range(0.0..t.sector_size),
                rng.gen_range(0.0..t.sector_size),
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
fn sector_range(center: Vec2, half_view: Vec2, margin: f32, t: &StarfieldTuning) -> (IVec2, IVec2) {
    let side = (t.max_sectors as f32).sqrt().ceil() as i32;
    let max_span = side * 2 + 1;
    let reach = half_view + Vec2::splat(margin);
    let lo_f = (center - reach) / t.sector_size;
    let hi_f = (center + reach) / t.sector_size;
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
fn desired_sectors(center: Vec2, half_view: Vec2, margin: f32, t: &StarfieldTuning) -> Vec<IVec2> {
    let (lo, hi) = sector_range(center, half_view, margin, t);
    let mut cells: Vec<(f32, IVec2)> = Vec::new();
    for gy in lo.y..=hi.y {
        for gx in lo.x..=hi.x {
            let grid = IVec2::new(gx, gy);
            let d = grid.as_vec2() * t.sector_size + Vec2::splat(t.sector_size * 0.5) - center;
            cells.push((d.length_squared(), grid));
        }
    }
    cells.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    cells.truncate(t.max_sectors);
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
    tuning: Res<StarfieldTuning>,
    mut lod: ResMut<SkyLod>,
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

    // Livello di dettaglio del cielo (dipende dallo zoom, non dalla camera):
    // a zoom out estremo la polvere è sotto il pixel e si spegne.
    lod.set_if_neq(SkyLod(sky_lod(zoom, tuning.sector_size)));

    // 1) Which sectors should stay alive (KEEP_MARGIN, hysteresis) and
    //    which of the visible ones (SPAWN_MARGIN) are still missing.
    let mut wanted: HashSet<(u8, IVec2)> = HashSet::new();
    let mut missing: Vec<(u8, IVec2)> = Vec::new();
    for (li, cfg) in tuning.layers.iter().enumerate() {
        let layer = li as u8;
        let base = cam * (1.0 - cfg.factor);
        for grid in desired_sectors(base, half_view, tuning.keep_margin, &tuning) {
            wanted.insert((layer, grid));
        }
        for grid in desired_sectors(base, half_view, tuning.spawn_margin, &tuning) {
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
        let entity = spawn_sector(
            &mut commands,
            layer,
            grid,
            seed.0,
            cam,
            &textures,
            &tuning,
            lod.0,
        );
        sectors.0.insert((layer, grid), entity);
    }

    // 4) Place every live sector for this frame's parallax offset. Sectors
    //    spawned in step 3 are not in the World yet (deferred commands) and
    //    already carry the right Transform, so skipping them is correct.
    for ((li, grid), entity) in sectors.0.iter() {
        let cfg = &tuning.layers[*li as usize];
        if let Ok(mut tf) = roots.get_mut(*entity) {
            let origin = sector_origin_world(*grid, cfg.factor, cam, tuning.sector_size);
            tf.translation.x = origin.x;
            tf.translation.y = origin.y;
        }
    }
}

// ============================================================
// Pannello "Sfondo": chiavi -> parametri
// ============================================================
//
// Il pannello UI non conosce la struct: parla solo di chiavi `set_sf_*` (il
// prefisso `set_` le fa riconoscere al keypad numerico su iPhone, vedi
// keypad.rs::is_numeric_field). Qui stanno valore corrente e applicazione,
// con clamp e invarianti — esattamente come fa settings.rs per i globali.

/// `"l2_rmax"` -> `(1, "rmax")`. None se la chiave non è di un layer.
fn layer_key(key: &str) -> Option<(usize, &str)> {
    let rest = key.strip_prefix('l')?;
    let (n, field) = rest.split_at(1);
    let idx = n.parse::<usize>().ok()?;
    if !(1..=3).contains(&idx) {
        return None;
    }
    Some((idx - 1, field.strip_prefix('_')?))
}

/// Valore corrente di un campo `set_sf_*` (usato allo spawn del pannello).
pub(crate) fn starfield_field_value(key: &str, t: &StarfieldTuning) -> String {
    let Some(stripped) = key.strip_prefix("set_sf_") else {
        return String::new();
    };
    if let Some((li, field)) = layer_key(stripped) {
        let l = &t.layers[li];
        return match field {
            "factor" => format!("{:.3}", l.factor),
            "spread" => format!("{:.3}", l.factor_spread),
            "stars" => format!("{}", l.stars),
            "rmin" => format!("{:.2}", l.radius_min),
            "rmax" => format!("{:.2}", l.radius_max),
            "sizepow" => format!("{:.2}", l.size_power),
            "big" => format!("{:.2}", l.big_chance),
            "bright" => format!("{:.2}", l.brightness),
            _ => String::new(),
        };
    }
    match stripped {
        "sector" => format!("{:.0}", t.sector_size),
        "spawn_margin" => format!("{:.0}", t.spawn_margin),
        "keep_margin" => format!("{:.0}", t.keep_margin),
        "max_sectors" => format!("{}", t.max_sectors),
        "dot_scale" => format!("{:.2}", t.dot_scale),
        "spike_scale" => format!("{:.2}", t.spike_scale),
        "spike_core" => format!("{:.2}", t.spike_core),
        "spike_bloom" => format!("{:.2}", t.spike_bloom_exp),
        "spike_width" => format!("{:.3}", t.spike_width),
        "spike_shrink" => format!("{:.2}", t.spike_shrink),
        "spike_strength" => format!("{:.2}", t.spike_strength),
        "spike_taper" => format!("{:.2}", t.spike_taper_exp),
        "galaxy_chance" => format!("{:.2}", t.galaxy_chance),
        "galaxy_min" => format!("{:.0}", t.galaxy_size_min),
        "galaxy_max" => format!("{:.0}", t.galaxy_size_max),
        _ => String::new(),
    }
}

/// Applica il testo di UN campo al tuning, con clamp. Ritorna true se
/// qualcosa è cambiato (il sistema di refresh ricostruisce il cielo solo
/// quando cambia davvero).
pub(crate) fn apply_starfield_field(key: &str, text: &str, t: &mut StarfieldTuning) -> bool {
    let Ok(v) = text.trim().parse::<f32>() else {
        return false; // testo non valido: ignora, non rompere il pannello
    };
    let Some(stripped) = key.strip_prefix("set_sf_") else {
        return false;
    };
    let before = t.clone();
    if let Some((li, field)) = layer_key(stripped) {
        // Copio i valori PRIMA di mutare: `t.layers[li]` e `t` non possono
        // essere presi in prestito insieme.
        let (mut rmin, mut rmax) = (t.layers[li].radius_min, t.layers[li].radius_max);
        match field {
            "factor" => t.layers[li].factor = v.clamp(0.0, 1.0),
            "spread" => t.layers[li].factor_spread = v.clamp(0.0, 0.25),
            "stars" => t.layers[li].stars = v.max(0.0) as u32,
            "rmin" => rmin = v.clamp(0.05, 10.0),
            "rmax" => rmax = v.clamp(0.1, 20.0),
            "sizepow" => t.layers[li].size_power = v.clamp(0.3, 6.0),
            "big" => t.layers[li].big_chance = v.clamp(0.0, 1.0),
            "bright" => t.layers[li].brightness = v.clamp(0.0, 4.0),
            _ => return false,
        }
        // La gamma di dimensione deve restare NON vuota e ordinata: `rmin` è
        // il minore, `rmax` il maggiore.
        let rmin = rmin.min(rmax);
        t.layers[li].radius_min = rmin;
        t.layers[li].radius_max = rmax.max(rmin + 0.05);
        return *t != before;
    }
    match stripped {
        "sector" => t.sector_size = v.clamp(100.0, 5000.0),
        "spawn_margin" => t.spawn_margin = v.clamp(0.0, 5000.0),
        "keep_margin" => t.keep_margin = v.clamp(0.0, 10000.0),
        "max_sectors" => t.max_sectors = (v.max(1.0) as usize).min(400),
        "dot_scale" => t.dot_scale = v.clamp(0.2, 10.0),
        "spike_scale" => t.spike_scale = v.clamp(0.5, 12.0),
        "spike_core" => t.spike_core = v.clamp(0.02, 0.6),
        "spike_bloom" => t.spike_bloom_exp = v.clamp(0.3, 4.0),
        "spike_width" => t.spike_width = v.clamp(0.01, 0.4),
        "spike_shrink" => t.spike_shrink = v.clamp(0.0, 0.95),
        "spike_strength" => t.spike_strength = v.clamp(0.0, 1.0),
        "spike_taper" => t.spike_taper_exp = v.clamp(0.1, 2.0),
        "galaxy_chance" => t.galaxy_chance = v.clamp(0.0, 1.0),
        "galaxy_min" => t.galaxy_size_min = v.clamp(5.0, 600.0),
        "galaxy_max" => t.galaxy_size_max = v.clamp(5.0, 600.0),
        _ => return false,
    }
    // Invarianti (non manopole): senza isteresi le celle di bordo vengono
    // distrutte e ricreate a ogni frame; con min >= max il range RNG panica.
    t.keep_margin = t.keep_margin.max(t.spawn_margin);
    t.galaxy_size_max = t.galaxy_size_max.max(t.galaxy_size_min + 1.0);
    *t != before
}

/// Muove OGNI stella secondo la SUA parallasse: la profondità non è più una
/// proprietà del layer ma della stella, quindi `factor_spread > 0` fa scivolare
/// le stelle l'una rispetto all'altra mentre la camera si sposta (è quello che
/// toglie l'effetto "pareti piatte a tre distanze").
///
/// `Without<StarDrift>` sulla query della camera: due parametri che toccano
/// `Transform` nello stesso sistema sono un B0001 (panic all'avvio → su WASM
/// "Unreachable code").
fn update_star_drift(
    // `Changed<Transform>`: la posizione locale di una stella dipende SOLO
    // dalla posizione della camera, quindi se la camera non si è mossa (es.
    // durante uno zoom, che cambia `Projection` e non il Transform) non c'è
    // niente da riscrivere — e si risparmiano ~18k scritture per frame a zoom
    // out estremo.
    cameras: Query<
        &Transform,
        (
            With<Camera2d>,
            With<MainCamera>,
            Without<StarDrift>,
            Changed<Transform>,
        ),
    >,
    mut stars: Query<(&StarDrift, &mut Transform)>,
) {
    crate::mark_system("update_star_drift");
    let Ok(cam_tf) = cameras.single() else {
        return;
    };
    let cam = cam_tf.translation.truncate();
    for (drift, mut tf) in stars.iter_mut() {
        let local = drift.base + cam * (drift.factor - drift.layer_factor);
        tf.translation.x = local.x;
        tf.translation.y = local.y;
    }
}

/// Spegne la polvere che a questo zoom è sotto il pixel.
///
/// Si esegue SOLO quando il livello di dettaglio cambia (è l'unico momento in
/// cui serve un giro su tutte le stelle): scriverla ogni frame sarebbe un
/// cambiamento di `Visibility` a vuoto su ~18k entità.
fn apply_sky_lod(lod: Res<SkyLod>, mut dust: Query<(&Dust, &mut Visibility)>) {
    crate::mark_system("apply_sky_lod");
    if !lod.is_changed() {
        return;
    }
    for (dust, mut visibility) in dust.iter_mut() {
        let want = if dust_visible(dust.index, lod.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Tuning di RIFERIMENTO (quello spedito): i test parlano di parametri,
    /// non di costanti globali, così restano validi anche se il pannello
    /// "Sfondo" li cambia a runtime.
    fn sf_default() -> StarfieldTuning {
        StarfieldTuning::default()
    }

    fn sector_size() -> f32 {
        sf_default().sector_size
    }

    fn spawn_margin() -> f32 {
        sf_default().spawn_margin
    }

    fn keep_margin() -> f32 {
        sf_default().keep_margin
    }

    fn max_sectors() -> usize {
        sf_default().max_sectors
    }

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
        let sector_area = sector_size() * sector_size();
        for (i, cfg) in sf_default().layers.iter().enumerate() {
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
        let (lo, hi) = sector_range(Vec2::ZERO, half, margin, &sf_default());
        assert!(
            (lo.x as f32) * sector_size() <= -(half.x + margin),
            "left edge not covered"
        );
        assert!(
            ((hi.x + 1) as f32) * sector_size() >= half.x + margin,
            "right edge not covered"
        );
        assert!(
            (lo.y as f32) * sector_size() <= -(half.y + margin),
            "bottom edge not covered"
        );
        assert!(
            ((hi.y + 1) as f32) * sector_size() >= half.y + margin,
            "top edge not covered"
        );
        // A phone viewport must not over-provision: the spawn set stays small.
        let phone = desired_sectors(
            Vec2::ZERO,
            Vec2::new(195.0, 422.0),
            spawn_margin(),
            &sf_default(),
        );
        assert!(
            phone.len() <= 9,
            "phone view spawns {} sectors (expected a handful)",
            phone.len()
        );
        // Extreme zoom-out: the enumeration must stay bounded...
        let (lo, hi) = sector_range(
            Vec2::ZERO,
            Vec2::splat(1.0e6),
            spawn_margin(),
            &sf_default(),
        );
        let span_x = hi.x - lo.x + 1;
        let span_y = hi.y - lo.y + 1;
        assert!(
            span_x <= 21 && span_y <= 21,
            "range exploded: {span_x}×{span_y}"
        );
        // ...and the capped list never exceeds the budget, nearest first.
        let cells = desired_sectors(
            Vec2::ZERO,
            Vec2::splat(1.0e6),
            spawn_margin(),
            &sf_default(),
        );
        assert_eq!(cells.len(), max_sectors());
        let centre = |g: IVec2| g.as_vec2() * sector_size() + Vec2::splat(sector_size() * 0.5);
        assert!(
            centre(cells[0]).length() <= centre(cells[cells.len() - 1]).length(),
            "sectors must be kept nearest-first"
        );
        // Hysteresis: the keep set must be a superset of the spawn set.
        let spawn = desired_sectors(
            Vec2::ZERO,
            Vec2::new(640.0, 360.0),
            spawn_margin(),
            &sf_default(),
        );
        let keep = desired_sectors(
            Vec2::ZERO,
            Vec2::new(640.0, 360.0),
            keep_margin(),
            &sf_default(),
        );
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
        for (li, cfg) in sf_default().layers.iter().enumerate() {
            let grid = IVec2::new(-3, 7);
            let origin = grid.as_vec2() * sector_size();
            let root = sector_origin_world(grid, cfg.factor, cam, sector_size());
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
            let world2 = sector_origin_world(grid, cfg.factor, cam2, sector_size()) + local;
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
        // Varietà di dimensione: quasi tutte piccole, poche grandi E più
        // brillanti. La stella con croce sta sempre in cima alla gamma.
        let mut rng = StdRng::seed_from_u64(7);
        let cfg = sf_default().layers[0];
        let total = 2000;
        let mut big = 0;
        for _ in 0..total {
            let spec = star_look(&mut rng, 0, &sf_default());
            assert!(spec.radius >= cfg.radius_min && spec.radius <= cfg.radius_max);
            let s = spec.color.to_srgba();
            if spec.is_big {
                big += 1;
                // Le stelle con croce sono le più grandi della gamma...
                assert!(
                    spec.radius > cfg.radius_min * 1.8,
                    "una stella con croce non può essere della taglia della polvere ({})",
                    spec.radius
                );
                // ...e le più luminose.
                assert!(
                    s.red.max(s.green).max(s.blue) >= 1.3,
                    "le stelle grandi devono essere anche più brillanti"
                );
            }
        }
        let ratio = big as f32 / total as f32;
        assert!(
            (ratio - cfg.big_chance).abs() < 0.05,
            "quota di stelle con croce {ratio} ≠ {}",
            cfg.big_chance
        );
    }

    /// La richiesta di fondo: NON si deve poter dire "il layer 1 è quello
    /// delle piccole e il layer 3 quello delle grandi". Ogni layer genera sia
    /// polvere sia stelle grandi, con dimensioni intermedie in mezzo.
    #[test]
    fn every_layer_has_both_dust_and_big_stars() {
        for layer in 0..sf_default().layers.len() {
            let cfg = sf_default().layers[layer];
            let mut rng = StdRng::seed_from_u64(1000 + layer as u64);
            let (mut smallest, mut largest) = (f32::MAX, 0.0f32);
            let mut with_cross = 0;
            let total = 3000;
            for _ in 0..total {
                let spec = star_look(&mut rng, layer, &sf_default());
                smallest = smallest.min(spec.radius);
                largest = largest.max(spec.radius);
                if spec.is_big {
                    with_cross += 1;
                }
            }
            assert!(
                smallest < cfg.radius_min * 1.05,
                "layer {layer}: manca la polvere (minimo {smallest})"
            );
            assert!(
                largest > cfg.radius_max * 0.9,
                "layer {layer}: mancano le stelle grandi (massimo {largest})"
            );
            // E la gamma è continua: il 10% più grande è ben sopra la mediana.
            assert!(
                largest > smallest * 2.0,
                "layer {layer}: la gamma di dimensione è troppo stretta"
            );
            assert!(
                with_cross > 0,
                "layer {layer}: nessuna stella con alone+croce"
            );
        }
    }

    /// Ogni stella ha la SUA parallasse: le stelle dello stesso layer non
    /// stanno tutte alla stessa profondità.
    #[test]
    fn parallax_factor_oscillates_inside_each_layer() {
        for layer in 0..sf_default().layers.len() {
            let cfg = sf_default().layers[layer];
            let mut rng = StdRng::seed_from_u64(77 + layer as u64);
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            for _ in 0..2000 {
                let spec = star_look(&mut rng, layer, &sf_default());
                assert!(
                    (0.0..=1.0).contains(&spec.factor),
                    "parallasse fuori range: {}",
                    spec.factor
                );
                min = min.min(spec.factor);
                max = max.max(spec.factor);
            }
            assert!(
                max - min > cfg.factor_spread * 0.5,
                "layer {layer}: la parallasse non oscilla ({min}..{max}, spread {})",
                cfg.factor_spread
            );
            // La finestra è [factor ± spread]; il clamp a 1.0 può tagliarne
            // una parte (factor 1.0 con spread > 0), quindi si verifica
            // l'APPARTENENZA, non l'uguaglianza coi bordi.
            assert!(
                min >= cfg.factor - cfg.factor_spread - 1e-3
                    && max <= cfg.factor + cfg.factor_spread + 1e-3,
                "layer {layer}: oscillazione {min}..{max} fuori dalla finestra \
                 {}..{}",
                cfg.factor - cfg.factor_spread,
                cfg.factor + cfg.factor_spread
            );
        }
    }

    /// Una stella con parallasse propria viene disegnata dove dice la formula
    /// `base + cam·factor`, qualunque sia il fattore del suo settore.
    #[test]
    fn per_star_drift_renders_at_its_own_factor() {
        let cam = Vec2::new(-320.0, 480.0);
        let drift = StarDrift {
            base: Vec2::new(250.0, 700.0),
            factor: 0.93,
            layer_factor: 1.0,
        };
        let local = drift.base + cam * (drift.factor - drift.layer_factor);
        // Mondo = origine del settore + locale, con l'origine a cam·factor_layer.
        let sector_origin = cam * drift.layer_factor;
        let world = sector_origin + local;
        let expected = drift.base + cam * drift.factor;
        assert!(
            (world - expected).length() < 1e-4,
            "mondo {world:?} ≠ base + cam·factor {expected:?}"
        );
        // E la deriva sullo schermo è (1 − factor) × lo spostamento camera.
        let cam2 = cam + Vec2::new(100.0, 0.0);
        let local2 = drift.base + cam2 * (drift.factor - drift.layer_factor);
        let world2 = cam2 * drift.layer_factor + local2;
        let slide = ((world2 - cam2) - (world - cam)).length();
        assert!(
            (slide - 100.0 * (1.0 - drift.factor)).abs() < 1e-3,
            "deriva {slide} ≠ {}",
            100.0 * (1.0 - drift.factor)
        );
    }

    /// Il livello di dettaglio segue lo ZOOM: a zoom out estremo il settore
    /// copre pochi pixel e la polvere (sotto il pixel) si spegne. È la difesa
    /// contro il conto che fa scattare un Mac a zoom out: da ~1.100 a ~18.800
    /// sprite per zero pixel visibili in più.
    #[test]
    fn sky_lod_follows_the_zoom() {
        let sector = sf_default().sector_size;
        // Zoom normale: tutto visibile.
        assert_eq!(sky_lod(1.0, sector), 0);
        assert_eq!(sky_lod(2.0, sector), 0);
        // Settore ≤ 400 px a schermo: una polvere su 4.
        assert_eq!(sky_lod(3.0, sector), 1);
        assert_eq!(sky_lod(8.0, sector), 1);
        // Settore ≤ 120 px: solo le stelle grandi.
        assert_eq!(sky_lod(10.0, sector), 2);
        assert_eq!(sky_lod(50.0, sector), 2);
        // Zoom-in: mai sotto il dettaglio pieno.
        assert_eq!(sky_lod(0.5, sector), 0);
        // Difesa: zoom 0 o valori assurdi non devono panicare.
        assert_eq!(sky_lod(0.0, sector), 0);
    }

    /// A `lod = 1` si tiene una polvere su quattro, sempre le STESSE: la
    /// scelta dipende dall'indice nella cella, non dal frame.
    #[test]
    fn dust_lod_is_stable() {
        assert!(dust_visible(0, 0) && dust_visible(7, 0));
        for index in 0..64u32 {
            let expected = index % 4 == 0;
            assert_eq!(
                dust_visible(index, 1),
                expected,
                "lod 1: indice {index} sbagliato"
            );
        }
        assert!(!dust_visible(0, 2) && !dust_visible(3, 2));
        // A lod 1 resta un quarto della polvere.
        let kept = (0..100u32).filter(|i| dust_visible(*i, 1)).count();
        assert_eq!(kept, 25);
    }

    /// `apply_sky_lod` spegne/riaccende la polvere giusta quando il livello
    /// cambia, e non tocca niente se non è cambiato.
    #[test]
    fn apply_sky_lod_toggles_dust_visibility() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SkyLod>()
            .add_systems(Update, apply_sky_lod);
        for index in 0..8u32 {
            app.world_mut()
                .spawn((Dust { index }, Visibility::Inherited));
        }
        // lod 0 (default) → tutto visibile, niente da fare.
        app.update();
        let visible = |app: &mut App| -> Vec<bool> {
            let world = app.world_mut();
            let mut rows: Vec<(u32, bool)> = world
                .query::<(&Dust, &Visibility)>()
                .iter(world)
                .map(|(dust, visibility)| (dust.index, *visibility == Visibility::Inherited))
                .collect();
            rows.sort_by_key(|(index, _)| *index);
            rows.into_iter().map(|(_, visible)| visible).collect()
        };
        assert_eq!(visible(&mut app), vec![true; 8]);

        // lod 2 → solo le stelle grandi: tutta la polvere spenta.
        app.world_mut().resource_mut::<SkyLod>().0 = 2;
        app.update();
        assert_eq!(visible(&mut app), vec![false; 8]);

        // lod 1 → 1 su 4 (indici 0 e 4).
        app.world_mut().resource_mut::<SkyLod>().0 = 1;
        app.update();
        let expected = (0..8u32).map(|i| i % 4 == 0).collect::<Vec<_>>();
        assert_eq!(visible(&mut app), expected);

        // Ritorno a lod 0: tutto riacceso.
        app.world_mut().resource_mut::<SkyLod>().0 = 0;
        app.update();
        assert_eq!(visible(&mut app), vec![true; 8]);
    }

    #[test]
    fn galaxies_are_deterministic_and_sparse() {
        assert!(sf_default().galaxy_chance > 0.0 && sf_default().galaxy_chance < 0.5);
        assert!(sf_default().galaxy_size_min < sf_default().galaxy_size_max);
        // Deterministic per sector: the same cell gives the same coin flip.
        let mut rng_a = StdRng::seed_from_u64(seed_for(42, 2, IVec2::new(5, -2)));
        let mut rng_b = StdRng::seed_from_u64(seed_for(42, 2, IVec2::new(5, -2)));
        assert_eq!(
            rng_a.gen_bool(sf_default().galaxy_chance as f64),
            rng_b.gen_bool(sf_default().galaxy_chance as f64)
        );
        // Over a wide sample the fraction stays in the expected ballpark.
        let samples = 600;
        let mut with_galaxy = 0;
        for i in 0..samples {
            let mut rng = StdRng::seed_from_u64(seed_for(42, 2, IVec2::new(i, 3)));
            if rng.gen_bool(sf_default().galaxy_chance as f64) {
                with_galaxy += 1;
            }
        }
        let chance = sf_default().galaxy_chance as f64;
        let ratio = with_galaxy as f64 / samples as f64;
        assert!(
            (ratio - chance).abs() < 0.08,
            "galaxy ratio {ratio} should be close to {chance}"
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
        let px = star_spike_pixels(&sf_default());
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
        // L'alone NON deve più allargarsi a due raggi di stella: fuori dalla
        // croce (sulla diagonale) a 0.7r deve essere quasi spento. Con
        // bloom 1.6 valeva 0.22 e le stelle con croce sembravano un blob.
        assert!(
            on_diagonal < 0.1,
            "alone troppo largo: a 0.7r sulla diagonale vale {on_diagonal}"
        );
        // I bracci si RASTREMANO verso la punta: larghi al nucleo, a spillo
        // fuori (una punta larga all'estremità sembra un "tridente" rovescio).
        // Misura l'FWHM del braccio a due distanze.
        // Rapporto fuori-asse / asse alla STESSA distanza: misura la forma del
        // profilo senza dipendere dalla luminosità (che cala comunque).
        let profile = |d: f32| -> f32 {
            let x = c + (half * d) as usize;
            let on_axis = at(x, c);
            let off_axis = at(x, c + 1);
            if on_axis > 0.0 {
                off_axis / on_axis
            } else {
                0.0
            }
        };
        let near = profile(0.55);
        let far = profile(0.9);
        assert!(
            near > far * 1.2,
            "il braccio non si assottiglia verso la punta: profilo {near:.2} a 0.55r, {far:.2} a 0.9r"
        );
        // La punta resta comunque visibile (altrimenti i bracci sembrano corti).
        let outer = at(c + (half * 0.9) as usize, c);
        assert!(
            outer > 0.12,
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
        for layer in 0..sf_default().layers.len() {
            for _ in 0..200 {
                let s = star_look(&mut rng, layer, &sf_default()).color.to_srgba();
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
                let s = star_look(rng, layer, &sf_default()).color.to_srgba();
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
        // planets at z = -distance from the star). The depth is structural
        // (LAYER_Z), so it is not a panel knob — but it must stay ordered.
        for z in LAYER_Z {
            assert!(z < -100_000.0, "layer z {z} not behind planets");
        }
        assert!(LAYER_Z[0] < LAYER_Z[1] && LAYER_Z[1] < LAYER_Z[2]);
        // Far layers are more glued to the camera than near ones.
        assert!(
            sf_default().layers[0].factor > sf_default().layers[1].factor
                && sf_default().layers[1].factor > sf_default().layers[2].factor
        );
        assert!((sf_default().layers[0].factor - 1.0).abs() < 1e-6);
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
            .init_resource::<StarfieldTuning>()
            .init_resource::<SkyLod>()
            .init_resource::<StarSectors>()
            .add_systems(Startup, setup_starfield)
            .add_systems(
                Update,
                (
                    refresh_starfield_on_tuning_change,
                    stream_starfield,
                    apply_sky_lod,
                    update_star_drift,
                )
                    .chain(),
            );

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
        assert!(after.len() <= sf_default().layers.len() * max_sectors());
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
            let expected = sector_origin_world(
                sector.grid,
                sf_default().layers[sector.layer as usize].factor,
                cam_now,
                sector_size(),
            );
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
