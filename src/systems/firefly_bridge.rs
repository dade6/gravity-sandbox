//! Spike v2: firefly nativo per luci + ombre + normal map + z-sorting.
//!
//! Strategia (aggiornata dopo il test di v0.14.39):
//! - I sistemi custom (LightingPlugin/LightPlugin/ShadowPlugin) sono
//!   DISATTIVATI: firefly fa tutto (luce, ombre morbide, normal map, z-sort).
//! - Ogni corpo viene convertito da Mesh2d a Sprite + NormalMap + SpriteHeight
//!   (firefly legge la normal map SOLO sulle Sprite). La conversione è un
//!   sistema in Update: prende QUALSIASI corpo (spawn da preset, add tool,
//!   debug) e lo trasforma al primo frame utile.
//! - La stella: PointLight2d con LightCore circolare (radius = raggio stella)
//!   -> la luce non è puntiforme ma un disco grande quanto la stella.
//! - z_sorting: true -> le sprite con z maggiore non ricevono ombre dagli
//!   occluder (la stella a SpriteHeight alto non viene oscurata dai pianeti).
//! - Occluder: Occluder2d::circle per ogni corpo non-luminoso.

use avian2d::prelude::LockedAxes;
use bevy::prelude::*;
use bevy_firefly::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::rendering::textures::{build_image, generate_sphere_normal_map};

/// Configurazione luce: disco circolare della dimensione della stella.
/// ambient 0.03: il lato in ombra dei pianeti è quasi nero (solo il 3% del
/// colore). Lo sfondo grigio scuro NON dipende dall'ambient: la luce a
/// Falloff::NONE copre tutto il viewport -> sfondo = ClearColor (0.10).
const AMBIENT_BRIGHTNESS: f32 = 0.03;
const LIGHT_INTENSITY: f32 = 1.8;
const LIGHT_RADIUS: f32 = 5000.0;
/// Altezza della luce sopra il piano (TopDownY): dal v0.14.56 è INATTIVA:
/// il ramo mode 2 della shader usa il 2D puro (direzione luce nel solo
/// piano dello schermo: dx, dy, 0). LightHeight resta nel codice solo per
/// non rompere l'API del crate (badge lH0).
const LIGHT_HEIGHT: f32 = 0.0;
/// Boost del core: rende il disco centrale (dimensione stella) più brillante.
const LIGHT_CORE_BOOST: f32 = 3.0;
/// Altezza stella: con z_sorting, non riceve ombre dai pianeti.
const STAR_HEIGHT: f32 = 1000.0;
/// Dimensione dell'alone (glow) attorno alla stella, in multipli del raggio.
const GLOW_SCALE: f32 = 4.0;
/// Opacità dell'alone.
const GLOW_ALPHA: f32 = 0.55;

/// Marker sulla camera (ha già FireflyConfig).
#[derive(Component)]
pub struct FireflyCamera;

/// Marker: la stella ha già la sua PointLight2d child.
#[derive(Component)]
pub struct FireflyLightAttached;

/// Marker: il corpo ha già il suo Occluder2d child.
#[derive(Component)]
pub struct FireflyOccluderAttached;

/// Marker: il corpo è stato convertito da Mesh2d a Sprite.
#[derive(Component)]
pub struct FireflySpriteAttached;

/// Texture condivise: disco bianco (base sprite), normal map sfera, alone.
#[derive(Resource)]
pub struct FireflyTextures {
    pub disc: Handle<Image>,
    pub sphere_normal: Handle<Image>,
    /// Disco con gradiente radiale (bianco al centro → trasparente ai bordi),
    /// usato come alone luminoso attorno alle stelle.
    pub glow: Handle<Image>,
}

impl FromWorld for FireflyTextures {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();

