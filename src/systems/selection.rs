use bevy::prelude::*;

use crate::components::celestial::CelestialBody;
use crate::systems::tools::CurrentTool;

/// Marker per corpi selezionabili
#[derive(Component)]
pub struct Selectable;

/// Risorsa: corpo attualmente selezionato
#[derive(Default, Resource)]
pub struct SelectedBody(pub Option<Entity>);

/// Plugin selezione
pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedBody>()
            .add_systems(Update, selection_system)
            .add_systems(PostUpdate, highlight_selected);
    }
}

/// Raggio extra per il click
const CLICK_RADIUS: f32 = 5.0;

fn selection_system(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    bodies: Query<(Entity, &GlobalTransform, &CelestialBody)>,
    mut selected: ResMut<SelectedBody>,
    current_tool: Res<CurrentTool>,
) {
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // Only Select tool triggers selection; editing tools handle clicks themselves
    if current_tool.0 != crate::systems::tools::Tool::Select {
        return;
    }

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };
    let cursor = match window.cursor_position() {
        Some(p) => p,
        None => return,
    };
    let (camera, camera_transform) = match camera_query.single() {
        Ok(c) => c,
        Err(_) => return,
    };
    let world_pos = match camera.viewport_to_world_2d(camera_transform, cursor) {
        Ok(p) => p,
        Err(_) => return,
    };

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

    selected.0 = closest.map(|(e, _)| e);
}

/// Highlight visivo: cerchio bianco attorno al corpo selezionato
fn highlight_selected(
    selected: Res<SelectedBody>,
    bodies: Query<(&CelestialBody, &GlobalTransform)>,
    mut gizmos: Gizmos,
) {
    if let Some(entity) = selected.0 {
        if let Ok((body, transform)) = bodies.get(entity) {
            let pos = transform.translation().truncate();
            gizmos.circle_2d(pos, body.radius + 4.0, Color::srgba(1.0, 1.0, 1.0, 0.6));
        }
    }
}
