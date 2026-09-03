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
use crate::components::lighting::{
    AmbientLight, GlowCurve, LightFalloff, StarGlow, StarLightSettings,
};
use crate::rendering::textures::{build_image, generate_radial_glow_texture, generate_sphere_normal_map};

/// Configurazione luce: disco circolare della dimensione della stella.
/// I valori LIVE vengono letti dal componente `StarLightSettings` attaccato
/// alla stella (default corretti qui sotto: intensity 1.8, radius 5000).
/// ambient 0.03: il lato in ombra dei pianeti è quasi nero. Da Ticket 19
/// l'ambient è pilotato dalla risorsa `AmbientLight` (preset.json).
/// Altezza della luce sopra il piano (TopDownY): dal v0.14.56 è INATTIVA:
/// il ramo mode 2 della shader usa il 2D puro (direzione luce nel solo
/// piano dello schermo: dx, dy, 0). LightHeight resta nel codice solo per
/// non rompere l'API del crate (badge lH0).
const LIGHT_HEIGHT: f32 = 0.0;
/// Altezza stella: con z_sorting, non riceve ombre dai pianeti.
const STAR_HEIGHT: f32 = 1000.0;

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

/// Marker sul child glow interno (Sprite, ~4× raggio stella).
#[derive(Component)]
pub struct FireflyGlowInner;

/// Marker sul child glow esterno (Sprite, ~25× raggio stella, faint).
#[derive(Component)]
pub struct FireflyGlowOuter;

/// Texture condivise: disco bianco (base sprite), normal map sfera, alone.
#[derive(Resource)]
pub struct FireflyTextures {
    pub disc: Handle<Image>,
    pub sphere_normal: Handle<Image>,
    /// Disco con gradiente radiale (bianco al centro → trasparente ai bordi),
    /// usato come alone luminoso attorno alle stelle. La curva + soft edge
    /// sono GLOBALI (risorsa `GlowCurve`); `curve` qui serve a rigenerarla al
    /// cambio preset (Reset) senza toccare le stelle già sparse.
    pub glow: Handle<Image>,
    pub curve: GlowCurve,
}

impl FromWorld for FireflyTextures {
    fn from_world(world: &mut World) -> Self {
        let curve = world.resource::<GlowCurve>().clone();
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

        // Alone radiale: curva + soft edge dal GlowCurve globale (4321: niente
        // gradino al bordo). Tinto col colore della stella.
        let glow = images.add(generate_radial_glow_texture(128, &curve));

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
            curve,
        }
    }
}

pub struct FireflyBridgePlugin;

impl Plugin for FireflyBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FireflyPlugin)
            .init_resource::<FireflyTextures>()
            .init_resource::<GlowCurve>()
            .init_resource::<AmbientLight>()
            // Idempotente: si attiva al primo frame in cui la camera esiste.
            .add_systems(Update, setup_firefly_camera)
            // Ticket 19: rigenera la glow texture al cambio curva globale e
            // sincronizza l'ambient dalla risorsa AmbientLight (preset.json).
            .add_systems(
                Update,
                (ensure_glow_texture_matches_curve, apply_ambient_light),
            )
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
                    apply_star_light_settings,
                    apply_star_glow_settings,
                )
                    .chain(),
            );
    }
}

/// I corpi sono sfere: la rotazione dopo le collisioni fa ruotare la normal
/// map (texture locale alla sprite) → il pattern luce/ombra sulla superficie
/// non punta più alla stella. Blocchiamo la rotazione su tutti i corpi
/// (idempotente: gira finché non tutti hanno LockedAxes).
///
/// NB: la insert è deferred (apply_deferred) e l'entity iterata può essere
/// despawnata da un sistema precedente nello stesso frame (es. `ResetMessage`
/// che ricrea i corpi, o Delete tool). Senza protezione Bevy panica
/// "Entity despawned" e il panic hook crasha il WASM (bug v0.14.73, log
/// Safari Mac: entity 1283v0 generazione 1). Usiamo `try_insert` invece di
/// `insert`: la documentazione Bevy dice testualmente "If the entity does
/// not exist when this command is executed, the resulting error will be
/// ignored" — esattamente il comportamento che vogliamo. Il sistema resta
/// idempotente: i corpi appena creati al frame successivo verranno comunque
/// bloccati dalla prossima iterazione.
fn lock_body_rotation(
    mut commands: Commands,
    bodies: Query<Entity, (With<CelestialBody>, Without<LockedAxes>)>,
) {
    for entity in bodies.iter() {
        commands.entity(entity).try_insert(LockedAxes::ROTATION_LOCKED);
    }
}