        // Disco bianco 64x64 con alpha (cerchio pieno) — tinto con sprite.color.
        let size = 64usize;
        let mut pixels = vec![0u8; size * size * 4];
        let center = (size as f32 - 1.0) / 2.0;
        let radius = size as f32 / 2.0 - 1.0;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                let alpha = if dist <= radius { 255u8 } else { 0u8 };
                let i = (y * size + x) * 4;
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = alpha;
            }
        }
        let disc = images.add(build_image(pixels, size as u32, size as u32));

        // Alone radiale 128x128: alpha massima al centro, sfuma a 0 ai bordi
        // (falloff quadratico) — tinto con il colore della stella.
        let gsize = 128usize;
        let mut gpixels = vec![0u8; gsize * gsize * 4];
        let gcenter = (gsize as f32 - 1.0) / 2.0;
        let gradius = gsize as f32 / 2.0 - 1.0;
        for y in 0..gsize {
            for x in 0..gsize {
                let dx = x as f32 - gcenter;
                let dy = y as f32 - gcenter;
                let dist = (dx * dx + dy * dy).sqrt();
                let t = (dist / gradius).clamp(0.0, 1.0);
                let alpha = ((1.0 - t) * (1.0 - t) * 255.0) as u8;
                let i = (y * gsize + x) * 4;
                gpixels[i] = 255;
                gpixels[i + 1] = 255;
                gpixels[i + 2] = 255;
                gpixels[i + 3] = alpha;
            }
        }
        let glow = images.add(build_image(gpixels, gsize as u32, gsize as u32));

        // Normal map sfera: GENERATA proceduralmente (stessa identica immagine
        // della sfera, 256x256) invece di caricarla da file. Motivi:
        // 1. load_with_settings(is_srgb=false) crashava il WASM su iPhone
        //    (fetch asset async) -> WASM DEAD -> canvas congelato.
        // 2. build_image crea Rgba8Unorm NON-sRGB per costruzione: niente
        //    gamma correction, i valori delle normali sono corretti.
        // 3. Pronta subito: niente attesa del caricamento, niente timing.
        // Vedi generate_sphere_normal_map() in rendering/textures.rs.
        let nsize = 256usize;
        let npixels = generate_sphere_normal_map(nsize);
        let sphere_normal = images.add(build_image(npixels, nsize as u32, nsize as u32));

        Self {
            disc,
            sphere_normal,
            glow,
        }
    }
}

pub struct FireflyBridgePlugin;

impl Plugin for FireflyBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FireflyPlugin)
            .init_resource::<FireflyTextures>()
            // Idempotente: si attiva al primo frame in cui la camera esiste.
            .add_systems(Update, setup_firefly_camera)
            .add_systems(
                Update,
                (
                    lock_body_rotation,
                    convert_bodies_to_sprites,
                    attach_normal_maps,
                    spawn_star_lights,
                    spawn_planet_occluders,
                    sync_occluders,
                    sync_sprite_z,
                )
                    .chain(),
            );
    }
}

/// I corpi sono sfere: la rotazione dopo le collisioni fa ruotare la normal
/// map (texture locale alla sprite) → il pattern luce/ombra sulla superficie
/// non punta più alla stella. Blocchiamo la rotazione su tutti i corpi
/// (idempotente: gira finché non tutti hanno LockedAxes).
fn lock_body_rotation(
    mut commands: Commands,
    bodies: Query<Entity, (With<CelestialBody>, Without<LockedAxes>)>,
) {
    for entity in bodies.iter() {
        commands.entity(entity).insert(LockedAxes::ROTATION_LOCKED);
    }
}

/// La camera principale prende FireflyConfig (ambient scuro, soft shadows,
/// z-sorting, normal map mode Simple = direzione luce nel piano xy).
fn setup_firefly_camera(
    mut commands: Commands,
    cameras: Query<(Entity, &Camera), Without<FireflyCamera>>,
) {
    for (entity, _cam) in cameras.iter() {
        commands.entity(entity).insert((
            FireflyCamera,
            // Hdr: richiesto dalla pipeline firefly (vedi lights.rs/sprites.rs
            // che settano TONEMAP_IN_SHADER solo se !camera.hdr). L'esempio
            // ufficiale shapes.rs la mette sempre.
            bevy::core_pipeline::tonemapping::Tonemapping::Reinhard,
            bevy::camera::Hdr,
            FireflyConfig {
                ambient_color: Color::WHITE,
                ambient_brightness: AMBIENT_BRIGHTNESS,
                soft_shadows: true,
                // Spike v0.14.50: z_sorting=true + z=-y sui pianeti
                // (sync_sprite_z) -> ogni sprite ha z UNICO. Il crate salta
                // l'auto-ombra del corpo stesso (stencil.g >= occ.z - margin,
                // stessi z) ma applica le ombre degli ALTRI corpi (z diversi):
                // il rilievo normal map non viene più spento dall'ombra
                // propria. Con false e z tutti 0 (vecchio setup) il corpo si
                // oscurava da sé -> pianeti piatti nonostante normal map ok.
                z_sorting: true,
                // Allineato all'esempio ufficiale crates.rs (normal map +
                // z-sorting): normal_mode TopDownY (usa stencil.r/stencil.b
                // per la direzione della luce) + enable_32bit_stencils: true
                // (formato stencil Rgba32Float, richiesto dalle normal map).
                normal_mode: NormalMode::TopDownY,
                enable_32bit_stencils: true,
                // ATTENZIONE: attenuation è mix(normal, 0, t) — più ALTO =
                // più piatto! 0.2 = rilievo deciso, quasi pieno.
                normal_attenuation: 0.2,
                ..default()
            },
        ));
    }
}

