//! Velocità per orbite circolari (ADR 0002).
//!
//! Matematica pura, niente ECS: la UI (`ui.rs`) raccoglie i dati dai corpi
//! e chiama [`orbit_readiness`], che decide se il bottone "Orbita circolare"
//! è pronto (e con quale velocità) oppure disabilitato (e perché).
//!
//! La formula è coerente con `gravity_system` (`F = G·m1·m2 / (r²+s²)`):
//! uguagliando centripeta e gravità viene
//! `v = sqrt(G·M·r / (r²+s²))`. Per `r >> s` coincide con la scolastica
//! `sqrt(G·M/r)`.
use bevy::prelude::Vec2;

/// Stella di riferimento candidata (posizione, massa, raggio).
#[derive(Debug, Clone, Copy)]
pub struct RefStar {
    pub pos: Vec2,
    pub mass: f32,
    pub radius: f32,
}

/// Esito del calcolo per il corpo selezionato.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrbitReadiness {
    /// Bottone attivo: questa è la velocità da scrivere in `LinearVelocity`.
    Ready { velocity: Vec2 },
    /// Bottone disabilitato, col motivo da mostrare in etichetta.
    Disabled(&'static str),
}

/// Modulo della velocità circolare a distanza `distance` da una massa
/// centrale `star_mass`, con costante `g` e softening `softening`.
pub fn circular_speed(star_mass: f32, distance: f32, g: f32, softening: f32) -> f32 {
    (g * star_mass * distance / (distance * distance + softening * softening)).sqrt()
}

/// Vettore velocità circolare, perpendicolare al raggio stella→corpo.
///
/// Il verso conserva quello attuale (segno del momento angolare rispetto
/// alla stella); se la velocità è zero o puramente radiale il default è
/// antiorario. Richiede `body_pos != star_pos` (il chiamante garantisce
/// `r >= somma raggi`, vedi [`orbit_readiness`]).
pub fn circular_velocity(
    star_pos: Vec2,
    body_pos: Vec2,
    body_vel: Vec2,
    star_mass: f32,
    g: f32,
    softening: f32,
) -> Vec2 {
    let radial = body_pos - star_pos;
    let r = radial.length();
    if r < 1e-6 {
        return Vec2::ZERO;
    }
    let speed = circular_speed(star_mass, r, g, softening);
    // Tangente antioraria = raggio ruotato di +90°.
    let tangent_ccw = Vec2::new(-radial.y, radial.x) / r;
    // Momento angolare (componente z) rispetto alla stella:
    // > 0 antiorario, < 0 orario, == 0 radiale puro o fermo.
    let ang = radial.x * body_vel.y - radial.y * body_vel.x;
    if ang < 0.0 {
        -tangent_ccw * speed
    } else {
        tangent_ccw * speed
    }
}

