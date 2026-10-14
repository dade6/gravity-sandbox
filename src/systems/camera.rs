use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::systems::tools::MoveDragState;

/// Sensitivity e limiti per pan e zoom
const PAN_SPEED: f32 = 1.0;
const SCROLL_PAN_SPEED: f32 = 2.0;
const ZOOM_SPEED: f32 = 0.1;
const MIN_SCALE: f32 = 0.1;
const MAX_SCALE: f32 = 50.0;

/// Marker per la camera principale (quella controllata dall'utente)
#[derive(Component)]
pub struct MainCamera;

/// Nessun modale UI fullscreen aperto? Con uno aperto la camera è FERMA:
/// dentro un modale il trascinamento serve a scorrere i parametri (pannello
/// "Sfondo") e la rotellina scorre il pannello, non la scena. Senza questa
/// guardia si scorreva il pannello E si spostava la camera nello stesso gesto.
///
/// Un alias di tipo con `Query` NON è utilizzabile come parametro di sistema
/// (servono i lifetime `<'w, 's>`): la condizione va scritta per esteso.
macro_rules! no_modal_open {
    () => {
        Query<
            (),
            Or<(
                With<crate::systems::settings::SettingsDialog>,
                With<crate::systems::starfield_panel::StarfieldDialog>,
                With<crate::systems::ui::DeleteDialog>,
            )>,
        >
    };
}

/// Plugin per il controllo della camera (pan & zoom)
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanState>()
            .init_resource::<TouchPanState>()
            .add_systems(
                Update,
                (pan_camera, zoom_camera, scroll_pan, touch_pan, touch_zoom).ambiguous_with_all(),
            );
    }
}

// === Mouse / Trackpad State ===
#[derive(Default, Resource)]
pub struct PanState {
    dragging: bool,
}

// === Touch State ===
#[derive(Default, Resource)]
pub struct TouchPanState {
    active: bool,
    prev_pos: Vec2,
}

#[derive(Default, Resource)]
struct TouchPinchState {
    active: bool,
    prev_dist: f32,
}

/// Pan con click destro + drag (mouse)
fn pan_camera(
    mut state: ResMut<PanState>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    drag_state: Res<MoveDragState>,
    mut camera_query: Query<
        &mut Transform,
        (
            (With<Camera2d>, With<MainCamera>),
            With<Projection>,
            With<MainCamera>,
        ),
    >,
    modals: no_modal_open!(),
) {
    crate::mark_system("pan_camera");

    if modals.iter().next().is_some() {
        state.dragging = false;
        return;
    }
    if drag_state.active {
        // Durante il drag di un corpo (Move tool) la camera non si muove
        state.dragging = false;
        return;
    }
    let was_dragging = state.dragging;
    state.dragging = mouse_buttons.pressed(MouseButton::Right);

    if state.dragging && was_dragging {
        if let Ok(mut transform) = camera_query.single_mut() {
            let delta = mouse_motion.delta;
            transform.translation.x -= delta.x * PAN_SPEED;
            transform.translation.y += delta.y * PAN_SPEED;
        }
    }
}

/// Pan con scroll a due dita (trackpad)
fn scroll_pan(
    scroll: Res<AccumulatedMouseScroll>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    drag_state: Res<MoveDragState>,
    mut camera_query: Query<
        &mut Transform,
        (
            (With<Camera2d>, With<MainCamera>),
            With<Projection>,
            With<MainCamera>,
        ),
    >,
    modals: no_modal_open!(),
) {
    crate::mark_system("scroll_pan");

    if modals.iter().next().is_some() {
        return;
    }
    if drag_state.active {
        return;
    }
    if mouse_buttons.pressed(MouseButton::Right) {
        return;
    }
    if let Ok(mut transform) = camera_query.single_mut() {
        let delta = scroll.delta;
        if delta.x != 0.0 || delta.y != 0.0 {
            transform.translation.x -= delta.x * SCROLL_PAN_SPEED;
            transform.translation.y += delta.y * SCROLL_PAN_SPEED;
        }
    }
}

