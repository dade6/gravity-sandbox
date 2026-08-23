use avian2d::prelude::*;
use bevy::prelude::*;

use crate::components::celestial::{BodyType, CelestialBody};
use crate::components::initial_state::InitialBodyState;
use crate::components::trajectory::TrajectoryHistory;
use crate::systems::camera::MainCamera;
use crate::systems::selection::SelectedBody;
use crate::systems::timeline::SimulationState;

/// Marker component for toolbar buttons.
/// Defined here so it's available to both the native UI (ui.rs) and the
/// tool system (sync_tool_buttons) without pulling the entire native UI
/// module into WASM builds.
#[derive(Component)]
pub struct ToolBtn(pub &'static str);

/// Strumento attivo
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Tool {
    #[default]
    Select,
    Add,
    Move,
    Delete,
}

/// Risorsa: strumento correntemente selezionato
#[derive(Resource, Default)]
pub struct CurrentTool(pub Tool);

/// Drag state per Move tool
#[derive(Default, Resource)]
pub struct MoveDragState {
    pub active: bool,
    pub entity: Option<Entity>,
    /// Offset dal centro del corpo al click iniziale
    pub offset: Vec2,
    /// Original alpha value to restore on release
    pub original_alpha: f32,
    /// Cursore (pixel schermo) al momento del press — usato per la soglia di drag
    pub press_cursor: Option<Vec2>,
    /// Timestamp (Time<Real>) del press — usato per la soglia temporale
    pub press_start: Option<f64>,
    /// True quando il drag ha superato la soglia di movimento (engaged)
    pub engaged: bool,
}

/// Risorsa: corpo in attesa di conferma cancellazione
#[derive(Default, Resource)]
pub struct PendingDelete(pub Option<Entity>);

/// Plugin per la gestione degli strumenti
pub struct ToolPlugin;

impl Plugin for ToolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentTool>()
            .init_resource::<MoveDragState>()
            .init_resource::<PendingDelete>()
            .add_systems(Update, (
                handle_tool_shortcuts,
                add_tool_system,
                move_tool_system,
                delete_tool_system,
            ));
    }
}

/// Cambia tool con shortcut 1-4
fn handle_tool_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut current: ResMut<CurrentTool>,
    sim_state: Res<SimulationState>,
    input_focus: Res<bevy::input_focus::InputFocus>,
) {
    crate::mark_system("handle_tool_shortcuts");
    // Un campo di testo è attivo: i tasti digitati vanno al campo, NON alle
    // shortcut (es. digitare "2" in Mass non deve attivare lo strumento Add)
    if input_focus.get().is_some() {
        return;
    }

    let new_tool = if keys.just_pressed(KeyCode::Digit1) { Some(Tool::Select) }
    else if keys.just_pressed(KeyCode::Digit2) { Some(Tool::Add) }
    else if keys.just_pressed(KeyCode::Digit3) { Some(Tool::Move) }
    else if keys.just_pressed(KeyCode::Digit4) { Some(Tool::Delete) }
    else { None };

    if let Some(tool) = new_tool {
        // Only allow switching to Add/Move/Delete when paused (playing)
        // Otherwise revert to Select for clean UX
        match tool {
            Tool::Select => current.0 = Tool::Select,
            Tool::Add | Tool::Move | Tool::Delete => {
                if sim_state.paused {
                    current.0 = tool;
                } else {
                    current.0 = Tool::Select;
                }
            }
        }
    }
}

// ============================================================
// Utility: hit test e conversione coordinate
// ============================================================

const CLICK_RADIUS: f32 = 5.0;

/// Soglia minima di movimento (pixel schermo) prima che il drag agganci il corpo.
/// Sotto questa soglia un click su un corpo in Move non sposta nulla.
const DRAG_THRESHOLD_PX: f32 = 5.0;
/// Durata minima di pressione (in secondi) perché il drag parta:
/// un click rapido (< DRAG_HOLD_SECS) seleziona, non sposta.
const DRAG_HOLD_SECS: f64 = 0.15;

/// Trova il corpo più vicino sotto il punto indicato (coordinate mondo)
fn hit_test_body(
    world_pos: Vec2,
    bodies: &Query<(Entity, &GlobalTransform, &CelestialBody)>,
) -> Option<Entity> {
    let mut closest: Option<(Entity, f32)> = None;
    for (entity, transform, body) in bodies.iter() {
        let body_pos = transform.translation().truncate();
        let distance = world_pos.distance(body_pos);
        let threshold = body.radius + CLICK_RADIUS;
        if distance < threshold {
            match closest {
                Some((_, d)) if distance < d => closest = Some((entity, distance)),
                None => closest = Some((entity, distance)),
                _ => {}
            }
        }
    }
    closest.map(|(e, _)| e)
}

/// Converte posizione cursore a coordinate mondo
fn cursor_to_world(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform), (With<Camera2d>, With<MainCamera>)>,
) -> Option<Vec2> {
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let (camera, camera_transform) = camera_query.single().ok()?;
    camera.viewport_to_world_2d(camera_transform, cursor).ok()
}

