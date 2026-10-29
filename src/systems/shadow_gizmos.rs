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

            // --- punti di TANGENZA (formula del calcolatore di riferimento) ---
            // Il cono non parte dal punto più largo del disco (l'equatore,
            // `center ± perp*r`): da lì le due rette sono SECANTI e il
            // "triangolo" che ne esce non è quello dell'umbra. Il poligono del
            // calcolatore parte dal piede della tangente:
            //     o = asin(r/d)                  (semiangolo sotto cui si vede il corpo)
            //     dist = d·cos(o) = √(d² - r²)   (d_listTweak: distanza della tangente)
            //     punto = stella + dist·(dir ruotata di ±o rispetto all'asse)
            // Forte del fatto che la tangente è perpendicolare al raggio, i due
            // punti di tangenza stanno a `r` dall'asse con componente lungo
            // l'asse `d - r²/d`; la forma `d·cos(o)·rot(±o)` dà lo stesso punto
            // ed è quella del riferimento (una sola formula, nessuna deriva).
            let sin_o = (body.radius / d).clamp(-1.0, 1.0);
            let cos_o = (1.0 - sin_o * sin_o).max(0.0).sqrt();
            let tang_dist = d * cos_o; // = sqrt(d² - r²)
            // rotazione di ±o: il punto di tangenza visto DALLA STELLA, in
            // coordinate (lungo l'asse, perpendicolare).
            let along = tang_dist * cos_o; // proiezione sull'asse
            let aside = tang_dist * sin_o; // scostamento laterale
            let tangent_l = star_pos + axis * along + perp * aside;
            let tangent_r = star_pos + axis * along - perp * aside;
            // Base "visiva" del disco (serve per il riempimento diagnostico
            // del corpo, non per le rette del cono).
            let base_l = center + perp * body.radius;
            let base_r = center - perp * body.radius;

            // Direzioni delle due TANGENTI: la retta tangente nel punto di
            // tangenza scorre lungo `axis·cos(o) ± perp·sin(o)` (perpendicolare
            // al raggio in quel punto). È la stessa direzione del poligono
            // primario del calcolatore, che poi la estende fino a
            // d_RenderEnder: qui fino a GIZMO_CONE_LENGTH.
            let dir_l = axis * cos_o + perp * sin_o;
            let dir_r = axis * cos_o - perp * sin_o;

            // --- bordo esterno (penombra) ---
            let cone_end_l = tangent_l + dir_l * GIZMO_CONE_LENGTH;
            let cone_end_r = tangent_r + dir_r * GIZMO_CONE_LENGTH;
            gizmos.line_2d(tangent_l, cone_end_l, cone_color());
            gizmos.line_2d(tangent_r, cone_end_r, cone_color());
            gizmos.line_2d(cone_end_l, cone_end_r, cone_color());

            // --- umbra (nucleo buio) ---
            // Le rosse seguono il poligono dell'umbra del calcolatore, che NON
            // parte dalla stella ma da P_intersect:
            //   o_umbra      = asin((r_sun - r) / d)
            //   d_umbraTweak = |P - centro| · cos(o_umbra)
            //   estremo vicino = P - d_umbraTweak·rot(o_umbra)
            // Quell'estremo sta ESATTAMENTE sulla superficie del pianeta
            // (verificato: distanza dal centro = r) e a ~r_sun dalla stella.
            //
            // La v0.14.187 usava il punto di tangenza del CORPO (famiglia del
            // cono, pendenza r/d): sbagliato, le rosse hanno pendenza
            // (r_sun-r)/d. E una retta uscente dalla STELLA con quella
            // pendenza non tocca il pianeta (dista 328 da un corpo di 120):
            // per questo va costruita da P_intersect, non dalla stella.
            let k = ((r_sun - body.radius) / d).clamp(-1.0, 1.0);
            let root = (1.0 - k * k).max(0.0).sqrt();
            let t_close_raw = umbra_closing_distance(body.radius, r_sun, d);
            let umbra_start_l;
            let umbra_start_r;
            match t_close_raw {
                Some(tc) => {
                    // P_intersect misurato dal centro del corpo.
                    let p_inter = center + axis * tc;
                    let d_umbra = tc.max(0.0);
                    let d_tweak = d_umbra * root; // cos(o_umbra) = sqrt(1-k²)
                    // Estremo VICINO del poligono (verso il corpo), ruotato di
                    // ±o_umbra: è il punto da cui parte la linea dell'umbra.
                    let back = d_tweak * root; // componente lungo l'asse
                    let aside = d_tweak * k.abs(); // componente laterale
                    umbra_start_l = p_inter - axis * back + perp * aside;
                    umbra_start_r = p_inter - axis * back - perp * aside;
                }
                None => {
                    // Sole più piccolo del corpo: l'umbra diverge e non c'è
                    // P_intersect. Le linee partono dal bordo del disco.
                    umbra_start_l = center + perp * body.radius;
                    umbra_start_r = center - perp * body.radius;
                }
            }

            let t_umbra = umbra_closing_distance(body.radius, r_sun, d)
                .filter(|c| *c < GIZMO_CONE_LENGTH)
                .unwrap_or(GIZMO_CONE_LENGTH);
            let (w_umbra_end, _) = cone_half_widths(body.radius, r_sun, d, t_umbra);
            let umbra_end_l = center + axis * t_umbra + perp * w_umbra_end;
            let umbra_end_r = center + axis * t_umbra - perp * w_umbra_end;
            gizmos.line_2d(umbra_start_l, umbra_end_l, umbra_color());
            gizmos.line_2d(umbra_start_r, umbra_end_r, umbra_color());
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

    /// Le due rette del cono devono essere TANGENTI al corpo, non passare per
    /// l'equatore. La formula del calcolatore (d_listTweak = d·cos(o),
    /// o = asin(r/d)) dà, per ogni punto di partenza:
    ///   - distanza dal centro del corpo == r   (la retta tangente tocca il
    ///     cerchio, quindi il raggio nel punto di tangenza è perpendicolare);
    ///   - distanza dalla stella == √(d² − r²)  (il piede della tangente).
    /// Con il bug v0.14.187 entrambe erano sbagliate: partenza a `center ±
    /// perp*r`, cioè sull'equatore, dove la distanza dalla stella è d e quella
    /// dal centro è r ma la direzione NON è tangente (retta secante).
    #[test]
    fn tangent_points_touch_the_body_and_match_the_desmos_foot() {
        let (r, d) = (120.0_f32, 3000.0_f32);
        let star = Vec2::ZERO;
        let center = Vec2::new(d, 0.0); // corpo sull'asse +X
        let axis = (center - star).normalize();
        let perp = Vec2::new(-axis.y, axis.x);

        let sin_o = (r / d).clamp(-1.0, 1.0);
        let cos_o = (1.0 - sin_o * sin_o).max(0.0).sqrt();
        let tang_dist = d * cos_o;
        let along = tang_dist * cos_o;
        let aside = tang_dist * sin_o;
        let t_l = star + axis * along + perp * aside;
        let t_r = star + axis * along - perp * aside;

        for (name, p) in [("sinistra", t_l), ("destra", t_r)] {
            // 1) distanza dal CENTRO del corpo == r (punto sul cerchio)
            let dist_center = (p - center).length();
            assert!(
                (dist_center - r).abs() < 1e-2,
                "{name}: il punto di partenza deve stare SUL cerchio, \
                 distanza {dist_center} invece di {r}"
            );
            // 2) distanza dalla STELLA == sqrt(d² - r²) (piede della tangente)
            let expected = (d * d - r * r).sqrt();
            let dist_star = (p - star).length();
            assert!(
                (dist_star - expected).abs() < 1e-2,
                "{name}: distanza dalla stella {dist_star}, atteso {expected} \
                 (d·cos(o) del calcolatore)"
            );
            // 3) il raggio nel punto di tangenza è PERPENDICOLARE alla retta:
            //    la retta (dir) deve avere prodotto scalare nullo col raggio.
            let radius_dir = (p - center).normalize();
            let sign = if name == "sinistra" { 1.0 } else { -1.0 };
            let dir = axis * cos_o + perp * (sin_o * sign);
            assert!(
                radius_dir.dot(dir).abs() < 1e-4,
                "{name}: la retta NON è tangente (raggio·dir = {})",
                radius_dir.dot(dir)
            );
        }

        // Controllo del difetto: la partenza sull'EQUATORE non è tangente.
        // All'equatore il raggio è PERPENDICOLARE all'asse, quindi una retta
        // lungo l'asse è perpendicolare al raggio e sembrerebbe tangente: il
        // difetto vero è che il piede è a distanza `d` dalla stella (invece di
        // √(d²−r²)) e la retta da lì tocca il cerchio solo all'equatore, non
        // lungo la direzione di propagazione dell'ombra.
        let equator = center + perp * r;
        let dist_star_equator = (equator - star).length();
        let expected_equator = (d * d + r * r).sqrt();
        let tang_foot = (d * d - r * r).sqrt();
        assert!(
            (dist_star_equator - expected_equator).abs() < 1e-2,
            "l'equatore dista √(d²+r²) = {expected_equator:.1} dalla stella, \
             misurato {dist_star_equator:.1}"
        );
        assert!(
            dist_star_equator > tang_foot + 1.0,
            "il piede della tangente ({tang_foot:.1}) è più VICINO alla stella \
             dell'equatore ({dist_star_equator:.1}): partire dall'equatore \
             sposta la base del cono di oltre 4 unità (d·(1-cos o))"
        );
    }

    /// Le RETTE ROSSE dell'umbra: costruite come il poligono del calcolatore,
    /// a partire da P_intersect. L'estremo vicino deve stare ESATTAMENTE sulla
    /// superficie del pianeta (distanza dal centro = r): è ciò che vede
    /// l'utente ("tangenti anche al pianeta").
    /// Bug v0.14.187: partivano dal piede della tangente del CORPO, famiglia
    /// con pendenza r/d, e le rosse finivano staccate dal disco.
    #[test]
    fn umbra_lines_start_on_the_planet_surface() {
        let (r, d, r_sun) = (120.0_f32, 3000.0_f32, 450.0_f32);
        let star = Vec2::ZERO;
        let center = Vec2::new(d, 0.0);
        let axis = (center - star).normalize();
        let perp = Vec2::new(-axis.y, axis.x);

        let k = ((r_sun - r) / d).clamp(-1.0, 1.0);
        let root = (1.0 - k * k).max(0.0).sqrt();
        let tc = r * d / (r_sun - r); // P_intersect dal centro del corpo
        let p_inter = center + axis * tc;
        let d_tweak = tc * root;
        let back = d_tweak * root;
        let aside = d_tweak * k.abs();
        let s_l = p_inter - axis * back + perp * aside;
        let s_r = p_inter - axis * back - perp * aside;

        for (name, p) in [("sinistra", s_l), ("destra", s_r)] {
            // 1) il punto di partenza sta SULLA SUPERFICIE del pianeta
            let dist_center = (p - center).length();
            assert!(
                (dist_center - r).abs() < 0.5,
                "{name}: la linea rossa parte a {dist_center:.1} dal centro, \
                 deve partire sulla superficie (r = {r})"
            );
            // 2) la linea non deve attraversare il disco: il punto più vicino
            //    della retta al centro è il punto di partenza stesso (o più
            //    lontano), quindi distanza minima >= r - tolleranza.
            let dir = (p_inter - p).normalize();
            let to_center = center - p;
            let along = to_center.dot(dir).max(0.0);
            let closest = p + dir * along;
            let min_dist = (closest - center).length();
            assert!(
                min_dist > r - 1.0,
                "{name}: la linea rossa entra nel disco (distanza minima \
                 {min_dist:.1} < r = {r})"
            );
        }
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
