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

    /// Ultimo preset.json caricato da JS (via set_preset): usato dal bottone
    /// Reset per ripristinare il livello salvato nel file senza ricompilare.
    pub static PRESET_JSON: Mutex<Option<String>> = Mutex::new(None);

    /// Flag settato dal bottone Reset (Rust) per chiedere a JS di RI-FETCHARE
    /// preset.json dal server (con cache-buster) e ricaricarlo: così le
    /// modifiche al file si vedono al Reset senza ricaricare la pagina.
    pub static PRESET_RELOAD_REQUESTED: Mutex<bool> = Mutex::new(false);

    /// Flag settato dal bottone Salva (Rust): chiede a JS di serializzare il
    /// livello corrente (save_level) e farlo scrivere su assets/preset.json
    /// sul server via POST /save-preset.
    pub static SAVE_PRESET_REQUESTED: Mutex<bool> = Mutex::new(false);

    /// Last error message from persistence operations (empty = no error)
    pub static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

    /// Flag set by the Ctrl+O keyboard shortcut to signal JS to open a file dialog
    pub static LOAD_REQUESTED: Mutex<bool> = Mutex::new(false);

    /// DEBUG: snapshot JSON dello stato interno (tool, drag, selezione, corpi)
    pub static DEBUG_STATE: Mutex<String> = Mutex::new(String::new());

    /// Tastiera mobile: testo del campo focussato inviato da JS (iOS)
    pub static TEXT_INPUT_CMD: Mutex<Option<String>> = Mutex::new(None);
    /// 0=off, 1=select-all (apertura campo), 2=replace (digitazione)
    pub static TEXT_INPUT_MODE: Mutex<u8> = Mutex::new(0);
    /// true mentre la tastiera mobile è aperta (sync/panel sospesi)
    pub static TEXT_INPUT_ACTIVE: Mutex<bool> = Mutex::new(false);
    /// Flag per clear del focus (tap fuori dal campo su iOS)
    pub static CLEAR_FOCUS_CMD: Mutex<bool> = Mutex::new(false);

    /// true su dispositivi mobili (settato da JS via set_mobile_device):
    /// abilita il keypad numerico Bevy per l'editing dei valori
    pub static MOBILE_DEVICE: Mutex<bool> = Mutex::new(false);

    /// Flight recorder: ultimo sistema eseguito + contatore frame.
    /// Se il WASM crasha con un trap (non-panic), il polling JS legge
    /// l'ultimo valore congelato e mostra DOVE si è fermato.
    pub static FLIGHT: std::sync::LazyLock<Mutex<(String, u32)>> =
        std::sync::LazyLock::new(|| Mutex::new(("boot".to_string(), 0)));

    /// Ultimo nome scritto nel DOM da mark_system (per evitare reflow a ogni frame)
    pub static FLIGHT_DOM: std::sync::LazyLock<Mutex<String>> =
        std::sync::LazyLock::new(|| Mutex::new(String::new()));
}

/// Registra il sistema in esecuzione nel flight recorder (ogni frame) e
/// scrive il nome nel DOM in modo SINCRONO: se il WASM muore con un trap
/// a metà frame, il testo resta congelato sull'ultimo sistema avviato.
pub fn mark_system(name: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut f) = crate::js_bridge::FLIGHT.lock() {
            f.0 = name.to_string();
            f.1 = f.1.wrapping_add(1);
        }
        if let Ok(mut last) = crate::js_bridge::FLIGHT_DOM.lock() {
            if *last != name {
                *last = name.to_string();
                use wasm_bindgen::JsCast;
                if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                    if let Some(el) = doc.get_element_by_id("debug-state") {
                        let _ = el.set_text_content(Some(name));
                    }
                }
            }
        }
    }
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

/// Salva il preset di livello (assets/preset.json) lato Rust, così il bottone
/// Reset può ricaricare lo stato salvato nel file. Chiamata da JS dopo il
/// fetch del preset all'avvio (prima o dopo load_level, non importa).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn set_preset(json: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut preset) = crate::js_bridge::PRESET_JSON.lock() {
            *preset = Some(json.to_string());
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

/// Check if the Reset button has requested a fresh fetch of preset.json.
/// Returns true once per request, then resets. JS should poll this and,
/// when true, re-fetch assets/preset.json (cache-buster) and call
/// load_level() with the fresh content — so edits to the file on the
/// server are picked up on Reset WITHOUT reloading the page.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn is_preset_reload_requested() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut flag) = crate::js_bridge::PRESET_RELOAD_REQUESTED.lock() {
            if *flag {
                *flag = false;
                return true;
            }
        }
    }
    false
}

