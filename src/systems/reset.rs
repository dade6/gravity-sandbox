use avian2d::prelude::*;
use bevy::prelude::*;

use crate::components::celestial::{BodyType, CelestialBody};
use crate::components::initial_state::InitialBodyState;
use crate::components::trajectory::TrajectoryHistory;
use crate::systems::selection::SelectedBody;
use crate::systems::tools::PendingDelete;

/// Messaggio per richiedere il reset della simulazione allo stato iniziale
/// (posizioni/velocità/masse/raggi di partenza, senza ricaricare la pagina).
#[derive(Message)]
pub struct ResetMessage;

/// Plugin per il reset della simulazione.
pub struct ResetPlugin;

impl Plugin for ResetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ResetMessage>()
            .add_systems(Update, (lazy_init_initial_state, reset_simulation));
    }
}

/// Lazy init: cattura lo stato iniziale per i corpi sprovvisti di
/// `InitialBodyState` (spawnati da percorsi che non lo registrano
/// esplicitamente). Il primo frame in cui questo sistema vede il corpo
/// coincide con il momento dello spawn, quindi lo stato registrato è
/// quello iniziale del corpo.
fn lazy_init_initial_state(
    mut commands: Commands,
    bodies: Query<
        (Entity, &Transform, &LinearVelocity, &CelestialBody),
        Without<InitialBodyState>,
    >,
) {
    crate::mark_system("lazy_init_initial_state");

    for (entity, transform, velocity, body) in bodies.iter() {
        commands.entity(entity).insert(InitialBodyState {
            position: transform.translation.truncate(),
            velocity: velocity.0,
            mass: body.mass,
            radius: body.radius,
        });
    }
}