/// Zoom con rotellina / pinch trackpad
fn zoom_camera(
    scroll: Res<AccumulatedMouseScroll>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    drag_state: Res<MoveDragState>,
    mut camera_query: Query<&mut Projection, (With<Camera2d>, With<MainCamera>)>,
) {
    crate::mark_system("zoom_camera");

    if drag_state.active {
        return;
    }
    if mouse_buttons.pressed(MouseButton::Right) {
        return;
    }
    if let Ok(mut projection) = camera_query.single_mut() {
        let delta = scroll.delta;
        if let Projection::Orthographic(ortho) = &mut *projection {
            if delta.x == 0.0 && delta.y.abs() > 0.0 {
                let factor = 1.0 - delta.y.signum() * ZOOM_SPEED;
                ortho.scale = (ortho.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
            }
            if delta.x.abs() > 1.0 && delta.y.abs() > 1.0 {
                let factor = 1.0 - delta.y.signum() * ZOOM_SPEED;
                ortho.scale = (ortho.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
            }
        }
    }
}

// ========== TOUCH SUPPORT ==========

/// Pan con un dito (touch)
fn touch_pan(
    mut state: ResMut<TouchPanState>,
    touches: Res<Touches>,
    drag_state: Res<MoveDragState>,
    mut camera_query: Query<
        &mut Transform,
        (
            (With<Camera2d>, With<MainCamera>),
            With<Projection>,
            With<MainCamera>,
        ),
    >,
    modals: no_modal_open!(),
) {
    crate::mark_system("touch_pan");

    if modals.iter().next().is_some() {
        state.active = false;
        return;
    }
    if drag_state.active {
        state.active = false;
        return;
    }
    if touches.iter().count() == 1 {
        if let Some(touch) = touches.iter().next() {
            if touches.just_pressed(touch.id()) {
                state.active = true;
                state.prev_pos = touch.position();
            }
            if state.active && touches.get_pressed(touch.id()).is_some() {
                let pos = touch.position();
                if let Ok(mut transform) = camera_query.single_mut() {
                    let delta = pos - state.prev_pos;
                    transform.translation.x -= delta.x * PAN_SPEED;
                    transform.translation.y += delta.y * PAN_SPEED;
                }
                state.prev_pos = pos;
            }
            // Check if released by seeing if touch is no longer pressed
            if touches.get_pressed(touch.id()).is_none() {
                state.active = false;
            }
        }
    } else {
        state.active = false;
    }
}

/// Zoom con due dita (pinch)
fn touch_zoom(
    touches: Res<Touches>,
    mut state: Local<TouchPinchState>,
    drag_state: Res<MoveDragState>,
    mut camera_query: Query<&mut Projection, (With<Camera2d>, With<MainCamera>)>,
    modals: no_modal_open!(),
) {
    crate::mark_system("touch_zoom");

    if modals.iter().next().is_some() {
        state.active = false;
        return;
    }
    if drag_state.active {
        state.active = false;
        return;
    }
    let count = touches.iter().count();
    if count >= 2 {
        let mut positions: Vec<Vec2> = touches.iter().map(|t| t.position()).collect();
        positions.truncate(2);
        if positions.len() == 2 {
            let dist = positions[0].distance(positions[1]);
            if !state.active {
                state.active = true;
                state.prev_dist = dist;
            } else {
                let delta = dist - state.prev_dist;
                state.prev_dist = dist;
                if delta.abs() > 2.0 {
                    if let Ok(mut projection) = camera_query.single_mut() {
                        if let Projection::Orthographic(ortho) = &mut *projection {
                            let factor = 1.0 - (delta * 0.005).clamp(-0.2, 0.2);
                            ortho.scale = (ortho.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
                        }
                    }
                }
            }
        }
    } else {
        state.active = false;
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Con un modale UI aperto la camera è FERMA: il trascinamento serve a
    /// scorrere i parametri del pannello "Sfondo" e la rotellina scorre il
    /// pannello. Senza la guardia si scorreva il pannello E si spostava la
    /// scena nello stesso gesto.
    ///
    /// Il test esegue davvero i sistemi (MinimalPlugins, niente GPU): è anche
    /// il gate B0001 per le query aggiunte qui.
    #[test]
    fn camera_is_frozen_while_a_modal_is_open() {
        fn build(with_modal: bool) -> App {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .init_resource::<Touches>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<AccumulatedMouseMotion>()
                .init_resource::<AccumulatedMouseScroll>()
                .init_resource::<MoveDragState>()
                .init_resource::<PanState>()
                .init_resource::<TouchPanState>()
                .add_systems(
                    Update,
                    (pan_camera, zoom_camera, scroll_pan, touch_pan, touch_zoom),
                );
            app.world_mut().spawn((
                Camera2d,
                MainCamera,
                Projection::Orthographic(OrthographicProjection::default_2d()),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
            if with_modal {
                // Un modale qualunque (il pannello "Sfondo" è quello nuovo).
                app.world_mut()
                    .spawn(crate::systems::starfield_panel::StarfieldDialog);
            }
            app
        }

        fn camera_x(app: &mut App) -> f32 {
            let world = app.world_mut();
            world
                .query_filtered::<&Transform, With<MainCamera>>()
                .single(world)
                .expect("camera")
                .translation
                .x
        }

        // 1) Senza modale: il drag col tasto destro sposta la camera.
        let mut free = build(false);
        free.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        free.update(); // primo frame: registra l'inizio del drag
        free.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(25.0, 0.0);
        free.update();
        assert!(
            camera_x(&mut free).abs() > 0.1,
            "senza modale la camera deve muoversi"
        );

        // 2) Con modale: nessun movimento, né da drag né da rotellina.
        let mut modal = build(true);
        modal
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        modal.update();
        modal
            .world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(25.0, 0.0);
        modal
            .world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 40.0);
        modal.update();
        modal.update();
        assert_eq!(
            camera_x(&mut modal),
            0.0,
            "con un modale aperto la camera non deve muoversi"
        );
    }
}