/// Check if the Save button has requested overwriting assets/preset.json
/// on the server with the current level. Returns true once per request,
/// then resets. JS should poll this and, when true, call save_level()
/// (wait a frame, call again for the fresh JSON) then POST the JSON to
/// /save-preset, then set_preset() to refresh the in-memory copy.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn is_save_preset_requested() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut flag) = crate::js_bridge::SAVE_PRESET_REQUESTED.lock() {
            if *flag {
                *flag = false;
                return true;
            }
        }
    }
    false
}

/// WASM entry point
#[cfg(target_arch = "wasm32")]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn wasm_main() {
    #[cfg(target_arch = "wasm32")]
    {
        // Cattura i panic Rust e li mostra nel badge (diagnosi remota senza
        // console browser: l'utente testa su iPhone Safari/Mac)
        std::panic::set_hook(Box::new(|info| {
            use wasm_bindgen::JsCast;
            let msg = format!("PANIC: {}", info);
            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&msg));
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Some(el) = doc.get_element_by_id("version-badge") {
                    let cur = el.text_content().unwrap_or_default();
                    el.set_text_content(Some(&format!("{} | {}", cur, &msg[..msg.len().min(300)])));
                }
            }
        }));
    }
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
    // TEMP-DIAG: il trap "Unreachable" del primo frame su Mac/iPhone è la
    // Minimap (render-to-texture WebGL2) — CONFERMATO dalla bisettrice
    // (v0.14.24/25 verdi con minimap OFF, texture ON ok). v0.14.26:
    // Keypad RIATTIVATO, solo MinimapPlugin resta OFF (fix dedicato dopo).
    .add_plugins((
        GravitySandboxPlugin,
        TimelinePlugin,
        CameraControllerPlugin,
        SelectionPlugin,
        ToolPlugin,
        TrajectoryPlugin,
        LightingPlugin,
        LightPlugin,
        PersistencePlugin,
        rendering::TexturePlugin,
        PropertyEditorPlugin,
        DebugSpawnPlugin,
        ParallaxPlugin,
        PhysicsPlugins::default(),
    ))
    .add_plugins((SandboxUIPlugin, ResetPlugin, systems::keypad::KeypadPlugin))
    .insert_resource(Gravity::ZERO)
    .add_systems(FixedUpdate, gravity::gravity_system)
    .add_systems(Update, (debug_state_snapshot, apply_mobile_text_input, clear_focus_on_outside_press))
    .run();
    crate::mark_system("after_run");
}

