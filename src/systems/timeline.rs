use avian2d::prelude::{Physics, PhysicsTime};
use bevy::prelude::*;

/// Stato della simulazione
#[derive(Resource)]
pub struct SimulationState {
    pub paused: bool,
    pub speed: f32,
}

impl Default for SimulationState {
    fn default() -> Self {
        Self { paused: false, speed: 1.0 }
    }
}

/// Messaggio per richiedere uno step singolo
#[derive(Message)]
pub struct StepMessage;

/// Plugin per la timeline (Play/Pause/Step/Speed)
pub struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationState>()
            .add_message::<StepMessage>()
            .add_systems(Update, (
                handle_play_pause,
                handle_step,
                handle_speed_change,
                apply_speed,
            ))
            .add_systems(Last, repause_after_step);
    }
}

/// Toggle Pause/Play con Spazio
/// Ferma sia il tempo virtuale Bevy sia il timer fisico di Avian.
fn handle_play_pause(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    crate::mark_system("handle_play_pause");

    if keys.just_pressed(KeyCode::Space) {
        sim_state.paused = !sim_state.paused;
        if sim_state.paused {
            virtual_time.pause();
            physics_time.pause();
        } else {
            virtual_time.unpause();
            physics_time.unpause();
        }
    }
}

/// Step avanti con Freccia Destra o tasto '.' quando in pausa
fn handle_step(
    keys: Res<ButtonInput<KeyCode>>,
    sim_state: Res<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
    mut step_writer: MessageWriter<StepMessage>,
) {
    crate::mark_system("handle_step");

    if sim_state.paused
        && (keys.just_pressed(KeyCode::Period) || keys.just_pressed(KeyCode::ArrowRight))
    {
        virtual_time.unpause();
        physics_time.unpause();
        step_writer.write(StepMessage);
    }
}

/// Re-pausa dopo uno step (eseguito in Last, dopo che la fisica ha fatto il passo)
fn repause_after_step(
    mut step_reader: MessageReader<StepMessage>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    if step_reader.read().next().is_some() && sim_state.paused {
        virtual_time.pause();
        physics_time.pause();
    }
}

/// Cambio velocità con +/- o tasti 5-8
fn handle_speed_change(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimulationState>,
) {
    crate::mark_system("handle_speed_change");

    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        sim_state.speed = (sim_state.speed * 2.0).min(10.0);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        sim_state.speed = (sim_state.speed * 0.5).max(0.1);
    }
    for (key, val) in [
        (KeyCode::Digit5, 2.0),
        (KeyCode::Digit6, 4.0),
        (KeyCode::Digit7, 8.0),
        (KeyCode::Digit8, 10.0),
    ] {
        if keys.just_pressed(key) {
            sim_state.speed = val;
        }
    }
}

/// Applica la velocita' a Time<Virtual> e Time<Physics>
fn apply_speed(
    sim_state: Res<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    crate::mark_system("apply_speed");

    virtual_time.set_relative_speed(sim_state.speed);
    physics_time.set_relative_speed(sim_state.speed);
}
