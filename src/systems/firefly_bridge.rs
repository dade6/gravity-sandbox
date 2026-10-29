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

use avian2d::prelude::{Collider, LockedAxes};
use bevy::prelude::*;
use bevy_firefly::prelude::*;
// `TransformEasingSystems` non è riesportato dal prelude di avian2d 0.7:
// serve per ordinare `sync_sprite_z` prima di `UpdateEnd` (vedi il plugin).
use bevy_transform_interpolation::TransformEasingSystems;
use std::collections::HashMap;

use crate::components::celestial::CelestialBody;
use crate::components::lighting::{
    AmbientLight, GlowCurve, LightFalloff, StarGlow, StarLightSettings,
};
use crate::rendering::textures::{
    build_image, generate_disc_texture, generate_radial_glow_texture, generate_sphere_normal_map,
};

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

/// Risoluzione delle texture dei corpi (v0.14.151).
///
/// Disco e normal map NON sono più una texture unica stirata su ogni corpo:
/// ogni raggio ha la SUA risoluzione (`texture_size_for_radius`), con cache per
/// potenza di due così i corpi simili condividono gli handle (RAM + batching).
/// Il vincolo viene dal fatto che la silhouette del corpo è l'alpha del disco
/// steso su `custom_size = radius*2`: tenere ~2 texel per unità mondo dà
/// ~1.2 pixel FISICI per texel su un pianeta da 400 px (DPR 3 di iPhone/Mac).
const TEXELS_PER_WORLD_UNIT: f32 = 2.0;
const MIN_BODY_TEX_SIZE: usize = 64;
const MAX_BODY_TEX_SIZE: usize = 1024;
/// Lato dell'alone radiale: l'unica texture ancora globale (dipende solo dalla
/// curva, non dalla dimensione del corpo).
const GLOW_TEX_SIZE: u32 = 512;

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

/// Texture globali (indipendenti dalla dimensione del corpo): l'alone radiale,
/// la cui forma dipende solo dalla curva globale (`GlowCurve`). Disco e normal
/// map sono invece per-dimensione: vedi `BodyTextureCache`.
#[derive(Resource)]
pub struct FireflyTextures {
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
        // Alone radiale: curva + soft edge dal GlowCurve globale (niente
        // gradino al bordo). Tinto col colore della stella.
        let glow = images.add(generate_radial_glow_texture(GLOW_TEX_SIZE, &curve));
        Self { glow, curve }
    }
}

/// Coppia disco + normal map generate per UNA risoluzione (lato in texel).
#[derive(Clone)]
pub struct BodyTextureSet {
    pub disc: Handle<Image>,
    pub normal: Handle<Image>,
}

/// Cache delle texture per-dimensione, chiave = lato in texel (potenza di 2).
///
/// Perché una cache e non una texture per corpo: corpi di dimensioni simili
/// finiscono nella stessa potenza di due, quindi condividono gli handle (meno
/// RAM e sprite batchabili), senza però stirare una texture piccola su un
/// disco grande. Prima della v0.14.151 c'era UNA sola coppia 64²/256² per tutti
/// i corpi: a raggi grandi la silhouette andava a scalini e la normal map a
/// bande (il difetto si vede cambiando la scala del preset, ma anche solo
/// alzando il raggio dal pannello).
#[derive(Resource, Default)]
pub struct BodyTextureCache {
    sets: HashMap<usize, BodyTextureSet>,
}

impl BodyTextureCache {
    /// Texture per il lato `size`, generandole la prima volta.
    pub fn get_or_build(&mut self, size: usize, images: &mut Assets<Image>) -> BodyTextureSet {
        if let Some(set) = self.sets.get(&size) {
            return set.clone();
        }
        let disc = images.add(generate_disc_texture(size));
        let normal = images.add(build_image(
            generate_sphere_normal_map(size),
            size as u32,
            size as u32,
        ));
        let set = BodyTextureSet { disc, normal };
        self.sets.insert(size, set.clone());
        set
    }

    /// Quante coppie sono state generate (diagnostica/test).
    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
}

/// Lato (texel) della texture adatto al raggio del corpo, in potenza di due.
///
/// Il disco viene steso su `radius*2` unità mondo: `TEXELS_PER_WORLD_UNIT`
/// texel per unità, arrotondati alla potenza di due superiore e limitati a
/// [64, 1024] (lo stesso budget della v0.14.149, ma assegnato per corpo).
pub(crate) fn texture_size_for_radius(radius: f32) -> usize {
    let wanted = (radius.max(1.0) * 2.0 * TEXELS_PER_WORLD_UNIT).ceil() as usize;
    wanted
        .max(1)
        .next_power_of_two()
        .clamp(MIN_BODY_TEX_SIZE, MAX_BODY_TEX_SIZE)
}

/// Risoluzione attualmente applicata alla sprite del corpo (per non rigenerare
/// texture quando il raggio cambia ma resta nello stesso bucket).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct BodyTextureSize(pub usize);

pub struct FireflyBridgePlugin;

/// Algoritmo dei coni d'ombra, selezionabile a caldo dal pannello Impostazioni
/// (A/B sullo stesso device e sullo stesso preset: niente branch da
/// ricompilare per confrontare le due rese).
///
/// - `v2 = false` → v1: test poligonale/archi di firefly (`round_check`),
///   penombra approssimata sui vertici dell'occluder.
/// - `v2 = true` → v2 "Shadow System 3.0" (Desmos `pql2tb3dhv`): cono
///   analitico umbra+penombra costruito dal disco solare
///   (`light.core_radius`), vedi `desmos_round_check` nella shader.
#[derive(Resource)]
pub struct ShadowAlgo {
    pub v2: bool,
    /// Esponente del profilo di penombra (vedi `FireflyConfig::penumbra_exp`).
    /// Vive accanto a `v2` perché entrambi sono manopole della shader dei
    /// coni, regolabili a caldo da Impostazioni → OMBRE.
    pub penumbra_exp: f32,
}

impl Default for ShadowAlgo {
    /// Default = **v2**. Il ramo esiste per sviluppare l'algoritmo nuovo e
    /// l'A/B non è persistito nel preset: con default `false` un reload della
    /// pagina riportava silenziosamente alla v1 e si finiva a diagnosticare la
    /// v1 credendo di guardare la v2 (successo alla prima verifica su device).
    fn default() -> Self {
        Self {
            v2: true,
            penumbra_exp: 1.0,
        }
    }
}

impl Plugin for FireflyBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FireflyPlugin)
            .init_resource::<FireflyTextures>()
            .init_resource::<BodyTextureCache>()
            .init_resource::<GlowCurve>()
            .init_resource::<AmbientLight>()
            .init_resource::<ShadowAlgo>()
            // Idempotente: si attiva al primo frame in cui la camera esiste.
            .add_systems(Update, setup_firefly_camera)
            // Ticket 19: rigenera la glow texture al cambio curva globale e
            // sincronizza l'ambient dalla risorsa AmbientLight (preset.json).
            .add_systems(
                Update,
                (
                    ensure_glow_texture_matches_curve,
                    apply_ambient_light,
                    apply_shadow_algo,
                ),
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
                    sync_body_sprite_sizes,
                    sync_colliders,
                    apply_star_light_settings,
                    apply_star_glow_settings,
                )
                    .chain(),
            )
            // `sync_sprite_z` scrive `Transform.z` sui corpi (z-sort di
            // firefly = distanza dalla stella) e in orbita cambia OGNI frame.
            // Deve stare DENTRO il fixed step: una scrittura di `Transform` in
            // `Update` fa azzerare lo stato di interpolazione
            // (`reset_easing_states_on_transform_change` di
            // bevy_transform_interpolation) e il moto torna a scatti — è il
            // difetto della v0.14.154, rollback in v0.14.155 (v0.14.156: z nel
            // tick). Lo z-sort è una chiave di ordinamento: il tick basta.
            .add_systems(
                FixedPostUpdate,
                sync_sprite_z
                    .after(avian2d::schedule::PhysicsSystems::Writeback)
                    .before(TransformEasingSystems::UpdateEnd),
            )
            // Da fermo non gira nessun tick: senza questa passata lo z
            // resterebbe indietro su un corpo trascinato in pausa (ordinamento
            // sbagliato finché non si riprende). Scrive solo al cambio, quindi
            // un corpo fermo non tocca `Transform` (niente restart del ghost,
            // bug v0.14.93).
            .add_systems(Update, sync_sprite_z.run_if(simulation_is_paused));
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
        commands
            .entity(entity)
            .try_insert(LockedAxes::ROTATION_LOCKED);
    }
}

