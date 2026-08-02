use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use bevy::gizmos::prelude::{DefaultGizmoConfigGroup, GizmoConfigStore};
use bevy::window::{Window, WindowResolution};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub mod components;
pub mod rendering;
pub mod systems;
pub mod version;

use components::debug::DebugSpawnPlugin;
use systems::camera::{CameraControllerPlugin, MainCamera};
use systems::gravity;
use systems::light::LightPlugin;
use systems::lighting::LightingPlugin;
use systems::minimap::MinimapPlugin;
use systems::parallax::ParallaxPlugin;
use systems::persistence::PersistencePlugin;
use systems::property_editor::PropertyEditorPlugin;
use systems::reset::ResetPlugin;
use systems::selection::SelectionPlugin;
use systems::timeline::TimelinePlugin;
use systems::tools::ToolPlugin;
use systems::trajectory::TrajectoryPlugin;

use systems::ui::SandboxUIPlugin;

pub struct GravitySandboxPlugin;

impl Plugin for GravitySandboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup, setup_gizmo_layers));
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((Camera2d, RenderLayers::from_layers(&[0, 1]), MainCamera));
}

/// Gizmos (traiettorie, highlight selezione) su layer 1: visibili solo dalla
/// camera principale ([0,1]), NON dalla minimap (layer 0).
fn setup_gizmo_layers(mut store: ResMut<GizmoConfigStore>) {
    store
        .config_mut::<DefaultGizmoConfigGroup>()
        .0
        .render_layers = RenderLayers::layer(1);
}

// ============================================================
// JS ↔ Rust communication bridge (wasm32 only)
// ============================================================

/// Global bridge for JS ↔ Rust communication (wasm32 only).
/// Only persistence (save/load) and trajectory config remain;
/// toolbar, timeline, property panel, and delete confirm are now native Bevy UI.
#[cfg(target_arch = "wasm32")]
mod js_bridge {
    use std::sync::Mutex;

    /// Trajectory config command from JS (set by slider/toggle changes)
    pub static TRAJECTORY_CONFIG_CMD: Mutex<Option<String>> = Mutex::new(None);

    /// Trajectory config snapshot for JS polling (written by Rust systems)
    pub static TRAJECTORY_CONFIG_SNAPSHOT: std::sync::LazyLock<Mutex<String>> = std::sync::LazyLock::new(|| {
        Mutex::new(r#"{"trail_length":500,"prediction_steps":200,"trails_visible":true}"#.into())
    });

    // ---- Persistence bridge (save / load) ----

    /// Flag set by JS to request a save on the next ECS frame
    pub static SAVE_REQUESTED: Mutex<bool> = Mutex::new(false);

    /// Buffer holding the last saved level JSON, ready for JS polling
    pub static SAVE_RESULT: Mutex<String> = Mutex::new(String::new());

    /// Queue of level JSON strings pushed by JS to trigger a load
    pub static LOAD_COMMANDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// Last error message from persistence operations (empty = no error)
    pub static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

    /// Flag set by the Ctrl+O keyboard shortcut to signal JS to open a file dialog
    pub static LOAD_REQUESTED: Mutex<bool> = Mutex::new(false);

    /// DEBUG: snapshot JSON dello stato interno (tool, drag, selezione, corpi)
    pub static DEBUG_STATE: Mutex<String> = Mutex::new(String::new());
}

/// Set trajectory configuration from JavaScript (sliders, toggle).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn set_trajectory_config(config_json: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut cmd) = crate::js_bridge::TRAJECTORY_CONFIG_CMD.lock() {
            *cmd = Some(config_json.to_string());
        }
    }
}

/// Get current trajectory configuration as JSON for JavaScript polling.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn get_trajectory_config() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(cfg) = crate::js_bridge::TRAJECTORY_CONFIG_SNAPSHOT.lock() {
            return cfg.clone();
        }
    }
    r#"{"trail_length":500,"prediction_steps":200,"trails_visible":true}"#.to_string()
}

/// DEBUG: legge lo snapshot dello stato interno (tool, drag, selezione, corpi).
/// Riempito ogni frame dal sistema `debug_state_snapshot` (solo wasm32).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn debug_state() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(s) = crate::js_bridge::DEBUG_STATE.lock() {
            return s.clone();
        }
    }
    "{}".to_string()
}

// ============================================================
// Persistence: save / load level data
// ============================================================

/// Request a save of the current level. Returns the last saved JSON
/// (may be empty on first call; call again after one frame to get
/// fresh data). JS side should call, wait a frame, then call again.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn save_level() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        // Trigger a save on the next ECS frame
        if let Ok(mut req) = crate::js_bridge::SAVE_REQUESTED.lock() {
            *req = true;
        }
        // Return current buffer content
        if let Ok(result) = crate::js_bridge::SAVE_RESULT.lock() {
            return result.clone();
        }
    }
    String::new()
}

/// Queue a level JSON string to be loaded on the next ECS frame.
/// The system will despawn all current bodies and spawn new ones
/// from the level data, then pause the simulation.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn load_level(json: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut queue) = crate::js_bridge::LOAD_COMMANDS.lock() {
            queue.push(json.to_string());
        }
    }
}

