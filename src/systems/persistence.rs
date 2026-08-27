use avian2d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::celestial::{BodyType, CelestialBody};
use crate::components::lighting::{AmbientLight, GlowCurve, StarGlow, StarLightSettings};
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
    /// Per-star light params (present only for luminous bodies). Old presets
    /// without the field load with the component default.
    #[serde(default)]
    light: Option<StarLightSettings>,
    /// Per-star glow params (present only for luminous bodies).
    #[serde(default)]
    glow: Option<StarGlow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelData {
    pub name: String,
    pub gravity_constant: f32,
    pub bodies: Vec<BodyData>,
    /// Global ambient light params. Old presets load the default.
    #[serde(default)]
    pub ambient: AmbientLight,
    /// Global glow-curve params (radial falloff + soft edge).
    #[serde(default)]
    pub glow_curve: GlowCurve,
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
            .init_resource::<AmbientLight>()
            .init_resource::<GlowCurve>()
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
    bodies: Query<(
        &CelestialBody,
        &GlobalTransform,
        &LinearVelocity,
        Option<&StarLightSettings>,
        Option<&StarGlow>,
    )>,
    grav_constant: Res<GravitationalConstant>,
    ambient: Res<AmbientLight>,
    glow_curve: Res<GlowCurve>,
) {
    crate::mark_system("save_level_system");

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
            .map(|(i, (body, xform, vel, light, glow))| {
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
                    light: if body.luminous {
                        Some(light.cloned().unwrap_or_default())
                    } else {
                        None
                    },
                    glow: if body.luminous {
                        Some(glow.cloned().unwrap_or_default())
                    } else {
                        None
                    },
                }
            })
            .collect();

        let level = LevelData {
            name: "My Level".to_string(),
            gravity_constant: grav_constant.0,
            bodies: bodies_data,
            ambient: ambient.clone(),
            glow_curve: glow_curve.clone(),
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
    mut ambient: ResMut<AmbientLight>,
    mut glow_curve: ResMut<GlowCurve>,
) {
    crate::mark_system("process_load_commands");

    #[cfg(target_arch = "wasm32")]
    {
        use crate::components::initial_state::InitialBodyState;
        use crate::components::trajectory::TrajectoryHistory;

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

        // Update global ambient + glow curve resources from the preset
        *ambient = level.ambient.clone();
        *glow_curve = level.glow_curve.clone();

        // Spawn new bodies from the level data
        for body_data in &level.bodies {
            let radius = body_data.radius;
            let color = Color::srgb(body_data.color[0], body_data.color[1], body_data.color[2]);

            // Tutti i corpi usano ColorMaterial: la luce/ombre/normal map le
            // fa firefly (le mesh vengono convertite a Sprite dal bridge).
            let entity = commands
                .spawn((
                    CelestialBody {
                        name: body_data.name.clone(),
                        body_type: body_data.body_type,
                        mass: body_data.mass,
                        radius,
                        color: body_data.color,
                        luminous: body_data.luminous,
                    },
                    Mesh2d(meshes.add(Circle::new(radius))),
                    Transform::from_xyz(body_data.position[0], body_data.position[1], 0.0),
                    RigidBody::Dynamic,
                    Collider::circle(radius),
                    Mass(body_data.mass),
                    LinearVelocity(Vec2::new(body_data.velocity[0], body_data.velocity[1])),
                    ConstantForce(Vec2::ZERO),
                    TrajectoryHistory::default(),
                    InitialBodyState {
                        position: Vec2::new(body_data.position[0], body_data.position[1]),
                        velocity: Vec2::new(body_data.velocity[0], body_data.velocity[1]),
                        mass: body_data.mass,
                        radius,
                    },
                ))
                .id();

            if body_data.luminous {
                commands
                    .entity(entity)
                    .insert(MeshMaterial2d(
                        materials.add(ColorMaterial::from_color(color)),
                    ))
                    .insert(body_data.light.clone().unwrap_or_default())
                    .insert(body_data.glow.clone().unwrap_or_default());
            } else {
                commands.entity(entity).insert(MeshMaterial2d(materials.add(
                    ColorMaterial::from_color(color),
                )));
            }
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
    crate::mark_system("handle_save_load_shortcuts");

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

#[cfg(test)]
mod tests {
    use super::*;

    fn star_body() -> BodyData {
        BodyData {
            id: 0,
            name: "Star".into(),
            body_type: BodyType::Star,
            mass: 5000.0,
            radius: 30.0,
            position: [0.0, 0.0],
            velocity: [0.0, 0.0],
            color: [1.0, 0.9, 0.3],
            luminous: true,
            light: Some(StarLightSettings::default()),
            glow: Some(StarGlow::default()),
        }
    }

    #[test]
    fn serde_leveldata_roundtrip_with_new_fields() {
        let level = LevelData {
            name: "My Level".into(),
            gravity_constant: 5000.0,
            bodies: vec![
                star_body(),
                BodyData {
                    id: 1,
                    name: "Planet".into(),
                    body_type: BodyType::Planet,
                    mass: 1.0,
                    radius: 5.0,
                    position: [10.0, 0.0],
                    velocity: [0.0, 0.0],
                    color: [0.5, 0.5, 0.5],
                    luminous: false,
                    light: None,
                    glow: None,
                },
            ],
            ambient: AmbientLight {
                intensity: 0.03,
                color: [1.0, 1.0, 1.0],
                range: 0.0,
            },
            glow_curve: GlowCurve {
                falloff_exp: 2.0,
                soft_edge: 0.04,
            },
        };
        let json = serde_json::to_string(&level).unwrap();
        let back: LevelData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bodies.len(), 2);
        assert_eq!(back.bodies[0].light.as_ref().unwrap().intensity, 1.8);
        assert_eq!(back.bodies[0].glow.as_ref().unwrap().outer_scale, 25.0);
        assert!(back.bodies[1].light.is_none());
        assert_eq!(back.ambient.intensity, 0.03);
        assert_eq!(back.glow_curve.soft_edge, 0.04);
    }

    #[test]
    fn old_preset_without_new_fields_loads_defaults() {
        // Exactly the pre-19 preset.json body shape (no light/glow on bodies,
        // no level-level ambient/glow_curve).
        let old = r#"{"name":"My Level","gravity_constant":5000.0,"bodies":[{"id":0,"name":"Sun","body_type":"Star","mass":5000.0,"radius":30.0,"position":[0.0,0.0],"velocity":[0.0,0.0],"color":[1.0,0.9,0.3],"luminous":true},{"id":1,"name":"P","body_type":"Planet","mass":1.0,"radius":5.0,"position":[10.0,0.0],"velocity":[0.0,0.0],"color":[0.5,0.5,0.5],"luminous":false}]}"#;
        let l: LevelData = serde_json::from_str(old).unwrap();
        assert_eq!(l.ambient.intensity, AmbientLight::default().intensity);
        assert_eq!(l.glow_curve.soft_edge, GlowCurve::default().soft_edge);
        // Il preset vecchio non ha light/glow sui corpi -> None (a spawn il
        // componente riceve comunque i default via unwrap_or_default).
        assert!(l.bodies[0].light.is_none());
        assert!(l.bodies[1].glow.is_none());
    }
}
