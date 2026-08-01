use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::components::celestial::CelestialBody;
use crate::systems::camera::MainCamera;
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
    touches: Res<Touches>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<Camera2d>, With<MainCamera>)>,
    bodies: Query<(Entity, &GlobalTransform, &CelestialBody)>,
    ui_nodes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut selected: ResMut<SelectedBody>,
    current_tool: Res<CurrentTool>,
) {
    // Supporto mouse E touch (iPhone/iPad): la posizione del click può
    // venire dal cursore (mouse) oppure dal primo touch appena premuto.
    let mut pressed_pos: Option<Vec2> = None;
    if mouse_buttons.just_pressed(MouseButton::Left) {
        if let Ok(w) = windows.single() {
            pressed_pos = w.cursor_position();
        }
    }
    if pressed_pos.is_none() {
        if let Some(touch) = touches.iter_just_pressed().next() {
            pressed_pos = Some(touch.position());
        }
    }
    let cursor = match pressed_pos {
        Some(p) => p,
        None => return,
    };
    // Only Select tool triggers selection; editing tools handle clicks themselves
    if current_tool.0 != crate::systems::tools::Tool::Select {
        return;
    }

    let (camera, camera_transform) = match camera_query.single() {
        Ok(c) => c,
        Err(_) => return,
    };

    // Guardia UI: se il click cade su un nodo UI (toolbar, timeline,
    // property panel, dialog), NON toccare la selezione. L'input è
    // globale: senza questa guardia un click su un campo EditableText
    // del panel arriverebbe anche qui, il hit test non troverebbe alcun
    // corpo sotto il cursore e deselezionerebbe il corpo (→ il panel
    // sparirebbe nello stesso frame).
    //
    // Stessa convenzione di `ui_focus_system` (bevy_ui/focus.rs): il
    // punto va espresso in pixel fisici relativi all'origine della
    // viewport: `physical_cursor_position()` - `physical_viewport_rect().min`.
    if let Ok(window) = windows.single() {
        let viewport_min = camera
            .physical_viewport_rect()
            .map(|r| r.min.as_vec2())
            .unwrap_or_default();
        // Mouse first, touch fallback (posizione logica * scale factor).
        let ui_point = window
            .physical_cursor_position()
            .map(|p| p - viewport_min)
            .or_else(|| {
                touches
                    .iter_just_pressed()
                    .next()
                    .map(|t| t.position() * window.scale_factor() - viewport_min)
            });
        if let Some(point) = ui_point {
            if ui_point_hits_any_node(point, ui_nodes.iter()) {
                return;
            }
        }
    }

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

/// True se il punto (pixel fisici viewport-relative) cade su almeno un nodo UI.
/// Prende un iteratore (di `Query::iter()` o `QueryState::iter(world)`) così da
/// essere testabile senza un sistema: `Query` e `QueryState` producono lo stesso
/// tipo di item.
fn ui_point_hits_any_node<'a>(
    point: Vec2,
    mut ui_nodes: impl Iterator<Item = (&'a ComputedNode, &'a UiGlobalTransform)>,
) -> bool {
    ui_nodes.any(|(node, transform)| node.contains_point(*transform, point))
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ui::ResolvedBorderRadius;

    fn test_node(size: Vec2, pos: Vec2) -> (ComputedNode, UiGlobalTransform) {
        (
            ComputedNode {
                size,
                content_size: Vec2::ZERO,
                scrollbar_size: Vec2::ZERO,
                scroll_position: Vec2::ZERO,
                outline_width: 0.0,
                outline_offset: 0.0,
                unrounded_size: Vec2::ZERO,
                border: BorderRect::ZERO,
                border_radius: ResolvedBorderRadius::ZERO,
                padding: BorderRect::ZERO,
                inverse_scale_factor: 1.0,
            },
            UiGlobalTransform::from_translation(pos),
        )
    }

    fn query_with(world: &mut World, nodes: Vec<(ComputedNode, UiGlobalTransform)>) {
        for (node, transform) in nodes {
            world.spawn((node, transform));
        }
    }

    /// Un click dentro il rettangolo di un nodo UI deve essere rilevato
    /// (guardia attiva → niente deselezione).
    #[test]
    fn hit_detects_click_inside_node() {
        let mut world = World::new();
        query_with(&mut world, vec![test_node(Vec2::new(100.0, 50.0), Vec2::new(200.0, 100.0))]);
        let mut query = world.query::<(&ComputedNode, &UiGlobalTransform)>();
        let point = Vec2::new(210.0, 105.0); // dentro il nodo
        assert!(ui_point_hits_any_node(point, query.iter(&world)));
    }

    /// Un click fuori dal rettangolo del nodo NON deve essere rilevato.
    #[test]
    fn hit_ignores_click_outside_node() {
        let mut world = World::new();
        query_with(&mut world, vec![test_node(Vec2::new(100.0, 50.0), Vec2::new(200.0, 100.0))]);
        let mut query = world.query::<(&ComputedNode, &UiGlobalTransform)>();
        let point = Vec2::new(500.0, 500.0); // fuori da tutti i nodi
        assert!(!ui_point_hits_any_node(point, query.iter(&world)));
    }

    /// Un nodo nascosto (Display::None → size zero, come calcolato da Taffy)
    /// NON deve bloccare la selezione: la guardia usa contains_point che
    /// fallisce su nodi a dimensione zero.
    #[test]
    fn hit_ignores_zero_size_hidden_node() {
        let mut world = World::new();
        query_with(&mut world, vec![test_node(Vec2::ZERO, Vec2::new(200.0, 100.0))]);
        let mut query = world.query::<(&ComputedNode, &UiGlobalTransform)>();
        let point = Vec2::new(200.0, 100.0); // dove starebbe il nodo se visibile
        assert!(!ui_point_hits_any_node(point, query.iter(&world)));
    }

    /// Con più nodi, basta che uno contenga il punto.
    #[test]
    fn hit_any_node_with_multiple_nodes() {
        let mut world = World::new();
        query_with(
            &mut world,
            vec![
                test_node(Vec2::new(100.0, 50.0), Vec2::new(100.0, 100.0)),
                test_node(Vec2::new(60.0, 30.0), Vec2::new(400.0, 300.0)),
            ],
        );
        let mut query = world.query::<(&ComputedNode, &UiGlobalTransform)>();
        let point = Vec2::new(405.0, 302.0); // dentro il secondo nodo
        assert!(ui_point_hits_any_node(point, query.iter(&world)));
    }
}
