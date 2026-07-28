use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::window::{Window, WindowResolution};

use gravity_sandbox::components::debug::DebugSpawnPlugin;
use gravity_sandbox::systems::gravity;
use gravity_sandbox::version::VERSION;
use gravity_sandbox::GravitySandboxPlugin;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("Gravity Sandbox {}", VERSION),
                    canvas: Some("#bevy-canvas".into()),
                    fit_canvas_to_parent: true,
                    resolution: WindowResolution::new(800, 600),
                    ..default()
                }),
                ..default()
            }),
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity::ZERO)
        .insert_resource(ClearColor(Color::srgb(0.0, 0.0, 0.0)))
        .add_plugins((GravitySandboxPlugin, DebugSpawnPlugin))
        .add_systems(FixedUpdate, gravity::gravity_system)
        .run();
}