/// Primo update con TUTTI i sistemi del progetto: se c'è un B0001 (query
/// conflittuali nello stesso sistema), il test panica QUI con i nomi esatti
/// (eseguire con `cargo test --features bevy/debug -- --nocapture`).
#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox_app() -> App {
        let mut app = App::new();
        app.edit_schedule(Update, |s: &mut bevy::ecs::schedule::Schedule| {
            s.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
                ambiguity_detection: bevy::ecs::schedule::LogLevel::Ignore,
                ..default()
            });
        });
        app.edit_schedule(PostUpdate, |s: &mut bevy::ecs::schedule::Schedule| {
            s.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
                ambiguity_detection: bevy::ecs::schedule::LogLevel::Ignore,
                ..default()
            });
        });
        app.edit_schedule(FixedUpdate, |s: &mut bevy::ecs::schedule::Schedule| {
            s.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
                ambiguity_detection: bevy::ecs::schedule::LogLevel::Ignore,
                ..default()
            });
        });
        app.add_plugins(
            DefaultPlugins
                .build()
                // RenderPlugin ATTIVO: necessario per init_asset::<Shader>
                // (LightMaterial). Il B0001 eventuale scatta nell'Update,
                // prima del render; senza window il render non parte.
                .disable::<bevy::winit::WinitPlugin>() // event loop: main-thread only
                .disable::<bevy::window::WindowPlugin>(),
        )
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
        .add_plugins((SandboxUIPlugin, ResetPlugin, systems::keypad::KeypadPlugin))
        .insert_resource(Gravity::ZERO)
        .add_systems(FixedUpdate, gravity::gravity_system);
        // N.B.: debug_state_snapshot/apply_mobile_text_input/clear_focus_*
        // sono wasm32-only: non esistono in native
        app
    }

    fn sandbox_app_with_window() -> App {
        let mut app = sandbox_app();
        // Senza WindowPlugin i Message di bevy_window non sono registrati e
        // manca la Window resource: li forniamo a mano per far girare anche
        // il render (su llvmpipe) e riprodurre il primo frame reale.
        app.add_message::<bevy::window::WindowResized>();
        app.add_message::<bevy::window::WindowCreated>();
        app.add_message::<bevy::window::WindowCloseRequested>();
        app.add_message::<bevy::window::WindowScaleFactorChanged>();
        app.add_message::<bevy::window::WindowFocused>();
        app.add_message::<bevy::window::CursorMoved>();
        app.add_message::<bevy::window::CursorEntered>();
        app.add_message::<bevy::window::CursorLeft>();
        app.add_message::<bevy::window::Ime>();
        app.add_message::<bevy::window::WindowEvent>();
        // In Bevy 0.19 Window è un Component: la spawno come entità
        // (con PrimaryWindow) invece di insert_resource.
        app.world_mut().spawn((
            Window {
                title: "test".into(),
                resolution: WindowResolution::new(800, 600),
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app
    }

    #[test]
    fn first_update_no_b0001() {
        let mut app = sandbox_app_with_window();
        app.update();
        app.update();
    }

    /// Compila light_material.wgsl in GLSL esattamente come fa wgpu su
    /// WebGL2 (Safari/Chrome): se naga fallisce o panica qui, è lui il
    /// colpevole del trap "Unreachable" del primo frame su Mac/iPhone.
    ///
    /// Verifica entrambe le varianti del preprocessore: con e senza
    /// VERTEX_UVS (la mesh Circle ha gli UV, ma il fallback polare deve
    /// compilare comunque).
    #[test]
    fn light_shader_compiles_to_glsl() {
        let raw = std::fs::read_to_string("assets/shaders/light_material.wgsl").expect("shader file");
        // Pre-process: Bevy risolve `#define_import_path` (rimosso),
        // `#import` (stub di VertexOutput con la vera disposizione dei
        // location) e il macro `MATERIAL_BIND_GROUP` (sostituito dal valore
        // reale 2, vedi bevy_sprite_render::material::MATERIAL_2D_BIND_GROUP_INDEX).
        let base = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with("#define_import_path"))
            .map(|l| {
                if l.trim_start().starts_with("#import") {
                    "struct VertexOutput { @builtin(position) clip_position: vec4<f32>, @location(0) world_position: vec4<f32>, @location(1) world_normal: vec3<f32>, @location(2) uv: vec2<f32> }".to_string()
                } else {
                    l.replace("@group(#{MATERIAL_BIND_GROUP})", "@group(2)")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        for (variant, src) in [
            // Senza VERTEX_UVS: usa il fallback polare (mesh senza UV)
            ("polar_fallback", preprocess_vertex_uvs(&base, false)),
            // Con VERTEX_UVS: campiona in.uv (mesh Circle con UV)
            ("uv_mesh", preprocess_vertex_uvs(&base, true)),
        ] {
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("WGSL PARSE FAILED ({variant}): {e}"));
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("WGSL VALIDATION FAILED ({variant}): {e:#?}"));
            for (name, version) in [
                ("desktop440", naga::back::glsl::Version::Desktop(440)),
                (
                    "es310_webgl2",
                    naga::back::glsl::Version::Embedded {
                        version: 310,
                        is_webgl: true,
                    },
                ),
            ] {
                let options = naga::back::glsl::Options {
                    version,
                    writer_flags: naga::back::glsl::WriterFlags::all(),
                    ..Default::default()
                };
                let pipeline_options = naga::back::glsl::PipelineOptions {
                    shader_stage: naga::ShaderStage::Fragment,
                    entry_point: "fragment".into(),
                    multiview: None,
                };
                let mut out = String::new();
                let writer_result = naga::back::glsl::Writer::new(
                    &mut out,
                    &module,
                    &info,
                    &options,
                    &pipeline_options,
                    Default::default(),
                );
                let mut writer = match writer_result {
                    Ok(w) => w,
                    Err(e) => panic!("GLSL WRITER FAILED ({variant}/{name}): {e}"),
                };
                match writer.write() {
                    Ok(_) => println!("{variant}/{name}: OK ({} bytes)", out.len()),
                    Err(e) => panic!("GLSL COMPILATION FAILED ({variant}/{name}): {e}"),
                }
            }
        }
    }

    /// Mini-preprocessore per il blocco `#ifdef VERTEX_UVS` dello shader:
    /// naga non ha preprocessore, lo risolviamo come fa bevy_shader.
    /// `use_uv = true` tiene il ramo `in.uv`, `false` il fallback polare.
    fn preprocess_vertex_uvs(src: &str, use_uv: bool) -> String {
        let mut out = Vec::new();
        let mut in_ifdef = false;
        let mut in_else = false;
        for line in src.lines() {
            let t = line.trim_start();
            if t.starts_with("#ifdef") {
                in_ifdef = true;
                in_else = false;
                continue;
            }
            if in_ifdef && t.starts_with("#else") {
                in_else = true;
                continue;
            }
            if in_ifdef && t.starts_with("#endif") {
                in_ifdef = false;
                continue;
            }
            if in_ifdef {
                let take = if in_else { !use_uv } else { use_uv };
                if take {
                    out.push(line);
                }
                continue;
            }
            out.push(line);
        }
        out.join("\n")
    }
}

/// Tastiera mobile (iOS): JS invia il testo digitato nell'input nascosto.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn set_focused_text(value: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut cmd) = crate::js_bridge::TEXT_INPUT_CMD.lock() {
            *cmd = Some(value.to_string());
        }
    }
}