// ============================================================
// Add tool: click canvas → spawn nuovo corpo
// ============================================================

fn add_tool_system(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera2d>, With<MainCamera>)>,
    bodies: Query<(Entity, &GlobalTransform, &CelestialBody)>,
    current_tool: Res<CurrentTool>,
    sim_state: Res<SimulationState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut light_materials: ResMut<Assets<ColorMaterial>>,
    mut selected: ResMut<SelectedBody>,
) {
    crate::mark_system("add_tool_system");

    if current_tool.0 != Tool::Add || !sim_state.paused {
        return;
    }
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let world_pos = match cursor_to_world(&windows, &camera_query) {
        Some(p) => p,
        None => return,
    };
    // Non spawnare sopra a un corpo esistente
    if hit_test_body(world_pos, &bodies).is_some() {
        return;
    }

    // Spawn corpo con parametri di default
    let radius = 15.0;
    let color = [0.5, 0.5, 0.5];

    let entity = commands.spawn((
        CelestialBody {
            name: "New Body".into(),
            body_type: BodyType::Planet,
            mass: 100.0,
            radius,
            color,
            luminous: false,
        },
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(light_materials.add(ColorMaterial::from_color(
            Color::srgb(color[0], color[1], color[2]),
        ))),
        Transform::from_xyz(world_pos.x, world_pos.y, 0.0),
        RigidBody::Dynamic,
        Collider::circle(radius),
        Mass(100.0),
        LinearVelocity(Vec2::ZERO),
        ConstantForce(Vec2::ZERO),
        TrajectoryHistory::default(),
        InitialBodyState {
            position: world_pos,
            velocity: Vec2::ZERO,
            mass: 100.0,
            radius,
        },
    )).id();

    // Auto-select the newly spawned body so the property panel opens
    selected.0 = Some(entity);
}

// ============================================================
// Move tool: drag corpo → aggiorna posizione
// ============================================================

fn move_tool_system(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera2d>, With<MainCamera>)>,
    bodies: Query<(Entity, &GlobalTransform, &CelestialBody)>,
    mut transforms: Query<&mut Transform>,
    mut velocities: Query<&mut LinearVelocity>,
    current_tool: Res<CurrentTool>,
    sim_state: Res<SimulationState>,
    time: Res<Time<Real>>,
    mut selected: ResMut<SelectedBody>,
    mut drag_state: ResMut<MoveDragState>,
    mut light_materials: ResMut<Assets<ColorMaterial>>,
    material_query: Query<&MeshMaterial2d<ColorMaterial>>,
) {
    crate::mark_system("move_tool_system");

    // Se non siamo in Move+pausa, cancella eventuale drag attivo
    if current_tool.0 != Tool::Move || !sim_state.paused {
        if drag_state.active {
            close_drag(&mut drag_state, &mut velocities, &material_query, &mut light_materials);
        }
        return;
    }

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };
    // Posizione cursore in pixel schermo (per la soglia di drag)
    let cursor_px = match window.cursor_position() {
        Some(p) => p,
        None => return,
    };
    let world_pos = match cursor_to_world(&windows, &camera_query) {
        Some(p) => p,
        None => return,
    };

    if mouse_buttons.just_pressed(MouseButton::Left) {
        // Se un drag precedente è rimasto attivo (es. release persa su WASM),
        // chiudilo prima di iniziarne uno nuovo.
        if drag_state.active {
            close_drag(&mut drag_state, &mut velocities, &material_query, &mut light_materials);
        }
        // Inizia drag: cerca corpo sotto il cursore. Non sposta ancora nulla:
        // il corpo viene agganciato solo quando il cursore supera la soglia.
        if let Some(entity) = hit_test_body(world_pos, &bodies) {
            if let Ok(transform) = transforms.get(entity) {
                let body_pos = transform.translation.truncate();
                drag_state.active = true;
                drag_state.entity = Some(entity);
                drag_state.offset = body_pos - world_pos;
                drag_state.press_cursor = Some(cursor_px);
                drag_state.press_start = Some(time.elapsed_secs_f64());
                drag_state.engaged = false;

                // Salva l'alpha originale per il ripristino — ma NON ridurre
                // ancora l'opacità: il feedback 0.5 parte solo oltre soglia.
                if let Ok(mat_handle) = material_query.get(entity) {
                    if let Some(material) = light_materials.get(&mat_handle.0) {
                        let srgba = material.color.to_srgba();
                        drag_state.original_alpha = srgba.alpha;
                    }
                }
            }
        }
    } else if mouse_buttons.just_released(MouseButton::Left) {
        // Fine drag: ripristina opacità, azzera velocita'.
        // Se il drag non era engaged (click semplice, sotto soglia) → SELEZIONA
        // il corpo invece di spostarlo: click = select, drag = move.
        if drag_state.active {
            if !drag_state.engaged {
                if let Some(entity) = drag_state.entity {
                    selected.0 = Some(entity);
                }
            }
            close_drag(&mut drag_state, &mut velocities, &material_query, &mut light_materials);
        }
    } else if drag_state.active
        && mouse_buttons.pressed(MouseButton::Left)
        // Re-check tool/pausa: blocca qualunque movimento residuo con drag
        // preesistente o switch di tool avvenuto nello stesso frame.
        && current_tool.0 == Tool::Move
        && sim_state.paused
    {
        // Soglia di drag: il corpo si aggancia solo oltre DRAG_THRESHOLD_PX px
        // E dopo almeno DRAG_HOLD_SECS di pressione (click rapido = selezione)
        if !drag_state.engaged {
            if let (Some(press), Some(start)) =
                (drag_state.press_cursor, drag_state.press_start)
            {
                let moved = press.distance(cursor_px) >= DRAG_THRESHOLD_PX;
                let held = time.elapsed_secs_f64() - start >= DRAG_HOLD_SECS;
                if moved && held {
                    drag_state.engaged = true;
                    // Feedback trasparenza solo oltre soglia
                    if let Some(entity) = drag_state.entity {
                        set_alpha(entity, &material_query, &mut light_materials, 0.5);
                    }
                }
            }
        }
        if drag_state.engaged {
            // Aggiorna posizione durante il drag
            if let Some(entity) = drag_state.entity {
                if let Ok(mut transform) = transforms.get_mut(entity) {
                    transform.translation.x = world_pos.x + drag_state.offset.x;
                    transform.translation.y = world_pos.y + drag_state.offset.y;
                }
            }
        }
    } else if drag_state.active {
        // WASM: press+release nello stesso frame (o release persa) → al frame
        // successivo pressed() è già false ma active è ancora true. Chiudi il
        // drag senza spostare il corpo: se non ha superato la soglia non si è
        // mai mosso.
        close_drag(&mut drag_state, &mut velocities, &material_query, &mut light_materials);
    }
}

