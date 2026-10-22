//! Log di movimento per-frame su file (diagnostica "il pianeta scatta").
//!
//! Perché non il badge: i valori cambiano a 60-120 Hz e leggerli a occhio
//! significa prenderli nel frame sbagliato. Qui ogni frame produce una riga
//! CSV che viene svuotata da JS (`drain_motion_log`) e spedita con
//! `POST /log?s=<sessione>`; i server (`serve_gz.py`, `serve-https.py`)
//! l'appendono a `~/gravity-logs/motion-<sessione>.csv`. Il WASM non ha
//! thread né filesystem: il "thread che scrive su file" è il buffer qui +
//! il server che scrive, così il file esiste davvero e non serve copiarlo
//! a mano dal telefono.
//!
//! Colonne (devono combaciare con l'HEADER di `logsink.py`):
//! `t_ms,dt_ms,zoom,cam_x,cam_y,followed,body_idx,body_x,body_y,phys_x,phys_y,gap,fract,ease,speed`
//!
//! - `body_*` è il `Transform` RESO (dopo l'easing), `phys_*` la `Position`
//!   fisica: `gap` = loro distanza. Se `gap` è sempre ~0 mentre la sim gira,
//!   l'interpolazione non sta producendo nulla (qualcuno azzera lo stato).
//! - `ease`: 2 = stato di easing attivo, 1 = stato azzerato (`None`), 0 =
//!   componente assente. `fract` = `overstep_fraction` di `Time<Fixed>`.
//! - Lo scatto si misura sullo scostamento a schermo: `(body - cam) / zoom`.

use bevy::prelude::*;
use std::fmt::Write as _;
use std::sync::Mutex;

use crate::components::celestial::CelestialBody;
use crate::systems::camera::MainCamera;
use crate::systems::camera_follow::CameraFollow;
use crate::systems::timeline::SimulationState;

/// Buffer condiviso con l'export WASM. `Mutex<&'static str>`-free: `String`
/// vuota di partenza e `drain(..)` a ogni svuotamento.
pub static MOTION_LOG: Mutex<String> = Mutex::new(String::new());

/// Oltre questa soglia si buttano le righe più vecchie (metà buffer): con
/// ~120 byte/riga e 120 fps sono ~40 s di registrazione.
const MAX_BYTES: usize = 512 * 1024;

pub struct MotionLogPlugin;

impl Plugin for MotionLogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, sample_motion);
    }
}

/// Svuota il buffer e lo restituisce (chiamato da JS ogni ~2 s).
pub fn drain() -> String {
    match MOTION_LOG.lock() {
        Ok(mut buf) => std::mem::take(&mut *buf),
        Err(_) => String::new(),
    }
}

fn sample_motion(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    follow: Res<CameraFollow>,
    sim: Res<SimulationState>,
    camera: Query<(&Transform, &Projection), (With<Camera2d>, With<MainCamera>)>,
    bodies: Query<
        (
            Entity,
            &Transform,
            &CelestialBody,
            Option<&avian2d::prelude::Position>,
            Option<&bevy_transform_interpolation::TranslationEasingState>,
        ),
        Without<MainCamera>,
    >,
) {
    let Ok((cam_tf, projection)) = camera.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let zoom = ortho.scale.max(1e-6);
    let cam = cam_tf.translation.truncate();

    // Corpo osservato: quello agganciato dalla camera; se la camera è libera,
    // il non-luminoso più vicino al centro schermo (è quello che si guarda).
    let followed = follow.0.is_some();
    let mut picked: Option<(Entity, Vec3, Vec2, u8, f32)> = None;
    let mut best_dist = f32::INFINITY;
    for (entity, transform, body, position, easing) in &bodies {
        let is_target = follow.0 == Some(entity);
        let dist = (transform.translation.truncate() - cam).length();
        let take = if followed {
            is_target
        } else {
            !body.luminous && dist < best_dist
        };
        if !take {
            continue;
        }
        best_dist = dist;
        let ease = match easing {
            Some(state) if state.end.is_some() => 2u8,
            Some(_) => 1u8,
            None => 0u8,
        };
        let phys = position.map(|p| p.0).unwrap_or(Vec2::ZERO);
        picked = Some((entity, transform.translation, phys, ease, body.mass));
    }

    let (entity, body_pos, phys, ease) = match picked {
        Some((e, t, p, ease, _mass)) => (e.index().index(), t, p, ease),
        None => (u32::MAX, Vec3::ZERO, Vec2::ZERO, 0),
    };
    let gap = (body_pos.truncate() - phys).length();

    let mut line = String::with_capacity(128);
    let _ = writeln!(
        line,
        "{:.1},{:.3},{:.4},{:.3},{:.3},{},{},{:.3},{:.3},{:.3},{:.3},{:.4},{:.4},{},{}",
        time.elapsed_secs_f64() * 1000.0,
        time.delta_secs_f64() * 1000.0,
        zoom,
        cam.x,
        cam.y,
        if followed { 1 } else { 0 },
        entity,
        body_pos.x,
        body_pos.y,
        phys.x,
        phys.y,
        gap,
        fixed.overstep_fraction(),
        ease,
        sim.speed,
    );

    if let Ok(mut buf) = MOTION_LOG.lock() {
        buf.push_str(&line);
        if buf.len() > MAX_BYTES {
            // Taglia le righe più vecchie, allineando al '\n' successivo.
            let mut cut = buf.len() - MAX_BYTES / 2;
            while cut < buf.len() && !buf.is_char_boundary(cut) {
                cut += 1;
            }
            if let Some(nl) = buf[cut..].find('\n') {
                cut += nl + 1;
            }
            buf.drain(..cut);
        }
    }
}
