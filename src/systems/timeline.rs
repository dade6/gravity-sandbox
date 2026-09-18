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
        Self {
            paused: false,
            speed: 1.0,
        }
    }
}

/// Messaggio per richiedere uno step singolo (osservabilita'/test: la
/// logica di esecuzione vera passa da `PendingSteps`).
#[derive(Message)]
pub struct StepMessage;

/// Step armati ma non ancora eseguiti: ogni unita' = esattamente UN tick
/// fisico (1/64 s di tempo simulato alla velocita' corrente).
/// Produttori: bottone UI Step, tastiera (Freccia Destra / '.').
/// Consumatore: `arm_steps` (sequencer nel RunFixedMainLoop).
#[derive(Resource, Default)]
pub struct PendingSteps(pub u32);

/// Plugin per la timeline (Play/Pause/Step/Speed)
pub struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationState>()
            .init_resource::<PendingSteps>()
            .add_message::<StepMessage>()
            .add_systems(
                Update,
                (
                    handle_play_pause,
                    handle_step,
                    handle_speed_change,
                    apply_speed,
                ),
            )
            // Sequencer dello step singolo: gira DENTRO il wrapper del fixed
            // loop, prima e dopo i tick effettivi (vedi arm_steps).
            .add_systems(
                RunFixedMainLoop,
                (
                    arm_steps.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                    relock_physics_clock.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
                ),
            );
    }
}

/// Toggle Pause/Play con Spazio
/// Ferma sia il tempo virtuale Bevy sia il timer fisico di Avian.
fn handle_play_pause(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
    mut pending_steps: ResMut<PendingSteps>,
) {
    crate::mark_system("handle_play_pause");

    if keys.just_pressed(KeyCode::Space) {
        sim_state.paused = !sim_state.paused;
        // Play/Pause azzera gli step armati: Play = esecuzione continua,
        // Pause = stato congelato pulito (niente tick residui).
        pending_steps.0 = 0;
        if sim_state.paused {
            virtual_time.pause();
            physics_time.pause();
        } else {
            virtual_time.unpause();
            physics_time.unpause();
        }
    }
}

/// Step avanti con Freccia Destra o tasto '.' quando in pausa.
/// Arma un tick nel sequencer (`PendingSteps`): NON tocca gli orologi qui.
/// Il fixed loop gira PRIMA di Update nel frame (First -> PreUpdate ->
/// RunFixedMainLoop -> Update), quindi un'unpause immediata in Update non
/// produrrebbe mai un avanzamento nel frame corrente (era il bug storico
/// del tasto step: unpause+repause nello stesso frame = no-op).
fn handle_step(
    keys: Res<ButtonInput<KeyCode>>,
    sim_state: Res<SimulationState>,
    mut pending_steps: ResMut<PendingSteps>,
    mut step_writer: MessageWriter<StepMessage>,
) {
    crate::mark_system("handle_step");

    if sim_state.paused
        && (keys.just_pressed(KeyCode::Period) || keys.just_pressed(KeyCode::ArrowRight))
    {
        pending_steps.0 = pending_steps.0.saturating_add(1);
        step_writer.write(StepMessage);
    }
}

/// Re-pausa gli orologi dopo i tick armati. Gira in AfterFixedMainLoop,
/// cioe' DOPO tutte le iterazioni del fixed loop di questo frame: i tick
/// seminati da `arm_steps` sono gia' stati eseguiti.
///
/// A differenza del vecchio `repause_after_step` (in `Last`), che chiudeva
/// la finestra di unpause PRIMA che il fixed loop del frame successivo
/// potesse consumare tempo (lo step era un no-op per costruzione), qui il
/// relock avviene dopo l'avanzamento reale.
fn relock_physics_clock(
    sim_state: Res<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    crate::mark_system("relock_physics_clock");

    if sim_state.paused {
        virtual_time.pause();
        physics_time.pause();
    }
}

