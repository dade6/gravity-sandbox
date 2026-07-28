use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::window::{Window, WindowResolution};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub mod components;
pub mod systems;
pub mod version;

use components::debug::DebugSpawnPlugin;
use systems::camera::CameraControllerPlugin;
use systems::gravity;
use systems::timeline::TimelinePlugin;

pub struct GravitySandboxPlugin;

impl Plugin for GravitySandboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup);
    }
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// WASM entry point
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn wasm_main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("Gravity Sandbox {}", version::VERSION),
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
    .init_resource::<crate::systems::camera::PanState>()
    .add_plugins((GravitySandboxPlugin, DebugSpawnPlugin, CameraControllerPlugin, TimelinePlugin))
    .add_systems(FixedUpdate, gravity::gravity_system)
    .run();
}