/// Chiude un drag attivo: ripristina l'opacità originale e azzera la velocità.
fn close_drag(
    drag_state: &mut ResMut<MoveDragState>,
    velocities: &mut Query<&mut LinearVelocity>,
    material_query: &Query<&MeshMaterial2d<ColorMaterial>>,
    light_materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    if let Some(entity) = drag_state.entity {
        if let Ok(mut vel) = velocities.get_mut(entity) {
            vel.0 = Vec2::ZERO;
        }
        restore_alpha(entity, material_query, light_materials, drag_state.original_alpha);
    }
    drag_state.active = false;
    drag_state.entity = None;
    drag_state.engaged = false;
    drag_state.press_cursor = None;
    drag_state.press_start = None;
}

/// Set the alpha of a body's material (drag transparency feedback).
/// Scrive su `base_color.a`: la pipeline passa automaticamente a blend
/// quando l'alpha scende sotto 1 (vedi `LightMaterial::alpha_mode`).
fn set_alpha(
    entity: Entity,
    material_query: &Query<&MeshMaterial2d<ColorMaterial>>,
    light_materials: &mut ResMut<Assets<ColorMaterial>>,
    alpha: f32,
) {
    if let Ok(mat_handle) = material_query.get(entity) {
        if let Some(mut material) = light_materials.get_mut(&mat_handle.0) {
            let srgba = material.color.to_srgba();
            material.color = Color::srgba(srgba.red, srgba.green, srgba.blue, alpha);
        }
    }
}

/// Restore the alpha of a body's material to a previous value.
fn restore_alpha(
    entity: Entity,
    material_query: &Query<&MeshMaterial2d<ColorMaterial>>,
    light_materials: &mut ResMut<Assets<ColorMaterial>>,
    original_alpha: f32,
) {
    set_alpha(entity, material_query, light_materials, original_alpha);
}

// ============================================================
// Delete tool: click corpo → native Bevy delete dialog
// ============================================================

fn delete_tool_system(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera2d>, With<MainCamera>)>,
    bodies: Query<(Entity, &GlobalTransform, &CelestialBody)>,
    current_tool: Res<CurrentTool>,
    sim_state: Res<SimulationState>,
    mut selected: ResMut<SelectedBody>,
    mut pending: ResMut<PendingDelete>,
) {
    crate::mark_system("delete_tool_system");

    if current_tool.0 != Tool::Delete || !sim_state.paused {
        return;
    }
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // If something is already pending delete, ignore new clicks
    if pending.0.is_some() {
        return;
    }
    let world_pos = match cursor_to_world(&windows, &camera_query) {
        Some(p) => p,
        None => return,
    };
    if let Some(entity) = hit_test_body(world_pos, &bodies) {
        // Store entity as pending — the native Bevy UI (SandboxUIPlugin)
        // handles the confirmation dialog and actual deletion.
        pending.0 = Some(entity);
        selected.0 = Some(entity);
    }
}