/// Get the last error message from persistence operations.
/// Returns an empty string if no error occurred.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn get_last_error() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(err) = crate::js_bridge::LAST_ERROR.lock() {
            return err.clone();
        }
    }
    String::new()
}

/// Check if the Ctrl+O keyboard shortcut has requested a file-open dialog.
/// Returns true once per request, then resets. JS should poll this and
/// show a file picker when it returns true.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn is_load_requested() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut flag) = crate::js_bridge::LOAD_REQUESTED.lock() {
            if *flag {
                *flag = false;
                return true;
            }
        }
    }
    false
}

/// WASM entry point
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn wasm_main() {
    let mut app = App::new();
    // Bevy 0.19: disabilita la rilevazione ambiguità (causa B0001 su WASM con molti plugin)
    app.edit_schedule(Update, |schedule: &mut bevy::ecs::schedule::Schedule| {
        schedule.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
            ambiguity_detection: bevy::ecs::schedule::LogLevel::Ignore,
            ..default()
        });
    });
    app.edit_schedule(PostUpdate, |schedule: &mut bevy::ecs::schedule::Schedule| {
        schedule.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
            ambiguity_detection: bevy::ecs::schedule::LogLevel::Ignore,
            ..default()
        });
    });
    app.edit_schedule(FixedUpdate, |schedule: &mut bevy::ecs::schedule::Schedule| {
        schedule.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
            ambiguity_detection: bevy::ecs::schedule::LogLevel::Ignore,
            ..default()
        });
    });
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("Gravity Sandbox {}", version::VERSION),
            canvas: Some("#bevy-canvas".into()),
            fit_canvas_to_parent: true,
            resolution: WindowResolution::new(800, 600),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.0, 0.0, 0.0)))
    .add_plugins((
        GravitySandboxPlugin,
        TimelinePlugin,
        CameraControllerPlugin,
        SelectionPlugin,
        ToolPlugin,
        TrajectoryPlugin,
        LightingPlugin,
        LightPlugin,
        MinimapPlugin,
        PersistencePlugin,
        rendering::TexturePlugin,
        PropertyEditorPlugin,
        DebugSpawnPlugin,
        ParallaxPlugin,
        PhysicsPlugins::default(),
    ))
    .add_plugins((SandboxUIPlugin, ResetPlugin))
    .insert_resource(Gravity::ZERO)
    .add_systems(FixedUpdate, gravity::gravity_system)
    .add_systems(Update, debug_state_snapshot)
    .run();
}

/// DEBUG (solo WASM): scrive ogni frame lo snapshot dello stato interno in
/// `DEBUG_STATE` (leggibile da JS via `debug_state()`). Serve per diagnosticare
/// il bug "il pianeta si sposta con Select attivo".
#[cfg(target_arch = "wasm32")]
fn debug_state_snapshot(
    current_tool: Res<crate::systems::tools::CurrentTool>,
    drag_state: Res<crate::systems::tools::MoveDragState>,
    selected: Res<crate::systems::selection::SelectedBody>,
    sim_state: Res<crate::systems::timeline::SimulationState>,
    input_focus: Res<bevy::input_focus::InputFocus>,
    prop_texts: Query<(Entity, &crate::systems::ui::PropInput, &bevy::text::EditableText)>,
    bodies: Query<(
        Entity,
        &crate::components::celestial::CelestialBody,
        &Transform,
        Option<&LinearVelocity>,
    )>,
) {
    use crate::systems::tools::Tool;
    let tool = match current_tool.0 {
        Tool::Select => "Select",
        Tool::Add => "Add",
        Tool::Move => "Move",
        Tool::Delete => "Delete",
    };
    let selected_id = selected.0.map(|e| e.index().index()).unwrap_or(u32::MAX);
    let focused_id = input_focus.get().map(|e| e.index().index()).unwrap_or(u32::MAX);
    // Testo del campo focussato (per capire se la tastiera arriva)
    let mut focused_text = String::new();
    for (e, prop, editable) in prop_texts.iter() {
        if Some(e) == input_focus.get() {
            focused_text = format!("{}={}", prop.0, editable.value());
        }
    }
    let mut parts = Vec::new();
    for (e, body, tf, vel) in bodies.iter() {
        let v = vel.map(|v| v.0).unwrap_or(Vec2::ZERO);
        parts.push(format!(
            r#"{{"id":{},"name":"{}","x":{:.2},"y":{:.2},"vx":{:.2},"vy":{:.2}}}"#,
            e.index().index(),
            body.name,
            tf.translation.x,
            tf.translation.y,
            v.x,
            v.y
        ));
    }
    let json = format!(
        r#"{{"tool":"{}","paused":{},"selected":{},"focus":{},"focused_text":"{}","drag_active":{},"drag_engaged":{},"bodies":[{}]}}"#,
        tool,
        sim_state.paused,
        selected_id,
        focused_id,
        focused_text.replace('"', "\\\""),
        drag_state.active,
        drag_state.engaged,
        parts.join(",")
    );
    if let Ok(mut shared) = crate::js_bridge::DEBUG_STATE.lock() {
        *shared = json;
    }
}