/// Decide lo stato del bottone per il corpo selezionato.
///
/// - Solo in pausa (coerente con tutto l'editing).
/// - Il selezionato deve essere un non-stella; serve almeno una stella.
/// - Centro = stella più vicina; se `r < raggio stella + raggio corpo`
///   (anti divisione-per-zero) il bottone è disabilitato.
#[allow(clippy::too_many_arguments)]
pub fn orbit_readiness(
    body_pos: Vec2,
    body_vel: Vec2,
    body_radius: f32,
    body_is_star: bool,
    stars: &[RefStar],
    paused: bool,
    g: f32,
    softening: f32,
) -> OrbitReadiness {
    if !paused {
        return OrbitReadiness::Disabled("Pausa per usare");
    }
    if body_is_star {
        return OrbitReadiness::Disabled("È una stella");
    }
    let mut nearest: Option<&RefStar> = None;
    for star in stars {
        let d = body_pos.distance(star.pos);
        match nearest {
            Some(n) if body_pos.distance(n.pos) <= d => {}
            _ => nearest = Some(star),
        }
    }
    let star = match nearest {
        Some(s) => s,
        None => return OrbitReadiness::Disabled("Nessuna stella"),
    };
    let r = body_pos.distance(star.pos);
    if r < star.radius + body_radius {
        return OrbitReadiness::Disabled("Troppo vicino");
    }
    OrbitReadiness::Ready {
        velocity: circular_velocity(star.pos, body_pos, body_vel, star.mass, g, softening),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Numeri del preset debug: Sole M=5000 in origine, G=5000, s=5.
    const G: f32 = 5000.0;
    const SUN_M: f32 = 5000.0;
    const S: f32 = 5.0;

    fn sun_at_origin() -> Vec<RefStar> {
        vec![RefStar {
            pos: Vec2::ZERO,
            mass: SUN_M,
            radius: 30.0,
        }]
    }

    #[test]
    fn circular_speed_matches_engine_formula() {
        // Planet Alpha a r=200: v = sqrt(5000*5000*200/(200²+25)).
        let v = circular_speed(SUN_M, 200.0, G, S);
        let expected = (5000.0f32 * 5000.0 * 200.0 / (200.0 * 200.0 + 25.0)).sqrt();
        assert!((v - expected).abs() < 1e-3);
        assert!((v - 353.44).abs() < 0.05, "v divergente: {v}");
    }

    #[test]
    fn zero_velocity_defaults_to_counterclockwise() {
        // Corpo fermo a (200,0): tangente antioraria = +Y.
        let r = orbit_readiness(
            Vec2::new(200.0, 0.0),
            Vec2::ZERO,
            12.0,
            false,
            &sun_at_origin(),
            true,
            G,
            S,
        );
        match r {
            OrbitReadiness::Ready { velocity } => {
                assert!(
                    velocity.x.abs() < 1e-3,
                    "deve essere perpendicolare: {velocity}"
                );
                assert!(velocity.y > 0.0, "default antiorario: {velocity}");
            }
            OrbitReadiness::Disabled(reason) => panic!("doveva essere Ready: {reason}"),
        }
    }

    #[test]
    fn radial_velocity_defaults_to_counterclockwise() {
        // Velocità puramente radiale (in caduta verso la stella).
        let r = orbit_readiness(
            Vec2::new(200.0, 0.0),
            Vec2::new(-50.0, 0.0),
            12.0,
            false,
            &sun_at_origin(),
            true,
            G,
            S,
        );
        match r {
            OrbitReadiness::Ready { velocity } => {
                assert!(velocity.y > 0.0, "default antiorario: {velocity}");
            }
            OrbitReadiness::Disabled(reason) => panic!("doveva essere Ready: {reason}"),
        }
    }

    #[test]
    fn clockwise_sense_is_preserved() {
        // Sta già orbitando in senso orario: non ribaltare.
        let r = orbit_readiness(
            Vec2::new(200.0, 0.0),
            Vec2::new(0.0, -100.0),
            12.0,
            false,
            &sun_at_origin(),
            true,
            G,
            S,
        );
        match r {
            OrbitReadiness::Ready { velocity } => {
                assert!(velocity.y < 0.0, "verso orario conservato: {velocity}");
            }
            OrbitReadiness::Disabled(reason) => panic!("doveva essere Ready: {reason}"),
        }
    }

    #[test]
    fn counterclockwise_sense_is_preserved() {
        let r = orbit_readiness(
            Vec2::new(200.0, 0.0),
            Vec2::new(0.0, 100.0),
            12.0,
            false,
            &sun_at_origin(),
            true,
            G,
            S,
        );
        match r {
            OrbitReadiness::Ready { velocity } => {
                assert!(velocity.y > 0.0, "verso antiorario conservato: {velocity}");
            }
            OrbitReadiness::Disabled(reason) => panic!("doveva essere Ready: {reason}"),
        }
    }

    #[test]
    fn nearest_star_wins() {
        // Due stelle: vince la più vicina anche se meno massiccia.
        let stars = vec![
            RefStar {
                pos: Vec2::new(1000.0, 0.0),
                mass: 50000.0,
                radius: 30.0,
            },
            RefStar {
                pos: Vec2::ZERO,
                mass: SUN_M,
                radius: 30.0,
            },
        ];
        let r = orbit_readiness(
            Vec2::new(200.0, 0.0),
            Vec2::ZERO,
            12.0,
            false,
            &stars,
            true,
            G,
            S,
        );
        match r {
            OrbitReadiness::Ready { velocity } => {
                // Attorno al Sole in origine: v ≈ 353.4, non ~790 della massiccia.
                assert!((velocity.length() - 353.44).abs() < 0.05, "{velocity}");
            }
            OrbitReadiness::Disabled(reason) => panic!("doveva essere Ready: {reason}"),
        }
    }

    #[test]
    fn disabled_cases() {
        let stars = sun_at_origin();
        // a) selezionato è una stella
        assert_eq!(
            orbit_readiness(Vec2::ZERO, Vec2::ZERO, 30.0, true, &stars, true, G, S),
            OrbitReadiness::Disabled("È una stella")
        );
        // b) nessuna stella in scena
        assert_eq!(
            orbit_readiness(
                Vec2::new(200.0, 0.0),
                Vec2::ZERO,
                12.0,
                false,
                &[],
                true,
                G,
                S
            ),
            OrbitReadiness::Disabled("Nessuna stella")
        );
        // c) dentro la stella (r < 30 + 12 = 42)
        assert_eq!(
            orbit_readiness(
                Vec2::new(20.0, 0.0),
                Vec2::ZERO,
                12.0,
                false,
                &stars,
                true,
                G,
                S
            ),
            OrbitReadiness::Disabled("Troppo vicino")
        );
        // d) simulazione in corso
        assert_eq!(
            orbit_readiness(
                Vec2::new(200.0, 0.0),
                Vec2::ZERO,
                12.0,
                false,
                &stars,
                false,
                G,
                S
            ),
            OrbitReadiness::Disabled("Pausa per usare")
        );
    }
}