/// Tastiera mobile: modalità di applicazione (0=off, 1=select-all, 2=replace)
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn set_mobile_input_mode(mode: u8) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut m) = crate::js_bridge::TEXT_INPUT_MODE.lock() {
            *m = mode;
        }
    }
}

/// Tastiera mobile: true mentre è aperta (sync/panel sospesi)
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn set_mobile_input_active(active: bool) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut a) = crate::js_bridge::TEXT_INPUT_ACTIVE.lock() {
            *a = active;
        }
    }
}

/// Tap fuori da un campo su iOS: rimuove il focus Bevy dal campo
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn clear_field_focus() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut c) = crate::js_bridge::CLEAR_FOCUS_CMD.lock() {
            *c = true;
        }
    }
}

/// JS: segnala se siamo su un dispositivo mobile (abilita il keypad numerico)
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn set_mobile_device(mobile: bool) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(mut m) = crate::js_bridge::MOBILE_DEVICE.lock() {
            *m = mobile;
        }
    }
}

/// Applica il testo inviato da JS (tastiera mobile) al campo focussato.
#[cfg(target_arch = "wasm32")]
fn apply_mobile_text_input(
    input_focus: Res<bevy::input_focus::InputFocus>,
    mut editable_query: Query<&mut bevy::text::EditableText>,
) {
    crate::mark_system("apply_mobile_text_input");
    let Some(focused) = input_focus.get() else {
        return;
    };
    let cmd = if let Ok(mut c) = crate::js_bridge::TEXT_INPUT_CMD.lock() {
        c.take()
    } else {
        return;
    };
    let Some(new_text) = cmd else { return };
    let mode = crate::js_bridge::TEXT_INPUT_MODE.lock().map(|m| *m).unwrap_or(0);
    if let Ok(mut editable) = editable_query.get_mut(focused) {
        let current = editable.value().to_string();
        if current != new_text || mode == 1 {
            editable.clear();
            editable.queue_edit(bevy::text::TextEdit::Insert(smol_str::SmolStr::new(
                &new_text,
            )));
            if mode == 1 {
                // Apertura campo: seleziona tutto (il primo tasto sostituisce)
                editable.queue_edit(bevy::text::TextEdit::SelectAll);
            }
        }
    }
}

