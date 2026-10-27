//! Diagnostica: bordi dei coni d'ombra (umbra + penombra) come linee Gizmo.
//!
//! Perché serve: la shader calcola l'ombra per-pixel e a schermo si vede solo
//! il risultato (un cono più largo, un bordo netto, un pianeta non oscurato).
//! Queste linee ridisegnano gli STESSI bordi con le STESSE formule di
//! `desmos_round_check` (`vendor/bevy_firefly/src/shaders/create_lightmap.wgsl`):
//!
//!   bordo cono (penombra): w_cone(t)  = r + t·r/d
//!   bordo umbra:           w_umbra(t) = max(r − t·(r_sun − r)/d, 0)
//!
//! con `t` misurata dal CENTRO del corpo lungo l'asse stella→corpo e
//! `r_sun = raggio stella × core_radius_factor` (il disco solare). Se il cono
//! che si vede a schermo non tocca queste linee, il problema sta nei DATI
//! (raggi, distanze, core_radius_factor) o nei gate della shader (stencil,
//! z-sorting, binning) — non nella formula.
//!
//! Le linee descrivono la **v2**: con `ShadowAlgo::v2 = false` la shader usa la
//! v1 e le linee NON corrispondono (voluto: il confronto ha senso a v2 attiva).
//!
//! Colori: umbra rossa, bordo del cono gialla. Sul punto di chiusura
//! dell'umbra (`t = r·d/(r_sun − r)`, il `P_intersect` del calcolatore) c'è
//! una croce: se la croce sta dove il nucleo buio si spegne, geometria e
//! shader concordano.

use bevy::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::components::lighting::StarLightSettings;

/// Lunghezza diagnostica del cono disegnato (unità mondo). La shader non ha
/// una fine (il cono arriva al bordo dello schermo): qui serve un riferimento
/// fisso per confrontare le pendenze.
const GIZMO_CONE_LENGTH: f32 = 3000.0;
/// Corpi più piccoli non proiettano ombra (mirror del gate della shader, che
/// richiede `radius > 0`).
const GIZMO_MIN_RADIUS: f32 = 2.0;

fn umbra_color() -> Color {
    Color::srgb(1.0, 0.25, 0.2)
}

fn cone_color() -> Color {
    Color::srgb(1.0, 0.85, 0.25)
}

/// Toggle del pannello Impostazioni (sezione OMBRE).
#[derive(Resource, Default)]
pub struct ShadowGizmos {
    pub show: bool,
}

pub struct ShadowGizmoPlugin;

impl Plugin for ShadowGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShadowGizmos>()
            .add_systems(Update, draw_shadow_cone_borders);
    }
}

/// Semilarghezza dei due bordi a distanza `t` dal centro del corpo: è la
/// trascrizione 1:1 delle due righe della shader. Funzione pura, così un test
/// può bloccarla; il sistema che la usa ha UNA sola query (nessun B0001).
pub(crate) fn cone_half_widths(r_body: f32, r_sun: f32, d: f32, t: f32) -> (f32, f32) {
    if d <= 0.0 {
        return (0.0, r_body);
    }
    let w_umbra = (r_body - t * (r_sun - r_body) / d).max(0.0);
    let w_cone = r_body + t * r_body / d;
    (w_umbra, w_cone)
}

/// Distanza (dal centro del corpo) in cui l'umbra si chiude, se il disco
/// solare è più grande del corpo. `None` = umbra divergente (sole più piccolo).
pub(crate) fn umbra_closing_distance(r_body: f32, r_sun: f32, d: f32) -> Option<f32> {
    if r_sun > r_body && d > 0.0 {
        Some(r_body * d / (r_sun - r_body))
    } else {
        None
    }
}

