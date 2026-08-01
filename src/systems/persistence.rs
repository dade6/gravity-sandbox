use avian2d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::celestial::{BodyType, CelestialBody};
use crate::systems::timeline::SimulationState;

// ============================================================
// Data structures for JSON serialization
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BodyData {
    id: usize,
    name: String,
    body_type: BodyType,
    mass: f32,
    radius: f32,
    position: [f32; 2],
    velocity: [f32; 2],
    color: [f32; 3],
    luminous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelData {
    pub name: String,
    pub gravity_constant: f32,
    pub bodies: Vec<BodyData>,
}

/// Resource holding the gravitational constant loaded from/saved to level files.
/// Defaults to the hardcoded value used by the gravity system.
#[derive(Resource)]
pub struct GravitationalConstant(pub f32);

impl Default for GravitationalConstant {
    fn default() -> Self {
        Self(5000.0)
    }
}

// ============================================================
// Plugin
// ============================================================

pub struct PersistencePlugin;

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GravitationalConstant>()
            .add_systems(Update, (
                process_load_commands,
                save_level_system,
                handle_save_load_shortcuts,
            ));
    }
}

// ============================================================
// Save system
// ============================================================

/// System that runs every frame on wasm.
/// When SAVE_REQUESTED is true, it queries all CelestialBody entities,
/// serialises them to JSON, writes the result into SAVE_RESULT, and
/// clears the request flag.
fn save_level_system(
    bodies: Query<(&CelestialBody, &GlobalTransform, &LinearVelocity)>,
    grav_constant: Res<GravitationalConstant>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        // Check if a save has been requested by the JS side
        let should_save = match crate::js_bridge::SAVE_REQUESTED.lock() {
            Ok(mut req) => {
                if !*req {
                    return;
                }
                *req = false;
                true
            }
            Err(_) => return,
        };

        if !should_save {
            return;
        }

        let bodies_data: Vec<BodyData> = bodies
            .iter()
            .enumerate()
            .map(|(i, (body, xform, vel))| {
                let pos = xform.translation().truncate();
                BodyData {
                    id: i,
                    name: body.name.clone(),
                    body_type: body.body_type,
                    mass: body.mass,
                    radius: body.radius,
                    position: [pos.x, pos.y],
                    velocity: [vel.0.x, vel.0.y],
                    color: body.color,
                    luminous: body.luminous,
                }
            })
            .collect();

        let level = LevelData {
            name: "My Level".to_string(),
            gravity_constant: grav_constant.0,
            bodies: bodies_data,
        };

        let json = serde_json::to_string(&level).unwrap_or_else(|e| {
            // On serialisation failure, store the error and return empty
            if let Ok(mut err) = crate::js_bridge::LAST_ERROR.lock() {
                *err = format!("Failed to serialise level: {}", e);
            }
            String::new()
        });

        if let Ok(mut result) = crate::js_bridge::SAVE_RESULT.lock() {
            *result = json;
        }
    }
}

// ============================================================
// Load system
// ============================================================

/// System that drains LOAD_COMMANDS (pushed by JS via load_level()).
/// For each JSON string it deserialises the level, despawns all
/// existing CelestialBody entities, spawns the new ones, pauses
/// the simulation, and updates GravitationalConstant.
fn process_load_commands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    bodies: Query<Entity, With<CelestialBody>>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut grav_constant: ResMut<GravitationalConstant>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::components::initial_state::InitialBodyState;

        // Drain the command queue (take the most recent command)
        let json = match crate::js_bridge::LOAD_COMMANDS.lock() {
            Ok(mut queue) if !queue.is_empty() => queue.pop(),
            _ => return,
        };

        let json = match json {
            Some(j) => j,
            None => return,
        };

        let level: LevelData = match serde_json::from_str(&json) {
            Ok(l) => l,
            Err(e) => {
                if let Ok(mut err) = crate::js_bridge::LAST_ERROR.lock() {
                    *err = format!("Failed to parse level JSON: {}", e);
                }
                return;
            }
        };

        // Despawn all existing CelestialBody entities
        for entity in bodies.iter() {
            commands.entity(entity).despawn();
        }

        // Pause simulation
        sim_state.paused = true;
        virtual_time.pause();

        // Update gravitational constant
        grav_constant.0 = level.gravity_constant;

        // Spawn new bodies from the level data
        for body_data in &level.bodies {
            let radius = body_data.radius;
            let color = Color::srgb(body_data.color[0], body_data.color[1], body_data.color[2]);

            commands.spawn((
                CelestialBody {
                    name: body_data.name.clone(),
                    body_type: body_data.body_type,
                    mass: body_data.mass,
                    radius,
                    color: body_data.color,
                    luminous: body_data.luminous,
                },
                Mesh2d(meshes.add(Circle::new(radius))),
                MeshMaterial2d(materials.add(ColorMaterial::from_color(color))),
                Transform::from_xyz(body_data.position[0], body_data.position[1], 0.0),
                RigidBody::Dynamic,
                Collider::circle(radius),
                Mass(body_data.mass),
                LinearVelocity(Vec2::new(body_data.velocity[0], body_data.velocity[1])),
                ConstantForce(Vec2::ZERO),
                InitialBodyState {
                    position: Vec2::new(body_data.position[0], body_data.position[1]),
                    velocity: Vec2::new(body_data.velocity[0], body_data.velocity[1]),
                    mass: body_data.mass,
                    radius,
                },
            ));
        }

        // Clear any stale error on successful load
        if let Ok(mut err) = crate::js_bridge::LAST_ERROR.lock() {
            err.clear();
        }
    }
}

// ============================================================
// Keyboard shortcuts: Ctrl+S (save) and Ctrl+O (load)
// ============================================================

/// Detects Ctrl+S (save) and Ctrl+O (load) keyboard shortcuts.
///
/// On WASM, Ctrl+S triggers a save by setting SAVE_REQUESTED (picked up
/// by save_level_system next frame). Ctrl+O sets LOAD_REQUESTED so JS
/// can show a file-open dialog.
fn handle_save_load_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    #[cfg(target_arch = "wasm32")]
    {
        // Ctrl+S → request save
        if ctrl && keys.just_pressed(KeyCode::KeyS) {
            if let Ok(mut req) = crate::js_bridge::SAVE_REQUESTED.lock() {
                *req = true;
            }
        }

        // Ctrl+O → request file-open dialog from JS
        if ctrl && keys.just_pressed(KeyCode::KeyO) {
            if let Ok(mut flag) = crate::js_bridge::LOAD_REQUESTED.lock() {
                *flag = true;
            }
        }
    }
}
