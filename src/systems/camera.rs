use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

/// Sensitivity e limiti per pan e zoom
const PAN_SPEED: f32 = 1.0;
const SCROLL_PAN_SPEED: f32 = 2.0;
const ZOOM_SPEED: f32 = 0.1;
const MIN_SCALE: f32 = 0.1;
const MAX_SCALE: f32 = 50.0;

/// Marker per la camera principale (quella controllata dall'utente)
#[derive(Component)]
pub struct MainCamera;

/// Plugin per il controllo della camera (pan & zoom)
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanState>()
            .init_resource::<TouchPanState>()
            .add_systems(Update, (pan_camera, zoom_camera, scroll_pan, touch_pan, touch_zoom));
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
    mut camera_query: Query<&mut Transform, (With<Camera2d>, With<Projection>)>,
) {
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
    mut camera_query: Query<&mut Transform, (With<Camera2d>, With<Projection>)>,
) {
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
    mut camera_query: Query<&mut Projection, With<Camera2d>>,
) {
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
    mut camera_query: Query<&mut Transform, (With<Camera2d>, With<Projection>)>,
) {
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
    mut camera_query: Query<&mut Projection, With<Camera2d>>,
) {
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
