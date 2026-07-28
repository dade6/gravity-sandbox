use bevy::prelude::*;
use avian2d::prelude::*;

use crate::components::celestial::{BodyType, CelestialBody};

/// Plugin che spawna corpi di test per il debug.
pub struct DebugSpawnPlugin;

impl Plugin for DebugSpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_test_system);
    }
}

fn spawn_test_system(
    mut commands: Commands,
) {
    // --- Sole (centro) ---
    commands.spawn((
        CelestialBody {
            body_type: BodyType::Star,
            mass: 5000.0,
            radius: 30.0,
            color: [1.0, 0.9, 0.3],
            luminous: true,
        },
        Sprite {
            color: Color::srgb(1.0, 0.9, 0.3),
            custom_size: Some(Vec2::new(60.0, 60.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(30.0),
        MassPropertiesBundle::from_shape(&Collider::circle(30.0), 5000.0),
    ));

    // --- Pianeta 1 (orbita interna) ---
    commands.spawn((
        CelestialBody {
            body_type: BodyType::Planet,
            mass: 50.0,
            radius: 12.0,
            color: [0.3, 0.6, 1.0],
            luminous: false,
        },
        Sprite {
            color: Color::srgb(0.3, 0.6, 1.0),
            custom_size: Some(Vec2::new(24.0, 24.0)),
            ..default()
        },
        Transform::from_xyz(150.0, 0.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(12.0),
        MassPropertiesBundle::from_shape(&Collider::circle(12.0), 50.0),
        LinearVelocity(Vec2::new(0.0, 120.0)),
    ));

    // --- Pianeta 2 (orbita più lenta, più grande) ---
    commands.spawn((
        CelestialBody {
            body_type: BodyType::Planet,
            mass: 200.0,
            radius: 20.0,
            color: [0.8, 0.4, 0.2],
            luminous: false,
        },
        Sprite {
            color: Color::srgb(0.8, 0.4, 0.2),
            custom_size: Some(Vec2::new(40.0, 40.0)),
            ..default()
        },
        Transform::from_xyz(250.0, 0.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(20.0),
        MassPropertiesBundle::from_shape(&Collider::circle(20.0), 200.0),
        LinearVelocity(Vec2::new(0.0, 80.0)),
    ));

    // --- Asteroide (piccolo, orbita veloce) ---
    commands.spawn((
        CelestialBody {
            body_type: BodyType::Asteroid,
            mass: 10.0,
            radius: 5.0,
            color: [0.6, 0.6, 0.6],
            luminous: false,
        },
        Sprite {
            color: Color::srgb(0.6, 0.6, 0.6),
            custom_size: Some(Vec2::new(10.0, 10.0)),
            ..default()
        },
        Transform::from_xyz(100.0, 80.0, 0.0),
        RigidBody::Dynamic,
        Collider::circle(5.0),
        MassPropertiesBundle::from_shape(&Collider::circle(5.0), 10.0),
        LinearVelocity(Vec2::new(-60.0, 100.0)),
    ));
}
