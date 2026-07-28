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
            .add_systems(PostUpdate, repause_after_step);
    }
}

/// Toggle Pause/Play con Spazio
fn handle_play_pause(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        sim_state.paused = !sim_state.paused;
        if sim_state.paused {
            virtual_time.pause();
        } else {
            virtual_time.unpause();
        }
    }
}

/// Step avanti con Freccia Destra o tasto '.' quando in pausa
fn handle_step(
    keys: Res<ButtonInput<KeyCode>>,
    sim_state: Res<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut step_writer: MessageWriter<StepMessage>,
) {
    if sim_state.paused
        && (keys.just_pressed(KeyCode::Period) || keys.just_pressed(KeyCode::ArrowRight))
    {
        virtual_time.unpause();
        step_writer.write(StepMessage);
    }
}

/// Re-pausa dopo uno step (eseguito dopo FixedUpdate)
fn repause_after_step(
    mut step_reader: MessageReader<StepMessage>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    if step_reader.read().next().is_some() && sim_state.paused {
        virtual_time.pause();
    }
}

/// Cambio velocità con +/- o tasti 1-8
fn handle_speed_change(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimulationState>,
) {
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        sim_state.speed = (sim_state.speed * 2.0).min(10.0);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        sim_state.speed = (sim_state.speed * 0.5).max(0.1);
    }
    for (key, val) in [
        (KeyCode::Digit1, 0.1),
        (KeyCode::Digit2, 0.25),
        (KeyCode::Digit3, 0.5),
        (KeyCode::Digit4, 1.0),
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

/// Applica la velocita' a Time<Virtual>
fn apply_speed(
    sim_state: Res<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    virtual_time.set_relative_speed(sim_state.speed);
}
