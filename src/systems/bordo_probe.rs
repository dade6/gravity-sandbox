//! v0.14.211 — SONDA DIAGNOSTICA DEL BORDO (sola lettura, nessun effetto sulla resa).
//!
//! Per ogni pianeta, rispetto alla stella, calcola con la STESSA formula dello
//! shader (`create_lightmap.wgsl`, `normal_mode` TopDownY, `normal_attenuation`
//! 0.2, pavimento `NORMAL_RIM_Z_MIN`) il `normal_multi` del punto del bordo
//! che sta sul lato OPPOSTO alla stella (lato in ombra). Serve a leggere, per
//! quadrante, se il bordo riceve luce dove non dovrebbe.
//!
//! Esposto come JSON su `bordo_probe_state()` e mostrato nel badge con `?debug`.

use bevy::prelude::*;

use crate::components::celestial::{BodyType, CelestialBody};
use crate::rendering::textures::NORMAL_RIM_Z_MIN;

/// Attenuazione normale impostata in `firefly_bridge.rs` (`normal_attenuation: 0.2`).
const NORMAL_ATTENUATION: f32 = 0.2;

/// Colore della stella dal preset (`Sun.color`): è `light.color` sul pianeta,
/// PRIMA del tone mapping (che dipende da `view.color_grading`, non in CPU).
const LUCE_COLORE_STELLA: [f32; 3] = [1.0, 0.9, 0.3];

/// Intensità della luce della stella dal preset (`Sun.light.intensity`).
const LUCE_INTENSITA_STELLA: f32 = 3.0;

/// Quadrante del pianeta rispetto alla stella, in parole (per leggere il log).
fn quadrante(dx: f32, dy: f32) -> &'static str {
    match (dx >= 0.0, dy >= 0.0) {
        (true, true) => "pianeta alto-dx",
        (false, true) => "pianeta alto-sx",
        (true, false) => "pianeta basso-dx",
        (false, false) => "pianeta basso-sx",
    }
}

/// `normal_multi` sul bordo lato ombra, come lo calcola lo shader con
/// `normal_mode = TopDownY`:
/// - normale del bordo = (direzione verso l'esterno, nz = pavimento), normalizzata;
/// - `normal_dir = mix(n, 0, attenuation)`;
/// - `light_dir = normalize(light.x - px, py - light.y, 0)` (componente Y del
///   pianeta, come `stencil.r`).
fn normal_multi_bordo_ombra(dx: f32, dy: f32) -> f32 {
    // Bordo lato OPPOSTO alla stella: il punto del cerchio nella direzione -(stella - pianeta).
    let d = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (-dx / d, -dy / d);
    // Normale del bordo: componente nel piano + pavimento su nz (cfr. textures.rs).
    let (nx, ny, nz) = (ux, uy, NORMAL_RIM_Z_MIN);
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    let (nx, ny, nz) = (nx / len, ny / len, nz / len);
    let k = 1.0 - NORMAL_ATTENUATION;
    let nd = (nx * k, ny * k, nz * k);
    // Direzione luce dal pianeta verso la stella, nel piano (z = 0).
    let (lx, ly) = (dx, dy); // pianeta -> stella = -(pianeta - stella)
    let (lx, ly) = (-lx, -ly);
    let ll = (lx * lx + ly * ly).sqrt().max(1e-6);
    let (lx, ly) = (lx / ll, ly / ll);
    (nd.0 * lx + nd.1 * ly).max(0.0)
}

/// Riempie `BORDO_PROBE` ogni frame. Sola lettura del mondo.
pub fn bordo_probe_snapshot(
    bodies: Query<(&CelestialBody, &Transform, Option<&Name>)>,
) {
    let mut stella: Option<Vec2> = None;
    let mut pianeti: Vec<(String, Vec2)> = Vec::new();
    for (body, tf, name) in bodies.iter() {
        let p = tf.translation.truncate();
        match body.body_type {
            BodyType::Star => {
                stella.get_or_insert(p);
            }
            BodyType::Planet => {
                let n = name.map(|n| n.as_str().to_string()).unwrap_or_else(|| body.name.clone());
                pianeti.push((n, p));
            }
            _ => {}
        }
    }
    let Some(s) = stella else {
        write_probe("{\"stella\":null,\"pianeti\":[]}".to_string());
        return;
    };
    let mut righe: Vec<String> = Vec::new();
    for (nome, p) in &pianeti {
        let (dx, dy) = (p.x - s.x, p.y - s.y);
        let nm = normal_multi_bordo_ombra(dx, dy);
        righe.push(format!(
            "{{\"nome\":\"{}\",\"dx\":{:.1},\"dy\":{:.1},\"quadrante\":\"{}\",\"normal_multi_ombra\":{:.3},\"luce_colore_raw\":[{:.3},{:.3},{:.3}],\"falloff\":{:.3},\"surface\":{:.3},\"angle_multi\":{:.3}}}",
            nome,
            dx,
            dy,
            quadrante(dx, dy),
            nm,
            LUCE_COLORE_STELLA[0],
            LUCE_COLORE_STELLA[1],
            LUCE_COLORE_STELLA[2],
            1.0_f32,
            LUCE_INTENSITA_STELLA,
            1.0_f32,
        ));
    }
    write_probe(format!(
        "{{\"stella\":[{:.1},{:.1}],\"pianeti\":[{}]}}",
        s.x,
        s.y,
        righe.join(",")
    ));
}

fn write_probe(json: String) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut g) = crate::js_bridge::BORDO_PROBE.lock() {
            *g = json;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = json;
    }
}