/// Sistema che ripristina lo stato iniziale di tutti i corpi esistenti
/// alla ricezione di `ResetMessage`.
///
/// - Ripristina posizione, velocità, massa e raggio dallo `InitialBodyState`.
/// - Pulisce le traiettorie (`TrajectoryHistory`) quando presenti.
/// - Rimuove eventuali `PendingDelete` e deseleziona.
/// - NON ricrea i corpi cancellati dall'utente: agisce solo sui corpi esistenti.
///
/// Funziona sia in play sia in pausa: non tocca `SimulationState` né i timer.
fn reset_simulation(
    mut reset_reader: MessageReader<ResetMessage>,
    mut selected: ResMut<SelectedBody>,
    mut pending: ResMut<PendingDelete>,
    mut bodies: Query<(
        &InitialBodyState,
        &mut Transform,
        &mut LinearVelocity,
        &mut Mass,
        &mut CelestialBody,
        Option<&mut TrajectoryHistory>,
    )>,
) {
    crate::mark_system("reset_simulation");

    if reset_reader.read().next().is_none() {
        return;
    }

    for (initial, mut transform, mut velocity, mut mass, mut body, history) in bodies.iter_mut()
    {
        transform.translation.x = initial.position.x;
        transform.translation.y = initial.position.y;
        velocity.0 = initial.velocity;
        mass.0 = initial.mass;
        body.mass = initial.mass;
        body.radius = initial.radius;
        // Le traiettorie possono mancare (es. corpi caricati da livello JSON):
        // in quel caso non c'è nulla da pulire.
        if let Some(mut history) = history {
            history.positions.clear();
        }
    }

    // Cleanup stato UI: niente più corpo in attesa di cancellazione né selezione.
    pending.0 = None;
    selected.0 = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::timeline::SimulationState;

    fn test_app() -> App {
        let mut app = App::new();
        app.init_resource::<SelectedBody>()
            .init_resource::<PendingDelete>()
            .add_plugins(ResetPlugin);
        app
    }

    /// Corpo di test completo di stato iniziale (come spawnato dai plugin).
    fn spawn_body(
        commands: &mut Commands,
        pos: Vec2,
        vel: Vec2,
        mass: f32,
        radius: f32,
    ) -> Entity {
        commands
            .spawn((
                CelestialBody {
                    name: "Test".into(),
                    body_type: BodyType::Planet,
                    mass,
                    radius,
                    color: [0.5, 0.5, 0.5],
                    luminous: false,
                },
                Transform::from_xyz(pos.x, pos.y, 0.0),
                RigidBody::Dynamic,
                Collider::circle(radius),
                Mass(mass),
                LinearVelocity(vel),
                TrajectoryHistory::default(),
                InitialBodyState {
                    position: pos,
                    velocity: vel,
                    mass,
                    radius,
                },
            ))
            .id()
    }

    fn run_updates(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    /// Acceptance 1+2+3: dopo Reset posizione/velocità/massa/raggio tornano
    /// ai valori iniziali e le traiettorie vengono svuotate.
    #[test]
    fn reset_restores_initial_state_and_clears_trails() {
        let mut app = test_app();
        let initial_pos = Vec2::new(200.0, 0.0);
        let initial_vel = Vec2::new(0.0, 80.0);
        let entity = {
            let mut world = app.world_mut();
            let mut commands = world.commands();
            let e = spawn_body(&mut commands, initial_pos, initial_vel, 200.0, 20.0);
            world.flush();
            e
        };

        // Rovina lo stato: sposta, cambia velocità/massa/raggio, sporca la traccia
        {
            let mut world = app.world_mut();
            world.entity_mut(entity).get_mut::<Transform>().unwrap().translation =
                Vec3::new(999.0, -555.0, 0.0);
            world
                .entity_mut(entity)
                .get_mut::<LinearVelocity>()
                .unwrap()
                .0 = Vec2::new(123.0, 456.0);
            world.entity_mut(entity).get_mut::<Mass>().unwrap().0 = 1.0;
            {
                let mut ent = world.entity_mut(entity);
                let mut body = ent.get_mut::<CelestialBody>().unwrap();
                body.mass = 1.0;
                body.radius = 2.0;
            }
            world
                .entity_mut(entity)
                .get_mut::<TrajectoryHistory>()
                .unwrap()
                .positions = vec![Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)];
        }

        // Trigger reset via message
        app.world_mut()
            .resource_mut::<Messages<ResetMessage>>()
            .write(ResetMessage);
        run_updates(&mut app, 2);

        let world = app.world();
        let transform = world.entity(entity).get::<Transform>().unwrap();
        assert_eq!(transform.translation.truncate(), initial_pos);
        let vel = world.entity(entity).get::<LinearVelocity>().unwrap();
        assert_eq!(vel.0, initial_vel);
        let mass = world.entity(entity).get::<Mass>().unwrap();
        assert_eq!(mass.0, 200.0);
        let body = world.entity(entity).get::<CelestialBody>().unwrap();
        assert_eq!(body.mass, 200.0);
        assert_eq!(body.radius, 20.0);
        let history = world.entity(entity).get::<TrajectoryHistory>().unwrap();
        assert!(history.positions.is_empty(), "trails must be cleared");
    }

    /// Acceptance 4: un corpo aggiunto con Add (con InitialBodyState al suo
    /// spawn) torna al punto in cui è stato creato.
    #[test]
    fn reset_restores_added_body_to_spawn_point() {
        let mut app = test_app();
        let spawn_pos = Vec2::new(-300.0, 150.0);
        let entity = {
            let mut world = app.world_mut();
            let mut commands = world.commands();
            let e = spawn_body(&mut commands, spawn_pos, Vec2::ZERO, 100.0, 15.0);
            world.flush();
            e
        };

        // Il corpo si è spostato nel frattempo
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Transform>()
            .unwrap()
            .translation = Vec3::new(50.0, 50.0, 0.0);

        app.world_mut()
            .resource_mut::<Messages<ResetMessage>>()
            .write(ResetMessage);
        run_updates(&mut app, 2);

        let world = app.world();
        assert_eq!(
            world.entity(entity).get::<Transform>().unwrap().translation.truncate(),
            spawn_pos
        );
    }

    /// Acceptance 5: i corpi cancellati NON vengono ricreati — Reset agisce
    /// solo sui corpi esistenti. (Il corpo cancellato non esiste più nell'ECS,
    /// quindi il reset non può riportarlo: verifichiamo che il conteggio dei
    /// corpi non aumenti e che non ci siano entità orfane.)
    #[test]
    fn reset_does_not_recreate_deleted_bodies() {
        let mut app = test_app();
        let a = {
            let mut world = app.world_mut();
            let mut commands = world.commands();
            let e = spawn_body(&mut commands, Vec2::ZERO, Vec2::ZERO, 100.0, 15.0);
            world.flush();
            e
        };
        // "Cancella" il corpo: lo despawn
        app.world_mut().despawn(a);
        app.world_mut().flush();

        app.world_mut()
            .resource_mut::<Messages<ResetMessage>>()
            .write(ResetMessage);
        run_updates(&mut app, 2);

        let mut world = app.world_mut();
        let count = world.query::<&CelestialBody>().iter(world).count();
        assert_eq!(count, 0, "deleted bodies must not reappear");
    }

    /// Acceptance 6: Reset in play mode — il sistema non tocca la pausa.
    #[test]
    fn reset_does_not_change_paused_state() {
        let mut app = test_app();
        let entity = {
            let mut world = app.world_mut();
            let mut commands = world.commands();
            let e = spawn_body(&mut commands, Vec2::ZERO, Vec2::ZERO, 100.0, 15.0);
            world.flush();
            e
        };
        // Inserisci SimulationState non in pausa (play mode)
        app.world_mut()
            .insert_resource(SimulationState { paused: false, speed: 2.0 });
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Transform>()
            .unwrap()
            .translation = Vec3::new(1.0, 1.0, 0.0);

        app.world_mut()
            .resource_mut::<Messages<ResetMessage>>()
            .write(ResetMessage);
        run_updates(&mut app, 2);

        let world = app.world();
        let sim = world.resource::<SimulationState>();
        assert!(!sim.paused, "reset must not pause the simulation");
        assert_eq!(sim.speed, 2.0);
        assert_eq!(
            world.entity(entity).get::<Transform>().unwrap().translation.truncate(),
            Vec2::ZERO
        );
    }

    /// Cleanup: Reset rimuove PendingDelete e deseleziona.
    #[test]
    fn reset_clears_pending_delete_and_selection() {
        let mut app = test_app();
        let entity = {
            let mut world = app.world_mut();
            let mut commands = world.commands();
            let e = spawn_body(&mut commands, Vec2::ZERO, Vec2::ZERO, 100.0, 15.0);
            world.flush();
            e
        };
        app.world_mut().resource_mut::<PendingDelete>().0 = Some(entity);
        app.world_mut().resource_mut::<SelectedBody>().0 = Some(entity);

        app.world_mut()
            .resource_mut::<Messages<ResetMessage>>()
            .write(ResetMessage);
        run_updates(&mut app, 2);

        let world = app.world();
        assert!(world.resource::<PendingDelete>().0.is_none());
        assert!(world.resource::<SelectedBody>().0.is_none());
    }

    /// Lazy init: un corpo spawnato SENZA InitialBodyState lo riceve al primo
    /// frame, con lo stato corrente (= momento dello spawn).
    #[test]
    fn lazy_init_captures_state_for_bodies_without_initial_state() {
        let mut app = test_app();
        let entity = {
            let mut world = app.world_mut();
            let e = world
                .spawn((
                    CelestialBody {
                        name: "NoInit".into(),
                        body_type: BodyType::Planet,
                        mass: 42.0,
                        radius: 9.0,
                        color: [0.5, 0.5, 0.5],
                        luminous: false,
                    },
                    Transform::from_xyz(10.0, -20.0, 0.0),
                    RigidBody::Dynamic,
                    Collider::circle(9.0),
                    Mass(42.0),
                    LinearVelocity(Vec2::new(1.0, 2.0)),
                    TrajectoryHistory::default(),
                ))
                .id();
            world.flush();
            e
        };

        run_updates(&mut app, 1);

        let world = app.world();
        let init = world.entity(entity).get::<InitialBodyState>().unwrap();
        assert_eq!(init.position, Vec2::new(10.0, -20.0));
        assert_eq!(init.velocity, Vec2::new(1.0, 2.0));
        assert_eq!(init.mass, 42.0);
        assert_eq!(init.radius, 9.0);
    }

    /// Regressione: i corpi caricati da un livello JSON (persistence.rs) NON
    /// hanno TrajectoryHistory; il reset deve comunque ripristinare il loro
    /// stato (prima venivano esclusi dalla query e non venivano ripristinati).
    #[test]
    fn reset_works_for_bodies_without_trajectory_history() {
        let mut app = test_app();
        let entity = {
            let mut world = app.world_mut();
            let e = world
                .spawn((
                    CelestialBody {
                        name: "Loaded".into(),
                        body_type: BodyType::Planet,
                        mass: 77.0,
                        radius: 11.0,
                        color: [0.5, 0.5, 0.5],
                        luminous: false,
                    },
                    Transform::from_xyz(400.0, -100.0, 0.0),
                    RigidBody::Dynamic,
                    Collider::circle(11.0),
                    Mass(77.0),
                    LinearVelocity(Vec2::new(5.0, 6.0)),
                    InitialBodyState {
                        position: Vec2::new(400.0, -100.0),
                        velocity: Vec2::new(5.0, 6.0),
                        mass: 77.0,
                        radius: 11.0,
                    },
                    // Volutamente SENZA TrajectoryHistory
                ))
                .id();
            world.flush();
            e
        };

        // Rovina lo stato
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Transform>()
            .unwrap()
            .translation = Vec3::new(1.0, 2.0, 0.0);
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<LinearVelocity>()
            .unwrap()
            .0 = Vec2::new(9.0, 9.0);

        app.world_mut()
            .resource_mut::<Messages<ResetMessage>>()
            .write(ResetMessage);
        run_updates(&mut app, 2);

        let world = app.world();
        assert_eq!(
            world.entity(entity).get::<Transform>().unwrap().translation.truncate(),
            Vec2::new(400.0, -100.0),
            "body without TrajectoryHistory must still be reset"
        );
        assert_eq!(
            world.entity(entity).get::<LinearVelocity>().unwrap().0,
            Vec2::new(5.0, 6.0)
        );
    }
}