/// Sequencer dello step singolo (BeforeFixedMainLoop, una volta per frame):
///
/// 1. Se la simulazione non e' in pausa gli step armati sono obsoleti
///    (Play subentra): li azzera ed esce.
/// 2. Per ogni step armato semina l'accumulatore di `Time<Fixed>` con UN
///    timestep (64 Hz) tramite l'API pubblica `accumulate_overstep`: con
///    `Time<Virtual>` in pausa l'overstep NON cresce da solo, quindi il
///    fixed loop girera' esattamente le iterazioni seminate.
/// 3. Apre `Time<Physics>` (unpause) per la durata del loop: Avian avanza
///    la simulazione di `timestep x relative_speed` per iterazione, come
///    in play. `Time<Virtual>` resta in pausa: nessun tempo extra si
///    accumula dal frame reale.
/// 4. `relock_physics_clock` (AfterFixedMainLoop) richiude gli orologi.
///
/// Un click = esattamente 1/64 s di simulazione alla velocita' corrente,
/// a qualsiasi framerate (60/120 Hz, iPhone Safari, tab in background).
fn arm_steps(
    sim_state: Res<SimulationState>,
    mut pending_steps: ResMut<PendingSteps>,
    mut physics_time: ResMut<Time<Physics>>,
    mut fixed_time: ResMut<Time<Fixed>>,
) {
    crate::mark_system("arm_steps");

    if pending_steps.0 == 0 {
        return;
    }
    if !sim_state.paused {
        // Difensivo: handle_play_pause azzera gia' al toggle, ma se uno
        // step arriva da qualunque altro path mentre si e' in play, i tick
        // continui subentrano agli step residui.
        pending_steps.0 = 0;
        return;
    }

    let timestep = fixed_time.timestep();
    for _ in 0..pending_steps.0 {
        fixed_time.accumulate_overstep(timestep);
    }
    pending_steps.0 = 0;

    // Finestra di unpause SOLO per Time<Physics>: il fixed loop consumera'
    // l'overstep seminato; il tempo virtuale resta in pausa.
    physics_time.unpause();
}