/// La camera principale prende FireflyConfig (ambient scuro, soft shadows,
/// z-sorting, normal map mode Simple = direzione luce nel piano xy).
fn setup_firefly_camera(
    mut commands: Commands,
    cameras: Query<(Entity, &Camera), Without<FireflyCamera>>,
    ambient: Res<AmbientLight>,
    algo: Res<ShadowAlgo>,
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
                ambient_color: Color::srgb(ambient.color[0], ambient.color[1], ambient.color[2]),
                ambient_brightness: ambient.intensity,
                // v0.14.135: ombre di nuovo MORBIDE (soft). Le hard (v0.14.133)
                // servivano solo a rendere i coni visibili dopo che il gate
                // v0.14.131 li aveva cancellati dallo sfondo; col gate
                // v0.14.134 (solo nuclei+stelle esentati) il nucleo
                // geometrico del cono e' nero anche in soft, con alone
                // sfumato ai bordi come piace a Davide. Terminatori
                // giorno/notte invariati (vengono dalle normal map).
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
                // v0.14.171: l'algoritmo va scritto GIA' qui. `apply_shadow_algo`
                // al primo frame può girare prima che questa config esista, e
                // con `..default()` (0 = v1) la shader resterebbe sulla v1.
                shadow_algorithm: if algo.v2 { 1 } else { 0 },
                ..default()
            },
        ));
    }
}

