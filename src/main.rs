use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::window::{Window, WindowResolution};

use gravity_sandbox::components::debug::DebugSpawnPlugin;
use gravity_sandbox::systems::camera::CameraControllerPlugin;
use gravity_sandbox::systems::gravity;
use gravity_sandbox::systems::light::LightPlugin;
use gravity_sandbox::systems::lighting::LightingPlugin;
use gravity_sandbox::systems::minimap::MinimapPlugin;
use gravity_sandbox::systems::parallax::ParallaxPlugin;
use gravity_sandbox::systems::persistence::PersistencePlugin;
use gravity_sandbox::systems::property_editor::PropertyEditorPlugin;
use gravity_sandbox::systems::reset::ResetPlugin;
use gravity_sandbox::systems::selection::SelectionPlugin;
use gravity_sandbox::systems::timeline::TimelinePlugin;
use gravity_sandbox::systems::tools::ToolPlugin;
use gravity_sandbox::systems::trajectory::TrajectoryPlugin;

use gravity_sandbox::systems::ui::SandboxUIPlugin;
use gravity_sandbox::rendering::TexturePlugin;
use gravity_sandbox::version::VERSION;
use gravity_sandbox::GravitySandboxPlugin;


#[cfg(not(target_family = "wasm"))]
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
        .add_plugins((
            GravitySandboxPlugin, DebugSpawnPlugin, CameraControllerPlugin,
            TimelinePlugin, PersistencePlugin, PropertyEditorPlugin,
            
            SandboxUIPlugin,
            SelectionPlugin, ToolPlugin,
            ParallaxPlugin, LightPlugin, LightingPlugin, MinimapPlugin, TrajectoryPlugin, TexturePlugin,
        ))
        .add_plugins((ResetPlugin,))
        .add_systems(FixedUpdate, gravity::gravity_system)
        .run();
}

#[cfg(target_family = "wasm")]
fn main() {}