fn draw_shadow_cone_borders(
    mut gizmos: Gizmos,
    show: Res<ShadowGizmos>,
    bodies: Query<(&CelestialBody, &GlobalTransform, Option<&StarLightSettings>)>,
) {
    if !show.show {
        return;
    }
    // UNA query per stelle e pianeti: due query che leggono `&CelestialBody`
    // sarebbero lecite (sola lettura), ma una sola mette al riparo dalla
    // classe B0001 se in futuro servisse una scrittura.
    let mut stars: Vec<(Vec2, f32)> = Vec::new();
    for (body, gt, settings) in bodies.iter() {
        if body.luminous {
            let factor = settings.map(|s| s.core_radius_factor).unwrap_or(1.0);
            stars.push((gt.translation().truncate(), body.radius * factor));
        }
    }
    if stars.is_empty() {
        return;
    }

    for (body, gt, _) in bodies.iter() {
        if body.luminous || body.radius < GIZMO_MIN_RADIUS {
            continue;
        }
        let center = gt.translation().truncate();
        for &(star_pos, r_sun) in stars.iter() {
            let to_body = center - star_pos;
            let d = to_body.length();
            if d <= body.radius {
                continue; // stella dentro il corpo: nessun cono, come la shader
            }
            let axis = to_body / d;
            let perp = Vec2::new(-axis.y, axis.x);
            let base_l = center + perp * body.radius;
            let base_r = center - perp * body.radius;

            // --- bordo esterno (penombra) ---
            let (_, w_cone_end) = cone_half_widths(body.radius, r_sun, d, GIZMO_CONE_LENGTH);
            let cone_end_l = center + axis * GIZMO_CONE_LENGTH + perp * w_cone_end;
            let cone_end_r = center + axis * GIZMO_CONE_LENGTH - perp * w_cone_end;
            gizmos.line_2d(base_l, cone_end_l, cone_color());
            gizmos.line_2d(base_r, cone_end_r, cone_color());
            gizmos.line_2d(cone_end_l, cone_end_r, cone_color());

            // --- umbra (nucleo buio) ---
            let t_umbra = umbra_closing_distance(body.radius, r_sun, d)
                .filter(|c| *c < GIZMO_CONE_LENGTH)
                .unwrap_or(GIZMO_CONE_LENGTH);
            let (w_umbra_end, _) = cone_half_widths(body.radius, r_sun, d, t_umbra);
            let umbra_end_l = center + axis * t_umbra + perp * w_umbra_end;
            let umbra_end_r = center + axis * t_umbra - perp * w_umbra_end;
            gizmos.line_2d(base_l, umbra_end_l, umbra_color());
            gizmos.line_2d(base_r, umbra_end_r, umbra_color());
            gizmos.line_2d(umbra_end_l, umbra_end_r, umbra_color());

            // Croce sul punto di chiusura: lì il nucleo buio finisce.
            if t_umbra < GIZMO_CONE_LENGTH {
                let p = center + axis * t_umbra;
                let half = body.radius * 0.35;
                let dx = perp * half;
                let dy = axis * half;
                gizmos.line_2d(p - dx - dy, p + dx + dy, umbra_color());
                gizmos.line_2d(p - dx + dy, p + dx - dy, umbra_color());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A t = 0 i due bordi coincidono col raggio del corpo (è il punto di
    /// partenza di entrambe le formule nella shader).
    #[test]
    fn both_borders_start_at_the_body_radius() {
        let (w_umbra, w_cone) = cone_half_widths(120.0, 450.0, 3000.0, 0.0);
        assert!((w_umbra - 120.0).abs() < 1e-3, "umbra {w_umbra}");
        assert!((w_cone - 120.0).abs() < 1e-3, "cono {w_cone}");
    }

    /// Il bordo del cono cresce con pendenza r/d (silhouette dal centro del
    /// sole). Riferimento numerico: Beta dietro Alpha del preset.
    #[test]
    fn cone_border_uses_the_silhouette_slope() {
        let (_, w_cone) = cone_half_widths(120.0, 450.0, 3000.0, 1000.0);
        assert!((w_cone - 160.0).abs() < 1e-3, "w_cone {w_cone} (atteso 160)");
    }

    /// L'umbra si chiude esattamente dove la formula del calcolatore mette
    /// `P_intersect`: t = r·d/(r_sun − r).
    #[test]
    fn umbra_closes_at_the_desmos_intersection_point() {
        let t = umbra_closing_distance(120.0, 450.0, 3000.0).expect("umbra convergente");
        assert!((t - 1090.909).abs() < 0.01, "t {t} (atteso 1090.9)");
        let (w_umbra, _) = cone_half_widths(120.0, 450.0, 3000.0, t);
        assert!(w_umbra < 1e-3, "umbra non chiusa: {w_umbra}");
    }

    /// Con un sole più piccolo del corpo l'umbra NON si chiude: il cono resta
    /// pieno e il caso non deve produrre una croce a distanza negativa.
    #[test]
    fn small_sun_keeps_the_umbra_open() {
        assert!(umbra_closing_distance(200.0, 60.0, 3000.0).is_none());
        let (w_umbra, w_cone) = cone_half_widths(200.0, 60.0, 3000.0, 1000.0);
        assert!(w_umbra > 200.0, "umbra deve divergere, got {w_umbra}");
        assert!(w_cone > w_umbra, "il cono deve contenere l'umbra");
    }
}