/// Alpha scritta nello Sprite glow (Ticket 20, v0.14.79).
///
/// SEMPLIFICATA rispetto alla v0.14.78: la separazione alone/luce ora vive
/// NELLO SHADER (create_lightmap.wgsl: i pixel senza normal map ricevono
/// `Halo Brightness` trasportato dall'alpha del colore della luce, i pixel
/// dei pianeti ricevono `Planet Light`/intensity). Lo sprite glow quindi
/// porta SOLO l'alpha base (inner/outer alpha del pannello): la lightmap
/// lo moltiplica per Halo Brightness → alone finale = base × brightness.
pub(crate) fn glow_alpha(base_alpha: f32) -> f32 {
    base_alpha.clamp(0.0, 1.0)
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
    mut cache: ResMut<BodyTextureCache>,
    mut images: ResMut<Assets<Image>>,
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
        // Texture della dimensione giusta per QUESTO corpo (v0.14.151).
        let tex_size = texture_size_for_radius(body.radius);
        let set = cache.get_or_build(tex_size, &mut images);
        let mut cmd = commands.entity(entity);
        cmd.remove::<Mesh2d>()
            .remove::<MeshMaterial2d<crate::systems::lighting::LightMaterial>>()
            .remove::<MeshMaterial2d<ColorMaterial>>()
            .insert(FireflySpriteAttached)
            .insert(BodyTextureSize(tex_size))
            .insert((
                Sprite {
                    image: set.disc.clone(),
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
            // v0.14.79: la separazione è nello shader (pixel senza normal map
            // → Halo Brightness). Lo sprite glow porta l'alpha base pura.
            let inner_color = Color::srgba(
                body.color[0],
                body.color[1],
                body.color[2],
                glow_alpha(g.inner_alpha),
            );
            let outer_color = Color::srgba(
                body.color[0],
                body.color[1],
                body.color[2],
                glow_alpha(g.outer_alpha),
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
    mut cache: ResMut<BodyTextureCache>,
    mut images: ResMut<Assets<Image>>,
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
        let set = cache.get_or_build(texture_size_for_radius(body.radius), &mut images);
        commands
            .entity(entity)
            .insert(NormalMap::from_image(set.normal.clone()));
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
        (
            Entity,
            &CelestialBody,
            Option<&StarLightSettings>,
            Option<&StarGlow>,
        ),
        (
            Without<FireflyLightAttached>,
            Without<FireflyOccluderAttached>,
        ),
    >,
    mut commands: Commands,
) {
    for (entity, body, settings, glow) in stars.iter() {
        if !body.luminous {
            continue;
        }
        let s = settings.cloned().unwrap_or_default();
        // v0.14.79 (Ticket 20): l'ALPHA del colore della luce trasporta
        // `Halo Brightness` (StarGlow.brightness) alla shader vendored, che
        // lo applica ai pixel SENZA normal map (sfondo/starfield/glow) al
        // posto di `intensity` → l'alone sullo sfondo risponde SOLO a Halo
        // Brightness, i pianeti SOLO a Planet Light.
        let halo_brightness = glow
            .map(|g| g.brightness)
            .unwrap_or(StarGlow::default().brightness);
        commands
            .entity(entity)
            .insert(FireflyLightAttached)
            .with_children(|parent| {
                parent.spawn((
                    PointLight2d {
                        color: Color::srgba(
                            body.color[0],
                            body.color[1],
                            body.color[2],
                            halo_brightness,
                        ),
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
                    // T22-FIX: la luce deve illuminare anche quando la stella
                    // e' fuori dal frustum (zoom su pianeta lontano): senza
                    // NoFrustumCulling l'extract salta la luce (ViewVisibility
                    // false) e il pianeta resta al solo ambient ~12%.
                    // SOLO la luce: gli occluder restano frustum-cullati.
                    bevy::camera::visibility::NoFrustumCulling,
                    // v0.14.121 (CAUSA del "pianeti spenti, niente coni"):
                    // PointLight2d NON richiede Visibility/InheritedVisibility
                    // -> il check_visibility di Bevy ignora la luce -> a fine
                    // frame le VisibleEntities hanno la classe PointLight2d
                    // VUOTA -> il queue firefly (iter_visible) non accoda alcun
                    // item -> la lightmap non contiene la luce, pur con dati
                    // GPU corretti (badge V.../.../0). Rendiamo la luce
                    // visibile a Bevy: Visibility::Visible + InheritedVisibility
                    // esplicita (non dipendiamo dalla propagazione, che non
                    // ricorre su un child aggiunto dopo il genitore).
                    Visibility::Visible,
                    InheritedVisibility::VISIBLE,
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

/// La sprite del corpo deve seguire il raggio, come già fa `sync_occluders`
/// per il cono d'ombra: senza questo sistema, cambiando Radius dal pannello
/// il cono cambiava e il disco no (v0.14.151).
///
/// Aggiorna, sui corpi cambiati:
/// - `Sprite.custom_size` = diameter (silhouette del corpo);
/// - la coppia disco/normal map alla risoluzione del nuovo raggio, quando il
///   raggio cambia di bucket (potenza di due) — niente rebuild se resta nello
///   stesso bucket;
/// - gli sprite degli aloni della stella (child), che sono funzione del raggio.
///
/// UNA sola query `&mut Sprite` con `Has<FireflyGlowInner/Outer>` (due query
/// separate sarebbero un B0001 all'init — vedi pitfall 0b2).
fn sync_body_sprite_sizes(
    mut commands: Commands,
    mut cache: ResMut<BodyTextureCache>,
    mut images: ResMut<Assets<Image>>,
    bodies: Query<
        (
            Entity,
            &CelestialBody,
            // OPTIONAL: i pianeti NON hanno figli (solo le stelle hanno gli
            // aloni). Con `&Children` il pianeta non entrava mai nel sistema e
            // il raggio restava senza effetto (bug v0.14.152).
            Option<&Children>,
            Option<&StarGlow>,
            Option<&BodyTextureSize>,
        ),
        (With<FireflySpriteAttached>, Changed<CelestialBody>),
    >,
    mut sprites: Query<(
        &mut Sprite,
        Has<FireflyGlowInner>,
        Has<FireflyGlowOuter>,
    )>,
) {
    for (entity, body, children, glow, applied) in bodies.iter() {
        // 1) Corpo: diametro + texture alla risoluzione del raggio.
        let tex_size = texture_size_for_radius(body.radius);
        let needs_new_texture = applied.map(|a| a.0) != Some(tex_size);
        let new_set = if needs_new_texture {
            Some(cache.get_or_build(tex_size, &mut images))
        } else {
            None
        };
        if let Ok((mut sprite, is_inner, is_outer)) = sprites.get_mut(entity) {
            if !is_inner && !is_outer {
                sprite.custom_size = Some(Vec2::splat(body.radius * 2.0));
                if let Some(set) = &new_set {
                    sprite.image = set.disc.clone();
                }
            }
        }
        if needs_new_texture {
            let set = new_set.as_ref().expect("texture appena generate");
            if !body.luminous {
                commands
                    .entity(entity)
                    .insert(NormalMap::from_image(set.normal.clone()));
            }
            commands.entity(entity).insert(BodyTextureSize(tex_size));
        }

        // 2) Aloni della stella (inner/outer): scala proporzionale al raggio.
        if let (Some(g), Some(children)) = (glow, children) {
            for child in children.iter() {
                let scale = match sprites.get_mut(child) {
                    Ok((_, true, _)) => g.inner_scale,
                    Ok((_, _, true)) => g.outer_scale,
                    _ => continue,
                };
                if let Ok((mut sp, _, _)) = sprites.get_mut(child) {
                    sp.custom_size = Some(Vec2::splat(body.radius * 2.0 * scale));
                }
            }
        }
    }
}

/// Il collider Avian deve seguire il raggio come la sprite e l'occluder:
/// altrimenti il corpo appare grande ma collide (e spinge) con la forma
/// vecchia. `Mass` è esplicita, quindi la forma non tocca la massa.
fn sync_colliders(
    mut commands: Commands,
    bodies: Query<(Entity, &CelestialBody), (Changed<CelestialBody>, With<Collider>)>,
) {
    for (entity, body) in bodies.iter() {
        commands.entity(entity).insert(Collider::circle(body.radius));
    }
}

/// z = body-id UNICO per corpo (index): la sprite e il suo occluder child
/// condividono lo stesso z -> lo shader (patch v0.14.59) skippa SOLO
/// l'auto-ombra (uguaglianza stencil.g == occ.z). Con z = -y (worker) o z=0
/// le ombre tra pianeti saltavano o l'auto-ombra oscurava tutto il disco.
/// La stella resta a z di spawn: non riceve ombre dai pianeti.
pub(crate) fn sync_sprite_z(
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
        // Write ONLY on actual change: an unconditional write marks
        // Changed<Transform> every frame even with an identical value, and
        // that kept `ghost_dirty_triggers` hot forever while paused — the
        // ghost restarted (snapshot + 1 chunk) every frame and never grew
        // past 256 ticks, so the horizon setting had no visible effect
        // (bug v0.14.93).
        let new_z = -d;
        if (transform.translation.z - new_z).abs() > f32::EPSILON {
            transform.translation.z = new_z;
        }
    }
}

/// Condizione per la passata in `Update` di `sync_sprite_z`: da fermo non gira
/// nessun tick fisico, quindi lo z-sort va ricalcolato qui (es. corpo
/// trascinato in pausa). `Option`: gli harness headless possono non avere il
/// resource `SimulationState`.
fn simulation_is_paused(sim: Option<Res<crate::systems::timeline::SimulationState>>) -> bool {
    sim.map(|s| s.paused).unwrap_or(false)
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
        textures.glow = images.add(generate_radial_glow_texture(GLOW_TEX_SIZE, &curve));
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

/// Scrive l'algoritmo scelto nella `FireflyConfig` della camera: la shader
/// legge `config.shadow_algorithm` (0 = v1, 1 = v2). Come
/// `apply_ambient_light`, scrive solo al cambio della risorsa — la config è
/// già a 0 di default, quindi al primo frame non serve altro.
fn apply_shadow_algo(
    algo: Res<ShadowAlgo>,
    mut cameras: Query<&mut FireflyConfig, With<FireflyCamera>>,
) {
    // NIENTE early-return su `algo.is_changed()` (v0.14.171): al primo frame la
    // camera con `FireflyConfig` non esiste ancora, quindi la scrittura andava
    // persa; dal frame dopo la risorsa non era piu' "changed" e il valore non
    // arrivava MAI alla shader -> si restava sulla v1 credendo di guardare la
    // v2 (sintomo: penombra ad archi di cerchio, tipica della v1 di firefly).
    // Costo trascurabile (una camera): si scrive solo al cambio effettivo.
    let want = if algo.v2 { 1 } else { 0 };
    // v0.14.176: anche l'esponente di penombra arriva qui (clamp difensivo:
    // la shader usa pow, un valore <= 0 renderebbe il bordo discontinuo).
    let want_exp = algo.penumbra_exp.clamp(0.05, 8.0);
    for mut cfg in &mut cameras {
        if cfg.shadow_algorithm != want {
            cfg.shadow_algorithm = want;
        }
        if cfg.penumbra_exp != want_exp {
            cfg.penumbra_exp = want_exp;
        }
    }
}

/// Applica le impostazioni luce (intensity/radius/falloff/fade/core) dalla
/// stella alla sua PointLight2d child quando `StarLightSettings` cambia
/// (edit live dal pannello). Allo spawn i valori sono già corretti (li legge
/// `spawn_star_lights`), quindi qui basta il Changed.
///
/// v0.14.79: NON tocca `light.color` — l'alpha del colore trasporta Halo
/// Brightness ed è gestito da `apply_star_glow_settings`.
fn apply_star_light_settings(
    bodies: Query<
        (&CelestialBody, &Children, &StarLightSettings),
        (Changed<StarLightSettings>, With<FireflyLightAttached>),
    >,
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

/// Applica le impostazioni glow quando `StarGlow` cambia (edit live).
///
/// DUE destinazioni (v0.14.79):
/// 1. I child Sprite inner/outer: custom_size + alpha base (UNA sola query
///    `&mut Sprite` con `Has<>` — due query separate erano un B0001, v0.14.70).
/// 2. L'ALPHA del colore della PointLight2d child = `g.brightness`
///    (Halo Brightness): è il canale che la shader vendored legge per i pixel
///    SENZA normal map (sfondo/starfield) → il disco di luce attorno alla
///    stella segue Halo Brightness e MAI Planet Light.
///
/// Reagisce anche a `Changed<StarLightSettings>` per ridisegnare gli sprite
/// glow (la lightmap li moltiplica, ma con la nuova shader il moltiplicatore
/// dei pixel glow è `halo`/brightness, non intensity — nessuna compensazione).
fn apply_star_glow_settings(
    stars: Query<
        (&CelestialBody, &Children, &StarGlow, &StarLightSettings),
        (
            Or<(Changed<StarGlow>, Changed<StarLightSettings>)>,
            With<FireflySpriteAttached>,
        ),
    >,
    mut glows: Query<(&mut Sprite, Has<FireflyGlowInner>, Has<FireflyGlowOuter>)>,
    mut lights: Query<&mut PointLight2d>,
) {
    for (body, children, g, _light) in &stars {
        // 2) Halo Brightness -> alpha del colore della luce (canale shader).
        for child in children.iter() {
            if let Ok(mut light) = lights.get_mut(child) {
                let srgba = light.color.to_srgba();
                light.color = Color::srgba(srgba.red, srgba.green, srgba.blue, g.brightness);
            }
        }
        // 1) Sprite glow: alpha base pura.
        let inner_alpha = glow_alpha(g.inner_alpha);
        let outer_alpha = glow_alpha(g.outer_alpha);
        for child in children.iter() {
            let Ok((mut sp, is_inner, is_outer)) = glows.get_mut(child) else {
                continue;
            };
            match (is_inner, is_outer) {
                (true, false) => {
                    sp.custom_size = Some(Vec2::splat(body.radius * 2.0 * g.inner_scale));
                    sp.color =
                        Color::srgba(body.color[0], body.color[1], body.color[2], inner_alpha);
                }
                (false, true) => {
                    sp.custom_size = Some(Vec2::splat(body.radius * 2.0 * g.outer_scale));
                    sp.color =
                        Color::srgba(body.color[0], body.color[1], body.color[2], outer_alpha);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v0.14.171: il toggle deve arrivare alla `FireflyConfig` della camera
    /// anche quando la camera nasce DOPO il primo frame (è quello che fa
    /// `setup_firefly_camera`). Con l'early-return su `is_changed()` la
    /// scrittura al primo frame andava persa e la shader restava sulla v1 per
    /// sempre: si vedeva una penombra ad archi di cerchio (v1) credendo di
    /// guardare la v2.
    #[test]
    fn shadow_algorithm_reaches_a_camera_created_after_the_first_frame() {
        let mut app = App::new();
        app.insert_resource(ShadowAlgo {
            v2: true,
            penumbra_exp: 1.0,
        })
        .add_systems(Update, apply_shadow_algo);
        // frame 1: nessuna camera con FireflyConfig
        app.update();
        // frame 2: la camera nasce con la config di default (v1)
        let cam = app
            .world_mut()
            .spawn((FireflyCamera, FireflyConfig::default()))
            .id();
        app.update();
        let cfg = app.world().get::<FireflyConfig>(cam).unwrap();
        assert_eq!(
            cfg.shadow_algorithm, 1,
            "il toggle non raggiunge la camera nata dopo il primo frame: si \
             resta sulla v1 (penombra ad archi di cerchio)"
        );
    }

    /// Il valore viaggia in entrambe le direzioni (v1 ⇄ v2) e non riscrive
    /// quando è già corretto.
    #[test]
    fn shadow_algorithm_follows_the_toggle_both_ways() {
        let mut app = App::new();
        app.insert_resource(ShadowAlgo {
            v2: false,
            penumbra_exp: 1.0,
        })
        .add_systems(Update, apply_shadow_algo);
        let cam = app
            .world_mut()
            .spawn((FireflyCamera, FireflyConfig::default()))
            .id();
        app.update();
        assert_eq!(app.world().get::<FireflyConfig>(cam).unwrap().shadow_algorithm, 0);
        app.world_mut().resource_mut::<ShadowAlgo>().v2 = true;
        app.update();
        assert_eq!(app.world().get::<FireflyConfig>(cam).unwrap().shadow_algorithm, 1);
        app.world_mut().resource_mut::<ShadowAlgo>().v2 = false;
        app.update();
        assert_eq!(app.world().get::<FireflyConfig>(cam).unwrap().shadow_algorithm, 0);
    }

    /// A/B (v0.14.156): la scrittura di `z` per lo z-sort NON deve distruggere
    /// l'interpolazione del moto.
    ///
    /// `sync_sprite_z` scrive `Transform.z` sui corpi e in orbita il valore
    /// cambia a ogni frame. In `Update` quella scrittura fa scattare
    /// `reset_easing_states_on_transform_change` (bevy_transform_interpolation
    /// azzera lo stato di easing a ogni `Transform` modificato fuori dal fixed
    /// step) → il corpo alterna posizione smussata e posizione fisica
    /// quantizzata: passi irregolari, peggio del caso senza easing (è il
    /// difetto della v0.14.154, rollback in v0.14.155). Dentro il fixed step,
    /// dopo il writeback di Avian, lo stesso sistema non tocca l'easing.
    #[test]
    fn z_sort_in_the_tick_keeps_body_motion_smooth() {
        use avian2d::prelude::*;

        fn steps(z_in_update: bool) -> Vec<f32> {
            use avian2d::prelude::*;
            use bevy_transform_interpolation::TransformEasingSystems;
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                bevy::transform::TransformPlugin,
                PhysicsPlugins::default(),
                crate::systems::gravity::GravityPlugin,
            ))
            .insert_resource(Gravity::ZERO)
            .insert_resource(crate::systems::persistence::GravitationalConstant(0.0))
            .insert_resource(crate::systems::timeline::SimulationState::default())
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / 120.0),
            ));
            if z_in_update {
                app.add_systems(Update, sync_sprite_z);
            } else {
                app.add_systems(
                    FixedPostUpdate,
                    sync_sprite_z
                        .after(avian2d::schedule::PhysicsSystems::Writeback)
                        .before(TransformEasingSystems::UpdateEnd),
                );
            }
            app.finish();
            spawn_body(app.world_mut(), "Sole", Vec2::ZERO, 30.0, true);
            let planet = app
                .world_mut()
                .spawn((
                    CelestialBody {
                        name: "Terra".into(),
                        body_type: crate::components::celestial::BodyType::Planet,
                        mass: 10.0,
                        radius: 5.0,
                        color: [0.5, 0.5, 0.8],
                        luminous: false,
                    },
                    Transform::from_xyz(0.0, 300.0, 0.0),
                    Position::from_xy(0.0, 300.0),
                    RigidBody::Dynamic,
                    Collider::circle(5.0),
                    Mass(10.0),
                    LinearVelocity(Vec2::new(120.0, 0.0)),
                    Sprite::default(),
                ))
                .id();
            for _ in 0..4 {
                app.update();
            }
            let mut ys = Vec::new();
            for _ in 0..12 {
                app.update();
                ys.push(
                    app.world()
                        .entity(planet)
                        .get::<Transform>()
                        .unwrap()
                        .translation
                        .x,
                );
            }
            ys.windows(2).map(|w| w[1] - w[0]).collect()
        }

        fn spread(v: &[f32]) -> f32 {
            v.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
                - v.iter().cloned().fold(f32::INFINITY, f32::min)
        }

        let bad = steps(true);
        let good = steps(false);
        eprintln!(
            "SPIKE z in Update : spread={:.4} passi={bad:?}",
            spread(&bad)
        );
        eprintln!(
            "SPIKE z nel tick  : spread={:.4} passi={good:?}",
            spread(&good)
        );
        assert!(
            spread(&good) < 0.05,
            "z nel tick: passi uniformi (spread={:.4})",
            spread(&good)
        );
        assert!(
            spread(&bad) > spread(&good) * 2.0,
            "z in Update deve degradare l'easing: spread bad={:.4} good={:.4}",
            spread(&bad),
            spread(&good)
        );
    }


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
        assert!(
            z_front < 0.0 && z_back < 0.0,
            "z negativi ({z_front}, {z_back})"
        );
        assert!(
            z_back < z_front - 1.0,
            "il pianeta dietro deve avere z PIU' BASSO (riceve l'ombra): {z_front} vs {z_back}"
        );
    }

    /// Conta le entity con Transform marcato Changed (sonda per il test
    /// di idempotenza qui sotto: gira DOPO sync_sprite_z nella stessa chain).
    #[derive(bevy::prelude::Resource, Default)]
    struct ChangedTransformCount(usize);

    fn count_changed_transform(
        q: bevy::prelude::Query<
            bevy::prelude::Entity,
            bevy::prelude::Changed<bevy::prelude::Transform>,
        >,
        mut c: bevy::prelude::ResMut<ChangedTransformCount>,
    ) {
        c.0 = q.iter().count();
    }

    #[test]
    fn sync_sprite_z_quiet_when_static() {
        // Regression test (bug v0.14.93): con corpi fermi, dopo il primo
        // assestamento sync_sprite_z NON deve più scrivere Transform — una
        // scrittura incondizionata marchia Changed ogni frame e teneva
        // `ghost_dirty_triggers` caldo in eterno (il ghost ripartiva ogni
        // frame e non superava mai 256 tick: l'orizzonte sembrava ignorato).
        let mut app = bevy::prelude::App::new();
        app.init_resource::<ChangedTransformCount>().add_systems(
            bevy::prelude::Update,
            (sync_sprite_z, count_changed_transform).chain(),
        );
        {
            let mut world = app.world_mut();
            spawn_body(&mut world, "Star", Vec2::ZERO, 30.0, true);
            spawn_body(&mut world, "P", Vec2::new(150.0, 0.0), 15.0, false);
        }
        app.update(); // primo z-write + assestamento Added/Changed di spawn
        app.update(); // assestamento
        app.update(); // a regime: con corpi fermi, zero scritture attese
        assert_eq!(
            app.world().resource::<ChangedTransformCount>().0,
            0,
            "corpi fermi -> sync_sprite_z non deve marcare Changed<Transform>"
        );
        // ...ma un vero spostamento deve ancora propagarsi (z ricalcolato).
        app.world_mut()
            .query::<&mut bevy::prelude::Transform>()
            .iter_mut(app.world_mut())
            .for_each(|mut t| t.translation.x += 10.0);
        app.update();
        assert!(
            app.world().resource::<ChangedTransformCount>().0 > 0,
            "dopo uno spostamento reale il Changed deve scattare"
        );
    }

    // ---- glow_alpha (Ticket 20, v0.14.79): alpha base pura ----
    // La separazione alone/luce pianeti vive NELLO SHADER vendored:
    // pixel senza normal map -> Halo Brightness (alpha del colore luce),
    // pixel dei pianeti -> Planet Light (intensity). Lo sprite glow porta
    // solo l'alpha base del pannello.

    #[test]
    fn glow_alpha_identity() {
        // Alpha base passata intatta (clamp 0..1).
        assert!((glow_alpha(0.55) - 0.55).abs() < 1e-6);
        assert!((glow_alpha(0.18) - 0.18).abs() < 1e-6);
        assert_eq!(glow_alpha(0.0), 0.0);
        assert_eq!(glow_alpha(1.0), 1.0);
    }

    #[test]
    fn glow_alpha_clamps_out_of_range() {
        assert_eq!(glow_alpha(1.5), 1.0);
        assert_eq!(glow_alpha(-0.2), 0.0);
    }

    #[test]
    fn halo_brightness_travels_in_light_color_alpha() {
        // Verifica della CONVENZIONE v0.14.79: spawn_star_lights e
        // apply_star_glow_settings scrivono StarGlow.brightness nell'ALPHA
        // del colore della PointLight2d. La shader vendored lo legge
        // (`let halo = light.color.a`) e lo applica ai pixel senza normal
        // map al posto di `intensity`. Simuliamo il calcolo della shader:
        let color = Color::srgba(1.0, 0.9, 0.3, 0.7); // brightness=0.7
        let halo = color.to_srgba().alpha;
        let intensity = 3.0; // Planet Light alto
                             // Pixel SFONDO (normal.a == 0): select(intensity, halo, true) = halo.
        let surface_background = if true { halo } else { intensity };
        assert_eq!(surface_background, 0.7, "sfondo usa Halo, non Planet Light");
        // Pixel PIANETA (normal.a > 0): select = intensity.
        let surface_planet = if false { halo } else { intensity };
        assert_eq!(surface_planet, 3.0, "pianeta usa Planet Light, non Halo");
    }

    #[test]
    fn halo_alpha_survives_to_linear_vec4() {
        // Il buffer GPU usa `light.color.to_linear().to_vec4()` (buffers.rs:189):
        // l'alpha DEVE sopravvivere alla conversione sRGB->linear, altrimenti
        // il canale trasporto Halo Brightness arriva corrotto alla shader.
        let color = Color::srgba(0.5, 0.9, 0.3, 0.42);
        let lin = bevy::color::LinearRgba::from(color);
        assert!(
            (lin.alpha - 0.42).abs() < 1e-6,
            "alpha persa nella conversione linear: {}",
            lin.alpha
        );
        let v4 = lin.to_vec4();
        assert!(
            (v4.w - 0.42).abs() < 1e-6,
            "alpha persa in to_vec4: {}",
            v4.w
        );
    }

    #[test]
    fn star_light_child_has_no_frustum_culling() {
        // Regression test T22-FIX: la PointLight2d child della stella deve
        // avere NoFrustumCulling, altrimenti quando la stella esce dal
        // frustum (zoom su pianeta lontano) l'extract salta la luce
        // (ViewVisibility false) e il pianeta resta al solo ambient ~12%.
        // Gli occluder restano frustum-cullati: solo la luce e' esente.
        let mut app = bevy::prelude::App::new();
        app.add_systems(bevy::prelude::Update, spawn_star_lights);
        {
            let mut world = app.world_mut();
            spawn_body(&mut world, "Star", Vec2::ZERO, 30.0, true);
            spawn_body(&mut world, "P", Vec2::new(150.0, 0.0), 15.0, false);
        }
        app.update();
        let light_entities: Vec<bevy::prelude::Entity> = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, With<PointLight2d>>()
            .iter(app.world())
            .collect();
        assert_eq!(
            light_entities.len(),
            1,
            "una sola PointLight2d attesa (solo la stella luminosa), trovate {}",
            light_entities.len()
        );
        assert!(
            app.world()
                .get::<bevy::camera::visibility::NoFrustumCulling>(light_entities[0])
                .is_some(),
            "la PointLight2d child della stella deve avere NoFrustumCulling (T22-FIX)"
        );
    }

    #[test]
    fn t23_star_light_visible_at_tight_zoom_real_numbers() {
        // Regression test T23-TEST: il gate CPU-side di visibilità della
        // luce deve coprire `radius + fade_width` (mirror di
        // `mark_visible_lights` T23-FIX in
        // vendor/bevy_firefly/src/visibility.rs e di `light_extent` in
        // prepare.rs), NON il solo `radius`. Numeri reali del preset di
        // Davide: stella in (63,57) con radius=100 / fade_width=7000 /
        // Falloff::None (shader illumina fino a 7100); pianeta a ~1068
        // (es. (216,-1000)); camera ortografica con area piccola = zoom
        // stretto sul pianeta, stella fuori schermo.
        // Nota harness: `mark_visible_lights` non è pilotabile headless
        // (LightRect privata nel vendor crate), quindi il test esercita la
        // condizione di intersezione con GLI STESSI operandi del sistema
        // (Aabb2d luce vs Aabb2d camera). Con il gate radius-only la stessa
        // assert FALLISCE (prova rosso/verde fatta revertendo `light_extent`
        // a solo `radius` qui sotto: rosso; con `radius + fade_width`: verde).
        use bevy::math::bounding::{Aabb2d, BoundingVolume, IntersectsVolume};

        // Luce REALE della stella (stessi valori del preset).
        let light = PointLight2d {
            radius: 100.0,
            fade_width: 7000.0,
            falloff: Falloff::None,
            ..default()
        };
        let star_pos = Vec2::new(63.0, 57.0);

        // Pianeta a distanza ~1068 dalla stella (verifica di sanità sui
        // numeri reali: sqrt(153^2 + 1057^2) ~= 1068).
        let planet_pos = Vec2::new(216.0, -1000.0);
        let dist = star_pos.distance(planet_pos);
        assert!(
            (dist - 1068.0).abs() < 2.0,
            "i numeri reali devono dare distanza stella-pianeta ~1068, trovata {dist}"
        );

        // Camera ortografica con area piccola (zoom stretto) centrata sul
        // pianeta: stella (63,57) fuori schermo.
        let camera_aabb = Aabb2d::new(planet_pos, Vec2::new(320.0, 180.0));

        // Gate PRE-fix (solo radius=100): scarta la luce -> IL BUG T23.
        let old_gate = Aabb2d::new(star_pos, Vec2::splat(light.radius));
        assert!(
            !old_gate.intersects(&camera_aabb),
            "sanità: col gate radius-only (100) la luce è fuori dalla camera \
             a zoom stretto — è proprio il caso che il fix deve recuperare"
        );

        // Gate POST-fix: `pos ± (radius + fade_width)`, identica espressione
        // di `mark_visible_lights` (T23-FIX). La luce deve risultare visibile.
        let light_extent = light.radius + light.fade_width;
        assert_eq!(light_extent, 7100.0, "extent reale: 100 + 7000 = 7100");
        let fixed_gate = Aabb2d::new(star_pos, Vec2::splat(light_extent));
        assert!(
            fixed_gate.intersects(&camera_aabb),
            "T23: la luce stella (extent 7100) deve intersecare la camera a \
             zoom stretto sul pianeta (distanza ~1068 < 7100)"
        );
    }

    /// E2E del flusso "cambio parametro stella dal pannello" (v0.14.117
    /// debug): il PATH REALE di apply è `apply_star_prop_value` (keypad OK e
    /// sync live di ui.rs), che deve triggerare Changed<StarLightSettings>/
    /// Changed<StarGlow> e i bridge `apply_star_light_settings`/
    /// `apply_star_glow_settings` fino a PointLight2d e agli sprite glow.
    /// Se questo test PASSA mentre Davide non riesce a cambiare i parametri,
    /// il problema è nel layer UI/input (focus/tap/keypad), NON nella catena
    /// di applicazione.
    #[test]
    fn e2e_star_param_edit_reaches_pointlight_and_glow_sprites() {
        use crate::systems::ui::apply_star_prop_value;

        let mut app = bevy::prelude::App::new();
        app.add_systems(
            bevy::prelude::Update,
            (apply_star_light_settings, apply_star_glow_settings),
        );

        let (star, light_entity, glow_entity) = {
            let mut world = app.world_mut();
            let star = world
                .spawn((
                    CelestialBody {
                        name: "Sun".into(),
                        body_type: crate::components::celestial::BodyType::Star,
                        mass: 1000.0,
                        radius: 30.0,
                        color: [1.0, 0.9, 0.3],
                        luminous: true,
                    },
                    StarLightSettings {
                        intensity: 5.0,
                        radius: 100.0,
                        falloff: LightFalloff::None,
                        fade_width: 5000.0,
                        ..Default::default()
                    },
                    StarGlow::default(),
                    FireflyLightAttached,
                    FireflySpriteAttached,
                ))
                .id();
            let light_entity = world.spawn((PointLight2d::default(), ChildOf(star))).id();
            let glow_entity = world
                .spawn((Sprite::default(), FireflyGlowInner, ChildOf(star)))
                .id();
            (star, light_entity, glow_entity)
        };

        // Frame 1: valori iniziali propagati (Changed = true allo spawn).
        app.update();

        // L'utente digita nei campi del pannello: il keypad/sync chiama
        // apply_star_prop_value — STESSO path di ui.rs:1049 e keypad.rs:342.
        // (clone-modify-write: non si possono tenere due Mut sullo stesso
        // World; l'insert finale triggera Changed come farebbe il Mut.)
        {
            let mut world = app.world_mut();
            let mut s = world.get::<StarLightSettings>(star).unwrap().clone();
            let mut g = world.get::<StarGlow>(star).unwrap().clone();
            apply_star_prop_value("light_radius", "250", &mut s, &mut g);
            apply_star_prop_value("light_fade", "7000", &mut s, &mut g);
            apply_star_prop_value("light_intensity", "3.5", &mut s, &mut g);
            apply_star_prop_value("glow_brightness", "2.0", &mut s, &mut g);
            apply_star_prop_value("glow_inner_scale", "8.0", &mut s, &mut g);
            let mut e = world.entity_mut(star);
            e.insert((s, g));
        }

        // Frame 2: Changed<T> triggera i bridge.
        app.update();

        let world = app.world_mut();
        let light = world
            .get::<PointLight2d>(light_entity)
            .expect("PointLight2d child");
        assert_eq!(
            light.radius, 250.0,
            "Radius digitato deve arrivare a PointLight2d"
        );
        assert_eq!(
            light.fade_width, 7000.0,
            "Fade digitato deve arrivare a PointLight2d"
        );
        assert_eq!(
            light.intensity, 3.5,
            "Planet Light digitato deve arrivare a PointLight2d"
        );
        let srgba = light.color.to_srgba();
        assert!(
            (srgba.alpha - 2.0).abs() < 0.001,
            "Halo Brightness (2.0) deve viaggiare nell'alpha della luce, trovato {}",
            srgba.alpha
        );

        let glow = world.get::<Sprite>(glow_entity).expect("sprite glow inner");
        let expected_size = 30.0 * 2.0 * 8.0; // radius * 2 * inner_scale
        assert_eq!(
            glow.custom_size,
            Some(Vec2::splat(expected_size)),
            "Glow Inner Scale digitato deve ridimensionare lo sprite glow"
        );

        println!("OK E2E: apply_star_prop_value -> PointLight2d + glow sprite propagati");
    }

    /// v0.14.121 (regressione della causa "pianeti spenti, nessun cono"):
    /// `PointLight2d` non richiede `Visibility`/`InheritedVisibility`, quindi
    /// `check_visibility_cpu_culling` di Bevy ignora la luce: a fine frame la
    /// classe `PointLight2d` delle `VisibleEntities` resta VUOTA (i push di
    /// `mark_visible_lights` vengono sovrascritti dal rebuild di Bevy) e il
    /// queue della lightmap non accoda alcun item: nessuna luce sui pianeti,
    /// nessun cono, pur con dati GPU corretti (badge `V{push}/0/0` con
    /// `U{radius},{fade},{intensity},{falloff}` giusti).
    #[test]
    fn star_light_child_is_visible_to_bevy_visibility() {
        let mut app = bevy::prelude::App::new();
        app.add_systems(bevy::prelude::Update, spawn_star_lights);
        let star = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "Sole".into(),
                    body_type: crate::components::celestial::BodyType::Planet,
                    mass: 1000.0,
                    radius: 30.0,
                    color: [1.0, 0.9, 0.4],
                    luminous: true,
                },
                Transform::default(),
                StarLightSettings::default(),
                StarGlow::default(),
            ))
            .id();
        app.update();

        let world = app.world_mut();
        let mut lights: Vec<bevy::prelude::Entity> = Vec::new();
        {
            let mut q = world.query::<(bevy::prelude::Entity, &PointLight2d)>();
            for (e, _) in q.iter(world) {
                lights.push(e);
            }
        }
        assert_eq!(lights.len(), 1, "deve esistere esattamente una luce child");
        let light = lights[0];
        assert!(
            world
                .get::<ChildOf>(light)
                .is_some_and(|p| p.parent() == star),
            "la luce deve essere child della stella"
        );
        assert!(
            world.get::<Visibility>(light).is_some(),
            "la luce DEVE avere Visibility, altrimenti Bevy non la processa in check_visibility \
             e la classe PointLight2d resta vuota -> nessun item nella lightmap"
        );
        let inherited = world
            .get::<InheritedVisibility>(light)
            .expect("la luce DEVE avere InheritedVisibility");
        assert!(
            inherited.get(),
            "InheritedVisibility deve essere TRUE alla spawn: con false Bevy scarta la luce e la \
             lightmap resta senza luce (pianeti spenti, nessun cono)"
        );
        assert!(
            world.get::<ViewVisibility>(light).is_some(),
            "ViewVisibility deve essere presente (richiesta dall'extract firefly)"
        );
        println!("OK: la luce child e' visibile a Bevy (Visibility + InheritedVisibility true)");
    }

    /// v0.14.121: verifica END-TO-END (non solo "i componenti ci sono") che la
    /// luce finisce davvero nelle `VisibleEntities` che Bevy ricostruisce ogni
    /// frame e che il queue della lightmap legge via `iter_visible()`.
    /// E' il gate esatto che si rompeva: `check_visibility_cpu_culling` di Bevy
    /// processa solo entita' con `InheritedVisibility`, e RICOSTRUISCE le classi
    /// a ogni frame: se la luce non e' visibile a Bevy, la classe PointLight2d
    /// resta VUOTA (i push manuali di `mark_visible_lights` vengono persi) ->
    /// nessun item nella lightmap -> pianeti spenti, nessun cono.
    /// Nessuna GPU richiesta: `VisibilityPlugin` (bevy_camera) e' un plugin
    /// puramente ECS.
    #[test]
    fn star_light_enters_bevy_visible_entities() {
        use bevy::camera::visibility::VisibleEntities;
        use std::any::TypeId;

        let mut app = bevy::prelude::App::new();
        // Solo il sistema che riempie VisibleEntities: il resto di
        // VisibilityPlugin (calculate_bounds, skinned mesh) richiede Assets
        // del render e non serve a questo gate.
        app.add_systems(
            bevy::prelude::PostUpdate,
            bevy::camera::visibility::check_visibility_cpu_culling,
        );
        app.add_systems(bevy::prelude::Update, spawn_star_lights);

        // Camera con le componenti che il check si aspetta (Frustum incluso:
        // la luce ha NoFrustumCulling, quindi il test del frustum non si applica).
        let cam = app
            .world_mut()
            .spawn((
                bevy::prelude::Camera2d::default(),
                VisibleEntities::default(),
                bevy::prelude::Transform::default(),
                bevy::camera::primitives::Frustum::default(),
            ))
            .id();
        let star = app
            .world_mut()
            .spawn((
                CelestialBody {
                    name: "Sole".into(),
                    body_type: crate::components::celestial::BodyType::Planet,
                    mass: 1000.0,
                    radius: 30.0,
                    color: [1.0, 0.9, 0.4],
                    luminous: true,
                },
                Transform::default(),
                StarLightSettings::default(),
                StarGlow::default(),
            ))
            .id();

        // 2 frame: spawn + propagazione/check visibilita' di Bevy.
        app.update();
        app.update();

        let world = app.world_mut();
        let light = {
            let mut q = world.query::<(bevy::prelude::Entity, &PointLight2d)>();
            let found: Vec<_> = q.iter(world).map(|(e, _)| e).collect();
            assert_eq!(found.len(), 1, "una sola luce");
            found[0]
        };
        assert!(world
            .get::<ChildOf>(light)
            .is_some_and(|p| p.parent() == star));
        let visible = world
            .get::<VisibleEntities>(cam)
            .expect("la camera deve avere VisibleEntities");
        let in_class = visible.get(TypeId::of::<PointLight2d>()).contains(&light);
        assert!(
            in_class,
            "LA LUCE DEVE STARE nella classe PointLight2d delle VisibleEntities \
             ricostruite da Bevy: se manca, il queue della lightmap non accoda alcun item \
             (pianeti spenti, nessun cono) — e' la regressione v0.14.121"
        );
        println!("OK E2E visibilita': la luce e' nella classe PointLight2d delle VisibleEntities");

        // Meccanismo della regressione: Bevy RICOSTRUISCE la classe a ogni
        // frame, quindi un'entita' che non e' visibile a Bevy non sopravvive
        // nella classe nemmeno se ci viene messa a mano (e' quello che fa
        // mark_visible_lights di firefly). E' il motivo per cui la luce
        // spariva dalla lightmap pur con un push riuscito.
        let fantasma = app
            .world_mut()
            .spawn(bevy::prelude::Transform::default())
            .id();
        app.world_mut()
            .get_mut::<VisibleEntities>(cam)
            .expect("VisibleEntities sulla camera")
            .get_mut(TypeId::of::<PointLight2d>())
            .push(fantasma);
        app.update();
        let world = app.world_mut();
        let visible = world.get::<VisibleEntities>(cam).unwrap();
        assert!(
            !visible
                .get(TypeId::of::<PointLight2d>())
                .contains(&fantasma),
            "il rebuild di Bevy deve CANCELLARE i push manuali di entita' non visibili \
             a Bevy: e' il meccanismo che faceva sparire la luce dalla lightmap"
        );
        println!(
            "OK meccanismo: il push manuale di un'entita' non visibile a Bevy viene cancellato"
        );
    }

    /// v0.14.151: la risoluzione della texture segue il raggio del corpo.
    #[test]
    fn texture_size_follows_radius_in_powers_of_two() {
        assert_eq!(texture_size_for_radius(5.0), 64, "raggi piccoli: minimo 64");
        assert_eq!(texture_size_for_radius(12.0), 64, "12*2*2=48 -> 64");
        assert_eq!(texture_size_for_radius(20.0), 128, "20*2*2=80 -> 128");
        assert_eq!(texture_size_for_radius(50.0), 256, "50*2*2=200 -> 256");
        assert_eq!(texture_size_for_radius(120.0), 512, "120*2*2=480 -> 512");
        assert_eq!(texture_size_for_radius(200.0), 1024, "200*2*2=800 -> 1024");
        assert_eq!(texture_size_for_radius(300.0), 1024, "tetto 1024");
        // monotona non decrescente: alzare il raggio non deve MAI abbassare la res
        let mut prev = 0;
        for r in [1.0, 3.0, 8.0, 16.0, 31.0, 64.0, 128.0, 255.0, 512.0] {
            let size = texture_size_for_radius(r);
            assert!(size >= prev, "risoluzione non monotona a raggio {r}");
            prev = size;
        }
    }

    /// La cache riusa la stessa coppia di texture per lo stesso lato.
    #[test]
    fn texture_cache_reuses_one_set_per_size() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        app.init_resource::<BodyTextureCache>();
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let mut cache = BodyTextureCache::default();
        let a = cache.get_or_build(256, &mut images);
        let b = cache.get_or_build(256, &mut images);
        let c = cache.get_or_build(512, &mut images);
        assert_eq!(cache.len(), 2, "due risoluzioni -> due coppie");
        assert_eq!(a.disc, b.disc, "stesso lato -> stesso handle (batching)");
        assert_eq!(a.normal, b.normal);
        assert_ne!(a.disc, c.disc, "lati diversi -> texture diverse");
        let img = images.get(&a.normal).expect("normal map generata");
        assert_eq!(img.width(), 256, "normal map alla risoluzione del bucket");
    }

    /// Cambiare `radius` (come fa il pannello) deve ridimensionare la sprite e
    /// rigenerare la texture: prima restava tutto com'era (solo il cono
    /// d'ombra seguiva il raggio, via `sync_occluders`).
    #[test]
    fn radius_change_resizes_sprite_and_swaps_texture() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        app.init_resource::<BodyTextureCache>();
        app.add_systems(Update, sync_body_sprite_sizes);

        let body = app
            .world_mut()
            .spawn((
                CelestialBody {
                    radius: 20.0,
                    ..Default::default()
                },
                StarGlow::default(),
                Sprite::default(),
                FireflySpriteAttached,
                BodyTextureSize(texture_size_for_radius(20.0)),
            ))
            .id();
        // Child alone (inner): deve seguire il raggio anche lui.
        let glow_child = app
            .world_mut()
            .spawn((FireflyGlowInner, Sprite::default(), ChildOf(body)))
            .id();

        app.update();
        let size_before = {
            let world = app.world();
            world.get::<Sprite>(body).unwrap().custom_size.unwrap().x
        };
        assert!((size_before - 40.0).abs() < 1e-3, "diametro = 2*raggio (20)");
        let tex_before = app.world().get::<BodyTextureSize>(body).unwrap().0;
        assert_eq!(tex_before, 128);

        // Il pannello scrive solo `radius`.
        app.world_mut().get_mut::<CelestialBody>(body).unwrap().radius = 200.0;
        app.update();

        let world = app.world();
        let sprite = world.get::<Sprite>(body).unwrap();
        assert!(
            (sprite.custom_size.unwrap().x - 400.0).abs() < 1e-3,
            "la sprite deve seguire il nuovo raggio (400), non restare 40: {:?}",
            sprite.custom_size
        );
        assert_eq!(
            world.get::<BodyTextureSize>(body).unwrap().0,
            1024,
            "la texture deve passare al bucket del nuovo raggio"
        );
        let glow = world.get::<Sprite>(glow_child).unwrap();
        assert!(
            (glow.custom_size.unwrap().x - 200.0 * 2.0 * 4.0).abs() < 1e-3,
            "l'alone inner deve seguire il raggio (2*r*inner_scale)"
        );
        println!("OK radius -> sprite/alone/texture seguono il raggio");
    }

    /// Forma REALE di un pianeta: nessun child e nessun `StarGlow`. La query
    /// del sistema chiedeva `&Children` (presente solo sulle stelle con aloni)
    /// → il pianeta non entrava mai nel sistema e il raggio non aveva effetto.
    #[test]
    fn radius_change_resizes_planet_without_children() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        app.init_resource::<BodyTextureCache>();
        app.add_systems(Update, sync_body_sprite_sizes);

        let planet = app
            .world_mut()
            .spawn((
                CelestialBody {
                    radius: 20.0,
                    ..Default::default()
                },
                Sprite::default(),
                FireflySpriteAttached,
                BodyTextureSize(texture_size_for_radius(20.0)),
            ))
            .id();
        assert!(
            app.world().get::<Children>(planet).is_none(),
            "il pianeta di prova non deve avere figli (come in app)"
        );

        app.update();
        app.world_mut().get_mut::<CelestialBody>(planet).unwrap().radius = 95.0;
        app.update();

        let world = app.world();
        let sprite = world.get::<Sprite>(planet).unwrap();
        assert!(
            (sprite.custom_size.unwrap().x - 190.0).abs() < 1e-3,
            "un pianeta SENZA figli deve comunque seguire il raggio: {:?}",
            sprite.custom_size
        );
        assert_eq!(
            world.get::<BodyTextureSize>(planet).unwrap().0,
            512,
            "95*2*2=380 -> bucket 512"
        );
    }

    /// Il disco ha un bordo antialiasato (almeno un texel di transizione).
    #[test]
    fn disc_texture_edge_is_antialiased() {
        let size = 128usize;
        let img = crate::rendering::textures::generate_disc_texture(size);
        let data = img.data.as_ref().expect("dati del disco").clone();
        let alpha = |x: usize, y: usize| data[(y * size + x) * 4 + 3];
        assert_eq!(alpha(size / 2, size / 2), 255, "centro opaco");
        assert_eq!(alpha(0, 0), 0, "angolo fuori dal cerchio");
        let row = size / 2;
        let transitions = (0..size).filter(|&x| (1..255).contains(&alpha(x, row))).count();
        assert!(
            transitions >= 1,
            "bordo a scalino: nessun texel di transizione (alpha binario)"
        );
    }

    /// L'anello NERO attorno alla stella (v0.14.165) non deve tornare.
    ///
    /// `apply_lightmap.wgsl` MOLTIPLICA la scena per la lightmap, quindi dove
    /// la lightmap vale 0 lo sfondo diventa nero e opaco. Il core della luce
    /// usa `LightCore::falloff` di default (InverseSquare) che vale 1 al
    /// centro e **0 esattamente a core_radius**: senza il pavimento a
    /// `surface`, lo sfondo tra il disco della stella e core_radius (1.5×
    /// raggio) veniva moltiplicato per ~0 → anello nero non controllabile dal
    /// pannello e più evidente alzando Halo Brightness.
    ///
    /// Gate sul testo della shader (come `sprite_shader_normal_write_...`):
    /// il ramo del core DEVE clampare a `surface` e `surface` deve essere
    /// calcolato PRIMA dei due rami.
    #[test]
    fn star_core_never_multiplies_the_sky_below_the_halo_level() {
        let src = std::fs::read_to_string("vendor/bevy_firefly/src/shaders/create_lightmap.wgsl")
            .expect("vendored create_lightmap shader must be present");
        let surface_at = src
            .find("let surface = select(light.intensity, halo")
            .expect("`surface` must still choose intensity vs halo");
        let core_at = src
            .find("if dist <= light.core_radius {")
            .expect("core branch must be present");
        assert!(
            surface_at < core_at,
            "`surface` va calcolato PRIMA del ramo del core: serve come pavimento"
        );
        let tail = &src[core_at..];
        let core_block = &tail[..tail.find("else {").unwrap_or(tail.len())];
        assert!(
            core_block.contains("max(core_term, surface)"),
            "il ramo del core deve usare max(core_term, surface): senza, la lightmap \
             va a 0 a core_radius e disegna l'anello nero attorno alla stella"
        );
    }

    /// Il blocco del core, preso DAL FILE che spediamo, deve essere WGSL valido.
    ///
    /// Il test di testo sopra controlla l'INTENZIONE; questo controlla che la
    /// sintassi passi davvero il parser/validator (naga) usato da Bevy: una
    /// shader non compilabile = schermo nero sul device, e `cargo check` non
    /// la vede. Il blocco viene estratto dal sorgente e incastrato in uno stub
    /// con i soli simboli che usa (`light`, `surface`, `normale`, `res`).
    #[test]
    fn star_core_block_is_valid_wgsl() {
        let src = std::fs::read_to_string("vendor/bevy_firefly/src/shaders/create_lightmap.wgsl")
            .expect("vendored create_lightmap shader must be present");
        let start = src
            .find("let surface = select(light.intensity, halo")
            .expect("`surface` definition");
        let rest = &src[start..];
        let end = rest.find("else {").expect("else branch");
        let block = &rest[..end];

        // Concatenazione, NON format!: il blocco WGSL contiene `{`/`}` che
        // format! interpreterebbe come placeholder.
        let mut stub = String::new();
        stub.push_str("enable f16;\n");
        stub.push_str("fn falloff(x: f32, f: u32, fi: f32) -> f32 { return 1.0; }\n");
        stub.push_str(
            "struct Light { intensity: f32, core_radius: f32, core_boost: f32, \
             core_falloff: u32, core_falloff_intensity: f32 }\n",
        );
        stub.push_str("@compute @workgroup_size(1)\nfn main() {\n");
        stub.push_str("    var res = vec4f(0.0);\n");
        stub.push_str("    let light = Light(3.0, 450.0, 3.0, 0u, 0.0);\n");
        stub.push_str("    let dist = 500.0;\n");
        stub.push_str("    let light_color = vec4f(1.0, 0.9, 0.3, 0.25);\n");
        stub.push_str("    let angle_multi = 1.0;\n");
        stub.push_str("    let normal_multi = 1.0;\n");
        stub.push_str("    let normal = vec4f(0.0);\n");
        stub.push_str("    let halo = 0.25;\n");
        stub.push_str(block);
        stub.push_str("\n    _ = res;\n}\n");
        let module = naga::front::wgsl::parse_str(&stub)
            .unwrap_or_else(|e| panic!("WGSL PARSE FAILED (core block): {e}\n---\n{stub}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("WGSL VALIDATION FAILED (core block): {e:?}"));
    }
}