/// Cambio velocita' con +/- o tasti 5-8
fn handle_speed_change(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimulationState>,
    input_focus: Res<bevy::input_focus::InputFocus>,
) {
    crate::mark_system("handle_speed_change");
    // Campo attivo: i tasti vanno al campo, non alle shortcut (5-8, +/-)
    if input_focus.get().is_some() {
        return;
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::celestial::{BodyType, CelestialBody};
    use crate::systems::persistence::GravitationalConstant;
    use avian2d::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    /// App headless con Avian + TimelinePlugin: un app.update() = un frame
    /// da 16 ms di tempo reale. Gli orologi vengono messi in pausa dai test
    /// con lo stesso stato che produce il bottone Pause.
    fn step_test_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            PhysicsPlugins::default(),
            TimelinePlugin,
            crate::systems::gravity::GravityPlugin,
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(GravitationalConstant(5000.0))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )))
        // Risorse che nel runtime completo arrivano dai plugin input:
        // i sistemi tastiera di TimelinePlugin non devono panicare headless.
        .insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(bevy::input_focus::InputFocus::default());
        // Avian registra le risorse diagnostica in finish(): senza questa
        // chiamata alcuni sistemi panicano (stesso harness dei test Avian).
        app.finish();
        app
    }

    fn spawn_planet(app: &mut App, x: f32, vx: f32) -> Entity {
        app.world_mut()
            .spawn((
                CelestialBody {
                    name: "P".into(),
                    body_type: BodyType::Planet,
                    mass: 100.0,
                    radius: 10.0,
                    color: [0.5, 0.5, 0.8],
                    luminous: false,
                },
                Transform::from_xyz(x, 0.0, 0.0),
                RigidBody::Dynamic,
                Collider::circle(10.0),
                Mass(100.0),
                LinearVelocity(Vec2::new(vx, 0.0)),
                ConstantForce(Vec2::ZERO),
            ))
            .id()
    }

    /// Pausa come la produce il bottone UI: SimulationState + entrambi gli
    /// orologi fermi.
    fn pause_sim(app: &mut App) {
        app.world_mut().resource_mut::<SimulationState>().paused = true;
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.world_mut().resource_mut::<Time<Physics>>().pause();
    }

    fn pos_x(app: &App, e: Entity) -> f32 {
        app.world()
            .entity(e)
            .get::<Transform>()
            .unwrap()
            .translation
            .x
    }

    const TICK_SECS: f32 = 1.0 / 64.0;

    #[test]
    fn armed_step_advances_exactly_one_tick_while_paused() {
        let mut app = step_test_app();
        let e = spawn_planet(&mut app, 0.0, 100.0);
        pause_sim(&mut app);

        // Due frame di assestamento in pausa: niente deve muoversi.
        for _ in 0..2 {
            app.update();
        }
        assert_eq!(pos_x(&app, e), 0.0, "in pausa il corpo non deve muoversi");

        // Un click = un tick armato.
        app.world_mut().resource_mut::<PendingSteps>().0 = 1;
        app.update();
        let dx = pos_x(&app, e);
        let expected = 100.0 * TICK_SECS;
        assert!(
            (dx - expected).abs() < 1e-4,
            "uno step = esattamente un tick (atteso {}, ottenuto {})",
            expected,
            dx
        );

        // Il sequencer richiude gli orologi e consuma la coda.
        assert!(
            app.world().resource::<Time<Physics>>().is_paused(),
            "Time<Physics> deve tornare in pausa dopo lo step"
        );
        assert!(
            app.world().resource::<Time<Virtual>>().is_paused(),
            "Time<Virtual> deve restare in pausa"
        );
        assert_eq!(app.world().resource::<PendingSteps>().0, 0);
        assert_eq!(
            app.world().resource::<Time<Fixed>>().overstep(),
            Duration::ZERO,
            "l'overstep seminato deve essere consumato tutto"
        );

        // Nessun tick extra senza nuovi click.
        app.update();
        assert_eq!(
            pos_x(&app, e),
            dx,
            "dopo lo step armato la simulazione deve restare congelata"
        );
    }

    #[test]
    fn paused_without_armed_steps_freezes_completely() {
        let mut app = step_test_app();
        let e = spawn_planet(&mut app, 50.0, -80.0);
        pause_sim(&mut app);

        for _ in 0..5 {
            app.update();
        }
        assert_eq!(pos_x(&app, e), 50.0, "in pausa nessun movimento");
        assert_eq!(
            app.world().resource::<Time<Physics>>().elapsed(),
            Duration::ZERO,
            "in pausa il tempo di fisica non avanza"
        );
        assert_eq!(app.world().resource::<PendingSteps>().0, 0);
    }

    #[test]
    fn multiple_armed_steps_advance_multiple_ticks_in_one_frame() {
        let mut app = step_test_app();
        let e = spawn_planet(&mut app, 0.0, 100.0);
        pause_sim(&mut app);
        for _ in 0..2 {
            app.update();
        }

        // Tre click arrivati nello stesso frame (tap rapidi): 3 tick.
        app.world_mut().resource_mut::<PendingSteps>().0 = 3;
        app.update();
        let dx = pos_x(&app, e);
        let expected = 100.0 * 3.0 * TICK_SECS;
        assert!(
            (dx - expected).abs() < 1e-4,
            "3 step armati = 3 tick (atteso {}, ottenuto {})",
            expected,
            dx
        );
        assert!(app.world().resource::<Time<Physics>>().is_paused());
        assert_eq!(app.world().resource::<PendingSteps>().0, 0);
    }

    #[test]
    fn armed_step_respects_speed_multiplier() {
        let mut app = step_test_app();
        let e = spawn_planet(&mut app, 0.0, 100.0);
        pause_sim(&mut app);
        for _ in 0..2 {
            app.update();
        }

        // Velocita' 2x come dal pannello: apply_speed la riflette sugli orologi.
        app.world_mut().resource_mut::<SimulationState>().speed = 2.0;
        app.update();
        assert!((app.world().resource::<Time<Physics>>().relative_speed() - 2.0).abs() < 1e-6);

        app.world_mut().resource_mut::<PendingSteps>().0 = 1;
        app.update();
        let dx = pos_x(&app, e);
        let expected = 100.0 * TICK_SECS * 2.0;
        assert!(
            (dx - expected).abs() < 1e-4,
            "uno step a velocita' 2x = 2x dt per tick (atteso {}, ottenuto {})",
            expected,
            dx
        );
        // Il tempo di fisica avanza del doppio del timestep base.
        assert_eq!(
            app.world().resource::<Time<Physics>>().delta(),
            Duration::from_secs_f64(TICK_SECS as f64 * 2.0),
            "un tick a 2x avanza Time<Physics> di 2x timestep"
        );
    }
}