/// Quando l'utente preme FUORI da un campo editabile (e fuori dal keypad),
/// il valore del campo attivo viene applicato al corpo e il focus Bevy viene
/// rimosso. Fonte di verità: la posizione del press + i rects reali dei campi.
/// SOLO mobile: su desktop il focus è gestito nativamente da bevy_input_focus
/// (il clear qui rubava il focus subito dopo il click sul campo su Mac).
#[cfg(target_arch = "wasm32")]
fn clear_focus_on_outside_press(
    touches: Res<Touches>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut input_focus: ResMut<bevy::input_focus::InputFocus>,
    selected: Res<crate::systems::selection::SelectedBody>,
    fields: Query<(
        &crate::systems::ui::PropInput,
        &bevy::text::EditableText,
        &bevy::ui::ComputedNode,
        &bevy::ui::UiGlobalTransform,
    )>,
    keypad_btns: Query<(
        &bevy::ui::ComputedNode,
        &bevy::ui::UiGlobalTransform,
    ), With<crate::systems::keypad::KeypadAction>>,
    mut bodies: Query<(
        &mut crate::components::celestial::CelestialBody,
        &mut Transform,
        &mut LinearVelocity,
        &mut Mass,
    )>,
    mut prev_focus: Local<Option<Entity>>,
) {
    crate::mark_system("clear_focus_on_outside_press");
    let mobile = crate::js_bridge::MOBILE_DEVICE
        .lock()
        .map(|m| *m)
        .unwrap_or(false);
    if !mobile {
        return;
    }
    // Se il focus è CAMBIATO in questo frame (il tap ha appena attivato un
    // campo, es. il keypad su iPhone), NON chiuderlo subito: la classificazione
    // dei rects potrebbe fallire e uccidere il focus appena impostato.
    if *prev_focus != input_focus.get() {
        *prev_focus = input_focus.get();
        return;
    }
    // Se il keypad è APERTO, il clear NON agisce mai: la classificazione dei
    // rects è inaffidabile su iPhone (mismatch di coordinate) e chiuderebbe
    // il keypad al primo tocco sui bottoni. Si chiude solo con OK
    // (KeypadAction::Done) o toccando un altro campo (focus cambia).
    let keypad_open = keypad_btns.iter().next().is_some();
    if keypad_open {
        return;
    }
    let pressed_pos: Option<Vec2> = if mouse_buttons.just_pressed(MouseButton::Left) {
        windows.iter().next().and_then(|w| w.cursor_position())
    } else if touches.any_just_pressed() {
        touches.iter_just_pressed().next().map(|t| t.position())
    } else {
        None
    };
    let Some(pos) = pressed_pos else { return };
    let point_in = |node: &bevy::ui::ComputedNode, gt: &bevy::ui::UiGlobalTransform| {
        let half = node.size / 2.0;
        pos.x >= gt.translation.x - half.x
            && pos.x <= gt.translation.x + half.x
            && pos.y >= gt.translation.y - half.y
            && pos.y <= gt.translation.y + half.y
    };
    let over_field = fields.iter().any(|(_, _, node, gt)| point_in(node, gt));
    let over_keypad = keypad_btns.iter().any(|(node, gt)| point_in(node, gt));
    if over_field || over_keypad {
        return;
    }
    // Tap fuori: applica il valore del campo attivo (se c'è) prima di chiudere,
    // così la digitazione del keypad non va persa.
    if let Some(f) = input_focus.get() {
        if let Ok((prop, editable, _, _)) = fields.get(f) {
            if let Some(e) = selected.0 {
                if let Ok((mut body, mut transform, mut velocity, mut mass)) = bodies.get_mut(e) {
                    let text_value = editable.value().to_string();
                    crate::systems::ui::apply_prop_value(
                        prop.0,
                        &text_value,
                        &mut body,
                        &mut transform,
                        &mut velocity,
                        &mut mass,
                    );
                }
            }
        }
    }
    *input_focus = bevy::input_focus::InputFocus::default();
}
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
    ui_nodes: Query<(
        &crate::systems::ui::PropInput,
        &bevy::ui::ComputedNode,
        &bevy::ui::UiGlobalTransform,
    )>,
    bodies: Query<(
        Entity,
        &crate::components::celestial::CelestialBody,
        &Transform,
        Option<&LinearVelocity>,
    )>,
) {
    crate::mark_system("debug_state_snapshot");
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
    let mut rects = Vec::new();
    for (_, node, gt) in ui_nodes.iter() {
        rects.push(format!(
            "[{:.0},{:.0},{:.0},{:.0}]",
            gt.translation.x - node.size.x / 2.0,
            gt.translation.y - node.size.y / 2.0,
            node.size.x,
            node.size.y
        ));
    }
    crate::mark_system("debug_state_snapshot");
    let (last_system, frame) = crate::js_bridge::FLIGHT
        .lock()
        .map(|f| (f.0.clone(), f.1))
        .unwrap_or(("none".to_string(), 0));
    let json = format!(
        r#"{{"last_system":"{}","frame":{},"tool":"{}","paused":{},"selected":{},"focus":{},"focused_text":"{}","drag_active":{},"drag_engaged":{},"field_rects":[{}],"bodies":[{}]}}"#,
        last_system,
        frame,
        tool,
        sim_state.paused,
        selected_id,
        focused_id,
        focused_text.replace('"', "\\\""),
        drag_state.active,
        drag_state.engaged,
        rects.join(","),
        parts.join(",")
    );
    if let Ok(mut shared) = crate::js_bridge::DEBUG_STATE.lock() {
        *shared = json;
    }
}
