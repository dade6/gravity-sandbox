//! Spike: ponte Bevy Firefly ↔ corpi del sandbox.
//!
//! Strategia ibrida (verificata sul sorgente firefly-0.19):
//! - Il nostro `LightMaterial` (lighting.rs) continua a fare la luce per-pixel
//!   con normal map sui MESH (nessun crate la fa su Mesh2d).
//! - Firefly fa SOLO le ombre: genera una lightmap e la MOLTIPLICA sulla
//!   scena (`scene_frag * light_frag` in apply_lightmap.wgsl).
//! - Configurando ambient_brightness=0.35 + una PointLight2d con
//!   Falloff::NONE intensità 0.65, la lightmap vale:
//!     1.0 fuori dall'ombra (moltiplicazione neutra: la scena resta dipinta
//!        dal nostro materiale) e 0.35 dentro l'ombra (corpo scurito).
//!   L'occlusione (pianeta dietro pianeta) è gratis: è il lavoro di firefly.
//!
//! Occluder: un `Occluder2d::circle(radius)` per ogni corpo non-luminoso,
//! sincronizzato col Transform del corpo ogni frame. Le stelle emettono luce
//! (`PointLight2d`), mai occluder.

use bevy::prelude::*;
use bevy_firefly::prelude::*;

use crate::components::celestial::CelestialBody;

/// Configurazione dello spike (calibrata per ombra pura, vedi module docs).
const AMBIENT_BRIGHTNESS: f32 = 0.35;
/// La PointLight con Falloff::NONE porta la lightmap a ~1.0 fuori dall'ombra.
const LIGHT_INTENSITY: f32 = 0.65;
/// Raggio luce: deve coprire tutta la scena (oltre MAX_LIGHT_DISTANCE).
const LIGHT_RADIUS: f32 = 5000.0;

/// Marker sulla camera (ha già FireflyConfig).
#[derive(Component)]
pub struct FireflyCamera;

/// Marker: la stella ha già la sua PointLight2d child.
#[derive(Component)]
pub struct FireflyLightAttached;

/// Marker: il corpo ha già il suo Occluder2d child.
#[derive(Component)]
pub struct FireflyOccluderAttached;

pub struct FireflyBridgePlugin;

impl Plugin for FireflyBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FireflyPlugin)
            .add_systems(Startup, setup_firefly_camera)
            .add_systems(
                Update,
                (
                    spawn_star_lights,
                    spawn_planet_occluders,
                    sync_occluders,
                )
                    .chain(),
            );
    }
}

/// La camera principale prende FireflyConfig (ambient scuro + soft shadows).
fn setup_firefly_camera(
    mut commands: Commands,
    cameras: Query<(Entity, &Camera), Without<FireflyCamera>>,
) {
    for (entity, _cam) in cameras.iter() {
        commands.entity(entity).insert((
            FireflyCamera,
            FireflyConfig {
                ambient_color: Color::WHITE,
                ambient_brightness: AMBIENT_BRIGHTNESS,
                soft_shadows: true,
                ..default()
            },
        ));
    }
}

/// Ogni stella ottiene una PointLight2d child che la segue.
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
                        ..default()
                    },
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