/// Converte ogni corpo da Mesh2d a Sprite.
/// - Pianeti: Sprite + NormalMap + SpriteHeight(0) → ricevono luce/ombre.
/// - Stelle: Sprite BRILLANTE senza normal map (sono sorgenti, non superfici
///   illuminate) + child glow (alone radiale). La normal map sulla stella
///   la scuriva al centro (direzione luce ≈ 0) e il LightCore creava solo un
///   puntino: sbagliato, la stella deve essere un disco emissivo.
/// Idempotente (marker FireflySpriteAttached). Aspetta che la normal map sia
/// caricata prima di attaccare NormalMap (niente texture async al primo frame).
fn convert_bodies_to_sprites(
    mut commands: Commands,
    textures: Res<FireflyTextures>,
    bodies: Query<
        (Entity, &CelestialBody, &Transform),
        (Without<FireflySpriteAttached>, With<Mesh2d>),
    >,
) {
    for (entity, body, transform) in bodies.iter() {
        let mut cmd = commands.entity(entity);
        cmd.remove::<Mesh2d>()
            .remove::<MeshMaterial2d<crate::systems::lighting::LightMaterial>>()
            .remove::<MeshMaterial2d<ColorMaterial>>()
            .insert(FireflySpriteAttached)
            .insert((
                Sprite {
                    image: textures.disc.clone(),
                    color: Color::srgba(body.color[0], body.color[1], body.color[2], 1.0),
                    custom_size: Some(Vec2::splat(body.radius * 2.0)),
                    ..default()
                },
                SpriteHeight(if body.luminous { STAR_HEIGHT } else { 0.0 }),
            ));

        if body.luminous {
            // Stella: NIENTE normal map, ma alone luminoso radiale (glow)
            // grande GLOW_SCALE× il raggio, tinto col colore della stella.
            let glow_color = Color::srgba(
                body.color[0],
                body.color[1],
                body.color[2],
                GLOW_ALPHA,
            );
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        image: textures.glow.clone(),
                        color: glow_color,
                        custom_size: Some(Vec2::splat(body.radius * 2.0 * GLOW_SCALE)),
                        ..default()
                    },
                    Transform::default(),
                ));
            });
        }
        // La NormalMap la attacca attach_normal_maps (separato): se l'asset
        // non è ancora caricato qui, il pianeta NON deve restare senza.
    }
}

/// Attacca la normal map sfera ai pianeti convertiti, appena l'asset è
/// caricato. Separato da convert_bodies_to_sprites perché se il PNG non è
/// pronto al primo frame, il pianeta verrebbe convertito senza NormalMap e
/// il marker FireflySpriteAttached impedirebbe di riprocessarlo -> rilievo
/// piatto per sempre. Idempotente: gira finché non tutti i pianeti l'hanno.
fn attach_normal_maps(
    mut commands: Commands,
    textures: Res<FireflyTextures>,
    planets: Query<
        (Entity, &CelestialBody),
        (
            With<FireflySpriteAttached>,
            With<Sprite>,
            Without<NormalMap>,
        ),
    >,
) {
    // La normal map è PROCEDURALE (aggiunta ad Assets<Image> con add()):
    // è pronta immediatamente, nessun caricamento async da attendere.
    // (Il vecchio check get_load_state bloccava l'attacco per sempre,
    // perché gli handle procedurali non passano dall'asset server.)
    for (entity, body) in planets.iter() {
        // FIX: la stella (luminosa) NON deve avere la normal map:
        // è una sorgente emissiva. Senza questo check la stella
        // riceveva la NormalMap e contaminava il rilievo.
        if body.luminous {
            continue;
        }
        commands
            .entity(entity)
            .insert(NormalMap::from_image(textures.sphere_normal.clone()));
    }
}

