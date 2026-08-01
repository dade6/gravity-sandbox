use bevy::prelude::*;
use avian2d::prelude::*;

use crate::components::celestial::{BodyType, CelestialBody};
use crate::components::initial_state::InitialBodyState;
use crate::components::trajectory::TrajectoryHistory;

/// Plugin che spawna corpi di test per il debug.
pub struct DebugSpawnPlugin;

impl Plugin for DebugSpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_test_system);
    }
}

fn spawn_test_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // --- Sole (centro) ---
    commands.spawn((
        CelestialBody {
            name: "Sun".into(),
            body_type: BodyType::Star,
            mass: 5000.0,
            radius: 30.0,
            color: [1.0, 0.9, 0.3],
            luminous: true,
        },
        Mesh2d(meshes.add(Circle::new(30.0))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(1.0, 0.9, 0.3)))),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(30.0),
        Mass(5000.0),
        ConstantForce(Vec2::ZERO),
        TrajectoryHistory::default(),
        InitialBodyState {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            mass: 5000.0,
            radius: 30.0,
        },
    ));

    // --- Pianeta 1 (fermo a destra, test gravità) ---
    commands.spawn((
        CelestialBody {
            name: "Planet Alpha".into(),
            body_type: BodyType::Planet,
            mass: 50.0,
            radius: 12.0,
            color: [0.3, 0.6, 1.0],
            luminous: false,
        },
        Mesh2d(meshes.add(Circle::new(12.0))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(0.3, 0.6, 1.0)))),
        Transform::from_xyz(200.0, 0.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(12.0),
        Mass(50.0),
        LinearVelocity(Vec2::new(0.0, 0.0)),  // velocità zero — test gravità
        ConstantForce(Vec2::ZERO),
        TrajectoryHistory::default(),
        InitialBodyState {
            position: Vec2::new(200.0, 0.0),
            velocity: Vec2::ZERO,
            mass: 50.0,
            radius: 12.0,
        },
    ));

    // --- Pianeta 2 (orbita più lenta, più grande) ---
    commands.spawn((
        CelestialBody {
            name: "Planet Beta".into(),
            body_type: BodyType::Planet,
            mass: 200.0,
            radius: 20.0,
            color: [0.8, 0.4, 0.2],
            luminous: false,
        },
        Mesh2d(meshes.add(Circle::new(20.0))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(0.8, 0.4, 0.2)))),
        Transform::from_xyz(250.0, 0.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(20.0),
        Mass(200.0),
        LinearVelocity(Vec2::new(0.0, 80.0)),
        ConstantForce(Vec2::ZERO),
        TrajectoryHistory::default(),
        InitialBodyState {
            position: Vec2::new(250.0, 0.0),
            velocity: Vec2::new(0.0, 80.0),
            mass: 200.0,
            radius: 20.0,
        },
    ));

    // --- Asteroide (piccolo, orbita veloce) ---
    commands.spawn((
        CelestialBody {
            name: "Rocky".into(),
            body_type: BodyType::Asteroid,
            mass: 10.0,
            radius: 5.0,
            color: [0.6, 0.6, 0.6],
            luminous: false,
        },
        Mesh2d(meshes.add(Circle::new(5.0))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(0.6, 0.6, 0.6)))),
        Transform::from_xyz(100.0, 80.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(5.0),
        Mass(10.0),
        LinearVelocity(Vec2::new(-60.0, 100.0)),
        ConstantForce(Vec2::ZERO),
        TrajectoryHistory::default(),
        InitialBodyState {
            position: Vec2::new(100.0, 80.0),
            velocity: Vec2::new(-60.0, 100.0),
            mass: 10.0,
            radius: 5.0,
        },
    ));
}