/// La camera principale prende FireflyConfig (ambient scuro, soft shadows,
/// z-sorting, normal map mode Simple = direzione luce nel piano xy).
fn setup_firefly_camera(
    mut commands: Commands,
    cameras: Query<(Entity, &Camera), Without<FireflyCamera>>,
    ambient: Res<AmbientLight>,
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
                ambient_color: Color::srgb(
                    ambient.color[0],
                    ambient.color[1],
                    ambient.color[2],
                ),
                ambient_brightness: ambient.intensity,
                soft_shadows: true,
                // v0.14.59 (con patch vendored nella shader): z_sorting=true
                // MA il check è ora l'UGUAGLIANZA esatta stencil.g == occ.z
                // (body-id nel Transform.z, sync_sprite_z) -> skippata SOLO
                // l'auto-ombra; le ombre tra pianeti sono sempre applicate
                // (la geometria decide: segmento stella->pixel). Le varianti
                // precedenti fallivano: false -> auto-ombra quasi totale
                // (pianeti neri, v0.14.58); true+z=-y -> nessuna ombra tra
                // pianeti allineati (v0.14.50).
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

/// Alpha efficace dello sprite glow dopo compensazione intensity (Ticket 20).
///
/// La lightmap firefly (`scene_frag × light_frag`) moltiplica anche gli sprite
/// dei glow, che quindi ricevono `∝ intensity`: con intensity=2 l'alone
/// raddoppia di brillantezza. Scrivendo nello Sprite un'alpha PRE-DIVISA per
/// intensity, la moltiplicazione della lightmap annulla la divisione e l'alone
/// resta visivamente costante (opzione C: alone decorativo puro).
///
/// intensity <= 0.0001 -> 0: stella spenta, alone invisibile (NON alpha=1 —
/// la formula nuda `1.0/max(intensity, 0.0001)` darebbe comp=10000 e il clamp
/// produrrebbe un alone PIENO, sbagliato).
pub(crate) fn glow_alpha(base_alpha: f32, intensity: f32) -> f32 {
    let comp = if intensity <= 0.0001 { 0.0 } else { 1.0 / intensity };
    (base_alpha * comp).clamp(0.0, 1.0)
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
        (
            Entity,
            &CelestialBody,
            &Transform,
            Option<&StarGlow>,
            Option<&StarLightSettings>,
        ),
        (Without<FireflySpriteAttached>, With<Mesh2d>),
    >,
) {
    for (entity, body, _transform, glow, light_settings) in bodies.iter() {
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
            // Stella: NIENTE normal map, ma DUE aloni radiali semi-trasparenti
            // (inner = alino stretto, outer = alone ampio e tenue), entrambi
            // tinti col colore della stella. Parametri (scale/alpha) dal
            // componente StarGlow (default: inner 4×/0.55, outer 25×/0.18).
            // v0.14.77 (Ticket 20): l'alpha è compensata per intensity con lo
            // STESSO helper `glow_alpha` usato da `apply_star_glow_settings`
            // (live edit) → sprite identici nei due path. Se StarLightSettings
            // manca, default 1.8 (come `spawn_star_lights`).
            let g = glow.cloned().unwrap_or_default();
            let intensity = light_settings
                .map(|s| s.intensity)
                .unwrap_or(StarLightSettings::default().intensity);
            let inner_color = Color::srgba(
                body.color[0],
                body.color[1],
                body.color[2],
                glow_alpha(g.inner_alpha, intensity),
            );
            let outer_color = Color::srgba(
                body.color[0],
                body.color[1],
                body.color[2],
                glow_alpha(g.outer_alpha, intensity),
            );
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    FireflyGlowInner,
                    Sprite {
                        image: textures.glow.clone(),
                        color: inner_color,
                        custom_size: Some(Vec2::splat(body.radius * 2.0 * g.inner_scale)),
                        ..default()
                    },
                    Transform::default(),
                ));
                parent.spawn((
                    FireflyGlowOuter,
                    Sprite {
                        image: textures.glow.clone(),
                        color: outer_color,
                        custom_size: Some(Vec2::splat(body.radius * 2.0 * g.outer_scale)),
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
/// circolare grande `core_radius_factor`× il raggio della stella (luce a
/// disco: il centro brillante copre tutta la stella, non un puntino). I
/// valori LEGGE dal componente `StarLightSettings` (intensity/radius/falloff/
/// fade_width/core), così il Reset/load produce esattamente i valori salvati
/// senza doverli ri-applicare in un secondo tempo.
fn spawn_star_lights(
    stars: Query<
        (Entity, &CelestialBody, Option<&StarLightSettings>),
        (Without<FireflyLightAttached>, Without<FireflyOccluderAttached>),
    >,
    mut commands: Commands,
) {
    for (entity, body, settings) in stars.iter() {
        if !body.luminous {
            continue;
        }
        let s = settings.cloned().unwrap_or_default();
        commands
            .entity(entity)
            .insert(FireflyLightAttached)
            .with_children(|parent| {
                parent.spawn((
                    PointLight2d {
                        color: Color::srgba(body.color[0], body.color[1], body.color[2], 1.0),
                        intensity: s.intensity,
                        radius: s.radius,
                        falloff: s.falloff.into_firefly(),
                        fade_width: s.fade_width,
                        core: LightCore {
                            radius: body.radius * s.core_radius_factor,
                            boost: s.core_boost,
                            ..default()
                        },
                        ..default()
                    },
                    // CRITICO per TopDownY: senza questa component la luce ha
                    // height=0 -> vedi commento su LIGHT_HEIGHT.
                    LightHeight(LIGHT_HEIGHT),
                    Transform::default(),
                ));
            });
    }
}

/// Ogni corpo non-luminoso è ANCHE Occluder2d::circle (stesso entity del
/// corpo, NON un child): così lo z letto dall'extract (GlobalTransform del
/// corpo) è ESATTAMENTE lo stesso che sync_sprite_z scrive sulla sprite ->
/// il confronto stencil.a == occ.z della shader skip Solo l'auto-ombra.
/// (Con il child, il GlobalTransform del child non rifletteva la z del
/// parent e l'uguaglianza falliva -> pianeti neri, v0.14.59-60.)
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
        // Occluder2d::circle ha gia' z_sorting: true di default (from_shape)
        commands
            .entity(entity)
            .insert((FireflyOccluderAttached, Occluder2d::circle(body.radius)));
    }
}

/// Il raggio dell'occluder deve seguire il raggio del corpo (che l'utente
/// può cambiare dal property panel). Ora l'occluder è sullo STESSO entity
/// del corpo: niente Children da percorrere.
fn sync_occluders(
    mut planets: Query<(&CelestialBody, &mut Occluder2d), With<FireflyOccluderAttached>>,
) {
    for (body, mut occ) in planets.iter_mut() {
        if body.luminous {
            continue;
        }
        *occ = Occluder2d::circle(body.radius);
    }
}

/// z = body-id UNICO per corpo (index): la sprite e il suo occluder child
/// condividono lo stesso z -> lo shader (patch v0.14.59) skippa SOLO
/// l'auto-ombra (uguaglianza stencil.g == occ.z). Con z = -y (worker) o z=0
/// le ombre tra pianeti saltavano o l'auto-ombra oscurava tutto il disco.
/// La stella resta a z di spawn: non riceve ombre dai pianeti.
fn sync_sprite_z(
    mut queries: ParamSet<(
        Query<(&CelestialBody, &mut Transform), With<Sprite>>,
        // stella (leggi Transform per la distanza): ParamSet obbligatorio,
        // le due query accedono entrambe a Transform (B0001 altrimenti)
        Query<(&Transform, &CelestialBody)>,
    )>,
) {
    let nan = Vec2::splat(f32::NAN);
    let star_pos = queries
        .p1()
        .iter()
        .find(|(_, b)| b.luminous)
        .map(|(t, _)| t.translation.truncate())
        .unwrap_or(nan);
    if star_pos == nan {
        return;
    }
    for (body, mut transform) in queries.p0().iter_mut() {
        if body.luminous {
            continue;
        }
        // z = -DISTANZA dalla stella (v0.14.67): semantica z-sort del crate
        // ("chi ha z maggiore non riceve ombre"):
        // - auto-ombra: sprite e occluder dello stesso corpo hanno lo
        //   stesso z -> skip (stencil.g >= occ.z - margin) -> mai neri.
        // - pianeta DAVANTI sul DIETRO: il dietro ha distanza maggiore ->
        //   z piu' basso -> l'ombra geometrica per-pixel si applica:
        //   solo la parte dentro il cono non riceve luce (PARZIALE
        //   SPAZIALE, come richiesto).
        let d = transform.translation.truncate().distance(star_pos);
        transform.translation.z = -d;
    }
}

// ============================================================
// Ticket 19 — star light / glow / ambient apply systems
// ============================================================

impl LightFalloff {
    /// Map our serialisable falloff choice onto the firefly GPU falloff.
    fn into_firefly(&self) -> Falloff {
        match self {
            LightFalloff::InverseSquare => Falloff::InverseSquare { intensity: 0.0 },
            LightFalloff::Linear => Falloff::Linear { intensity: 0.0 },
            LightFalloff::None => Falloff::None,
        }
    }
}

/// Rigenera la glow texture condivisa quando la curva globale (`GlowCurve`,
/// da preset.json) cambia — tipicamente su Reset. Le nuove stelle usano il
/// handle aggiornato a spawn; le già esistenti restano sulla vecchia texture
/// (solo la curva cambia via Reset, che respawna tutti i corpi).
fn ensure_glow_texture_matches_curve(
    mut textures: ResMut<FireflyTextures>,
    curve: Res<GlowCurve>,
    mut images: ResMut<Assets<Image>>,
) {
    if curve.is_changed() || textures.curve != *curve {
        textures.glow = images.add(generate_radial_glow_texture(128, &curve));
        textures.curve = curve.clone();
    }
}

/// Sincronizza l'ambient firefly dalla risorsa `AmbientLight` (preset.json)
/// quando cambia. Il `range` (gating per distanza dalla stella più vicina)
/// non è esprimibile nella config firefly globale, quindi viene persistito ma
/// non applicato per-pixel (range=0 default = nessun limite, comportamento
/// storico).
fn apply_ambient_light(
    ambient: Res<AmbientLight>,
    mut cameras: Query<&mut FireflyConfig, With<FireflyCamera>>,
) {
    if !ambient.is_changed() {
        return;
    }
    let color = Color::srgb(ambient.color[0], ambient.color[1], ambient.color[2]);
    for mut cfg in &mut cameras {
        cfg.ambient_brightness = ambient.intensity;
        cfg.ambient_color = color;
    }
}

/// Applica le impostazioni luce (intensity/radius/falloff/fade/core) dalla
/// stella alla sua PointLight2d child quando `StarLightSettings` cambia
/// (edit live dal pannello). Allo spawn i valori sono già corretti (li legge
/// `spawn_star_lights`), quindi qui basta il Changed.
fn apply_star_light_settings(
    bodies: Query<(&CelestialBody, &Children, &StarLightSettings), (Changed<StarLightSettings>, With<FireflyLightAttached>)>,
    mut lights: Query<&mut PointLight2d>,
) {
    for (body, children, s) in &bodies {
        for child in children.iter() {
            let Ok(mut light) = lights.get_mut(child) else {
                continue;
            };
            light.intensity = s.intensity;
            light.radius = s.radius;
            light.falloff = s.falloff.into_firefly();
            light.fade_width = s.fade_width;
            light.core.radius = body.radius * s.core_radius_factor;
            light.core.boost = s.core_boost;
        }
    }
}

/// Applica le impostazioni glow (inner/outer scale+alpha ai child Sprite)
/// quando `StarGlow` cambia (edit live dal pannello). Idempotente.
///
/// UNA sola query `&mut Sprite` (con `Has<FireflyGlowInner/Outer>` per
/// distinguere i due glow): due query `&mut Sprite` separate sullo stesso
/// componente erano un B0001 all'inizializzazione (panic su WASM
/// "Unreachable code", v0.14.70).
///
/// v0.14.77 (Ticket 20, opzione C — alone decorativo puro): la mappa di luce
/// firefly moltiplica anche gli sprite glow (light_frag ∝ intensity), quindi
/// con intensity=2 l'alone raddoppia di brillantezza. L'alpha scritta nello
/// Sprite è PRE-DIVISA per intensity (helper `glow_alpha`): la moltiplicazione
/// della lightmap annulla la divisione e l'alone resta visivamente costante.
/// Con intensity=0 (stella spenta) l'alpha è 0: alone invisibile.
///
/// La query reagisce anche a `Changed<StarLightSettings>`: se l'utente cambia
/// SOLO intensity (senza toccare il glow), l'alpha compensata va ricalcolata.
fn apply_star_glow_settings(
    stars: Query<
        (&CelestialBody, &Children, &StarGlow, &StarLightSettings),
        (
            Or<(Changed<StarGlow>, Changed<StarLightSettings>)>,
            With<FireflySpriteAttached>,
        ),
    >,
    mut glows: Query<(&mut Sprite, Has<FireflyGlowInner>, Has<FireflyGlowOuter>)>,
) {
    for (body, children, g, light) in &stars {
        let inner_alpha = glow_alpha(g.inner_alpha, light.intensity);
        let outer_alpha = glow_alpha(g.outer_alpha, light.intensity);
        for child in children.iter() {
            let Ok((mut sp, is_inner, is_outer)) = glows.get_mut(child) else {
                continue;
            };
            match (is_inner, is_outer) {
                (true, false) => {
                    sp.custom_size = Some(Vec2::splat(body.radius * 2.0 * g.inner_scale));
                    sp.color = Color::srgba(body.color[0], body.color[1], body.color[2], inner_alpha);
                }
                (false, true) => {
                    sp.custom_size = Some(Vec2::splat(body.radius * 2.0 * g.outer_scale));
                    sp.color = Color::srgba(body.color[0], body.color[1], body.color[2], outer_alpha);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_body(world: &mut World, name: &str, pos: Vec2, radius: f32, luminous: bool) {
        world.spawn((
            CelestialBody {
                name: name.into(),
                body_type: crate::components::celestial::BodyType::Planet,
                mass: 100.0,
                radius,
                color: [0.5, 0.5, 0.5],
                luminous,
            },
            Transform::from_xyz(pos.x, pos.y, 0.0),
            Sprite {
                color: Color::srgba(0.5, 0.5, 0.5, 1.0),
                ..default()
            },
        ));
    }

    #[test]
    fn z_neg_dist_stella_ordina_davanti_dietro() {
        let mut app = bevy::prelude::App::new();
        app.add_systems(bevy::prelude::Update, sync_sprite_z);
        {
            let mut world = app.world_mut();
            spawn_body(&mut world, "Star", Vec2::ZERO, 30.0, true);
            spawn_body(&mut world, "Front", Vec2::new(150.0, 0.0), 15.0, false);
            spawn_body(&mut world, "Back", Vec2::new(300.0, 0.0), 15.0, false);
        }
        app.update();
        let mut q = app.world_mut().query::<(&CelestialBody, &Transform)>();
        let mut z_front = 0.0f32;
        let mut z_back = 0.0f32;
        for (b, t) in q.iter(app.world()) {
            if b.name == "Front" {
                z_front = t.translation.z;
            }
            if b.name == "Back" {
                z_back = t.translation.z;
            }
        }
        assert!(z_front < 0.0 && z_back < 0.0, "z negativi ({z_front}, {z_back})");
        assert!(
            z_back < z_front - 1.0,
            "il pianeta dietro deve avere z PIU' BASSO (riceve l'ombra): {z_front} vs {z_back}"
        );
    }

    // ---- glow_alpha (Ticket 20, v0.14.77): compensazione intensity ----

    #[test]
    fn glow_alpha_identity_at_intensity_one() {
        // A intensity=1 la compensazione è l'identità.
        assert!((glow_alpha(0.55, 1.0) - 0.55).abs() < 1e-6);
    }

    #[test]
    fn glow_alpha_compensates_high_intensity() {
        // intensity=4: l'alpha si pre-divide per 4 -> la lightmap (×4) annulla
        // la divisione e l'alone resta costante.
        assert!((glow_alpha(0.55, 4.0) - 0.1375).abs() < 1e-6);
    }

    #[test]
    fn glow_alpha_clamps_to_one_on_low_intensity() {
        // intensity=0.5: 0.55/0.5 = 1.1 -> clamp in alto a 1.0.
        assert_eq!(glow_alpha(0.55, 0.5), 1.0);
    }

    #[test]
    fn glow_alpha_zero_intensity_hides_glow() {
        // Stella spenta: alone invisibile (NON alpha=1 — pitfall del ticket).
        assert_eq!(glow_alpha(0.55, 0.0), 0.0);
    }
}