/// Ogni stella ottiene una PointLight2d child che la segue, con LightCore
/// circolare grande 1.5× il raggio della stella (luce a disco: il centro
/// brillante copre tutta la stella, non un puntino).
fn spawn_star_lights(
    stars: Query<(Entity, &CelestialBody), (Without<FireflyLightAttached>, Without<FireflyOccluderAttached>)>,
    mut commands: Commands,
) {
    for (entity, body) in stars.iter() {
        if !body.luminous {
            continue;
        }
        commands.entity(entity).insert(FireflyLightAttached).with_children(
            |parent| {
                parent.spawn((
                    PointLight2d {
                        color: Color::srgba(body.color[0], body.color[1], body.color[2], 1.0),
                        intensity: LIGHT_INTENSITY,
                        radius: LIGHT_RADIUS,
                        falloff: Falloff::NONE,
                        core: LightCore {
                            radius: body.radius * 1.5,
                            boost: LIGHT_CORE_BOOST,
                            ..default()
                        },
                        ..default()
                    },
                    // CRITICO per TopDownY: senza questa component la luce ha
                    // height=0 -> vedi commento su LIGHT_HEIGHT.
                    LightHeight(LIGHT_HEIGHT),
                    Transform::default(),
                ));
            },
        );
    }
}

/// Ogni corpo non-luminoso ottiene un Occluder2d::circle child (raggio =
/// raggio del corpo; il diametro è gestito dalla mesh del corpo).
fn spawn_planet_occluders(
    planets: Query<
        (Entity, &CelestialBody),
        (
            Without<FireflyLightAttached>,
            Without<FireflyOccluderAttached>,
        ),
    >,
    mut commands: Commands,
) {
    for (entity, body) in planets.iter() {
        if body.luminous {
            continue;
        }
        commands.entity(entity).insert(FireflyOccluderAttached).with_children(
            |parent| {
                parent.spawn((
                    Occluder2d::circle(body.radius),
                    Transform::default(),
                ));
            },
        );
    }
}

/// Il raggio dell'occluder deve seguire il raggio del corpo (che l'utente
/// può cambiare dal property panel). Child = segue la posizione gratis;
/// qui sovrascriviamo solo il componente Occluder2d col raggio attuale
/// (idempotente quando il raggio non è cambiato).
fn sync_occluders(
    planets: Query<(&CelestialBody, &Children), With<FireflyOccluderAttached>>,
    mut occluders: Query<&mut Occluder2d>,
) {
    for (body, children) in planets.iter() {
        if body.luminous {
            continue;
        }
        for child in children.iter() {
            if let Ok(mut occ) = occluders.get_mut(child) {
                *occ = Occluder2d::circle(body.radius);
            }
        }
    }
}

/// z-sorting alla firefly: z = -y sui corpi NON luminosi (come l'esempio
/// ufficiale crates.rs: "transform.translation.z = -transform.translation.y").
/// Con z unici per sprite/occluder (child con Transform di default: il global
/// z è quello del parent), il crate salta l'AUTO-OMBRA del corpo stesso
/// (stencil.g >= occ.z - margin: z identici) ma applica le ombre degli ALTRI
/// corpi (z diversi): il rilievo normal map non viene più spento dall'ombra
/// propria. La stella resta a z fisso: non riceve ombre dai pianeti.
fn sync_sprite_z(
    mut sprites: Query<(&CelestialBody, &mut Transform), With<Sprite>>,
) {
    for (body, mut transform) in sprites.iter_mut() {
        if body.luminous {
            continue;
        }
        transform.translation.z = -transform.translation.y;
    }
}
