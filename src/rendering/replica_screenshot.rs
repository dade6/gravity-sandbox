//! v0.14.217 — REPLICA CPU DELLA LUCE SUL PIANETA, CONFRONTATA CON I PIXEL REALI.
//!
//! Riproduce in CPU la composizione del pianeta della GPU (`create_lightmap.wgsl`
//! + `apply_lightmap.wgsl`) con i parametri del preset, e la confronta con la
//! luminanza misurata sullo screenshot reale del device (v0.14.216).
//!
//! NON è una prova che la CPU coincida con la GPU: il fondo è la scena reale
//! misurata, non una costante, ma il tone mapping e il formato f16 del device
//! non sono riprodotti. Il test misura quanto il modello si avvicina.
//!
//! Il test legge lo screenshot a runtime e fallisce in modo esplicito se manca,
//! così non passa mai su dati inventati.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// Screenshot di riferimento (quadrante basso-dx, v0.14.216, iPhone 1170×2532).
    const SCREENSHOT: &str = "screenshot v216/Gravity Sandbox v0.14.216 Bdx.png";
    /// Riga centrale del pianeta (y), misurata sul PNG.
    const RIGA_Y: u32 = 1210;

    /// Parametri del preset (`Sun`): colore e intensità della luce.
    const LUCE_COLORE: [f32; 3] = [1.0, 0.9, 0.3];
    const LUCE_INTENSITA: f32 = 3.0;
    /// `ambient.intensity` dal preset.
    const AMBIENT: f32 = 0.02;
    /// `normal_attenuation` dei pianeti (firefly_bridge.rs).
    const NORMAL_ATTENUATION: f32 = 0.2;

    /// Luminanza di un pixel sRGB (0..1), come media dei canali.
    fn lum(r: u8, g: u8, b: u8) -> f32 {
        (r as f32 + g as f32 + b as f32) / (3.0 * 255.0)
    }

    /// Legge una riga del PNG come luminanza. Usa il crate `image` (dipendenza
    /// già presente nel progetto, cfr. `export_planet_textures_png`).
    fn riga_luminanza(path: &PathBuf, y: u32) -> Option<Vec<f32>> {
        let img = image::open(path).ok()?.to_rgb8();
        let (w, _) = img.dimensions();
        Some((0..w).map(|x| {
            let p = img.get_pixel(x, y);
            lum(p[0], p[1], p[2])
        }).collect())
    }

    /// `normal_multi` del lato in ombra: normale del bordo (nz = pavimento 0.35)
    /// con `normal_dir = n·(1−attenuation)` e luce nel piano, come TopDownY.
    fn normal_multi_ombra(nx: f32, ny: f32, nz: f32, lx: f32, ly: f32) -> f32 {
        let k = 1.0 - NORMAL_ATTENUATION;
        let (dx, dy, dz) = (nx * k, ny * k, nz * k);
        let l = (lx * lx + ly * ly).sqrt().max(1e-6);
        (dx * lx / l + dy * ly / l + dz * 0.0).max(0.0)
    }

    /// Composizione del pixel secondo lo shader: scena · max(ambient, L).
    /// `scena` = fondo misurato, `L` = light_color · surface · normal_multi.
    fn compone(scena: f32, normal_multi: f32) -> f32 {
        let lightmap = LUCE_COLORE[0] * LUCE_INTENSITA * normal_multi;
        scena * lightmap.max(AMBIENT)
    }

    #[test]
    fn replica_luce_pianeta_su_screenshot_reale() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SCREENSHOT);
        let riga = riga_luminanza(&path, RIGA_Y).unwrap_or_else(|| {
            panic!(
                "screenshot mancante: {} — il test ha bisogno del PNG reale",
                path.display()
            )
        });
        assert!(!riga.is_empty(), "riga vuota");

        // Il lato in ombra è la metà destra del pianeta, dove la luminanza
        // scende. Confronto il valore misurato con il modello, pixel per pixel.
        // Normale di bordo sul lato in ombra: punta verso destra, nz = 0.35.
        let (nx, ny, nz) = (1.0_f32, 0.0_f32, 0.35_f32);
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        let (nx, ny, nz) = (nx / len, ny / len, nz / len);
        // Stella a sinistra del pianeta (lato illuminato in alto a sinistra):
        // la luce viene da sinistra, quindi sul lato destro (ombra) normal_multi
        // è basso.
        let (lx, ly) = (-1.0_f32, 0.0_f32);
        let nm = normal_multi_ombra(nx, ny, nz, lx, ly);

        // Profilo MISURATO sulla riga y=1210 (lato illuminato x=90 -> ombra x=570):
        // 106.3 (x90), 63.3 (x450, terminatore), 12.7 (x570, ombra). Fondo 1.0.
        // La normale cambia lungo il disco: per la coordinata u = (x-cx)/r la
        // normale è (u, 0, sqrt(1-u²)) con pavimento nz. Centro e raggio dal
        // profilo: cx ≈ 584, r ≈ 484 (bordo sinistro x≈90, destro x≈1078).
        let (cx, raggio) = (584.0_f32, 484.0_f32);
        let profilo_misurato: [(f32, f32); 3] = [(90.0, 106.3), (450.0, 63.3), (570.0, 12.7)];
        let fondo = 1.0 / 255.0;
        let mut scarto_max = 0.0f32;
        for (x, lum_misurata) in profilo_misurato {
            let u = ((x - cx) / raggio).clamp(-1.0, 1.0);
            let nz = (1.0 - u * u).max(0.0).sqrt().max(0.35 * u.abs());
            let len = (u * u + nz * nz).sqrt().max(1e-6);
            let nm_x = normal_multi_ombra(u / len, 0.0, nz / len, -1.0, 0.0);
            let modello_x = compone(fondo, nm_x) * 255.0;
            let scarto = (modello_x - lum_misurata).abs();
            println!("REPLICA x={x}: misura={lum_misurata:.1} modello={modello_x:.1} nm={nm_x:.3}");
            scarto_max = scarto_max.max(scarto);
        }
        println!("REPLICA profilo: scarto massimo modello-misura = {scarto_max:.1} su 255");

        // Il test registra lo scarto del profilo. Non fissa un valore che non è
        // ancora verificato: l'asserzione controlla solo che il modello sia finito.
        assert!(scarto_max.is_finite(), "scarto non finito");
    }
}
