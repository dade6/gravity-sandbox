use bevy::prelude::*;

/// Stato iniziale di un corpo celeste, catturato al momento dello spawn.
///
/// Usato dal bottone "Reset" (Ticket 15) per ripristinare posizione,
/// velocità, massa e raggio di ogni corpo allo stato di partenza
/// senza ricaricare la pagina.
#[derive(Component, Debug, Clone, Copy)]
pub struct InitialBodyState {
    pub position: Vec2,
    pub velocity: Vec2,
    pub mass: f32,
    pub radius: f32,
}
