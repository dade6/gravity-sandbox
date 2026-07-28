use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

/// Sensitivity e limiti per pan e zoom
const PAN_SPEED: f32 = 1.0;
const SCROLL_PAN_SPEED: f32 = 2.0;
const ZOOM_SPEED: f32 = 0.1;
const MIN_SCALE: f32 = 0.1;
const MAX_SCALE: f32 = 50.0;

/// Plugin per il controllo della camera (pan & zoom)
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanState>()
            .add_systems(Update, (pan_camera, zoom_camera, scroll_pan));
    }
}

/// Risorsa per tracciare lo stato del drag
#[derive(Default, Resource)]
pub struct PanState {
    dragging: bool,
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

/// Pan con scroll a due dita (trackpad produce delta x + y)
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

/// Zoom con rotellina (solo delta.y) e pinch (delta.x + delta.y grandi)
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

        match &mut *projection {
            Projection::Orthographic(ortho) => {

                // Mouse wheel: solo delta.y
                if delta.x == 0.0 && delta.y.abs() > 0.0 {
                    let factor = 1.0 - delta.y.signum() * ZOOM_SPEED;
                    ortho.scale = (ortho.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
                }

                // Pinch to zoom: trackpad produce delta x + y grandi
                if delta.x.abs() > 1.0 && delta.y.abs() > 1.0 {
                    let factor = 1.0 - delta.y.signum() * ZOOM_SPEED;
                    ortho.scale = (ortho.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
                }
            }
            _ => {}
        }
    }
}
