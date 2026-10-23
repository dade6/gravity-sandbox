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

/// Riquadri UI che POSSIEDONO il gesto (trascinamento e rotellina): i modali
/// scorrevoli ("Sfondo", Impostazioni) e il pannello proprietà, che è sempre
/// visibile e scorrevole quando c'è una selezione. Sopra questi riquadri la
/// camera sta ferma e il gesto scorre i parametri; fuori torna alla scena.
macro_rules! ui_scroll_boxes {
    () => {
        Query<
            (&ComputedNode, &UiGlobalTransform),
            Or<(
                With<crate::systems::starfield_panel::StarfieldBox>,
                With<crate::systems::settings::SettingsBox>,
                With<crate::systems::ui::PropertyPanel>,
            )>,
        >
    };
}

/// Il punto (px FISICI viewport-relativi) cade sopra uno dei riquadri?
fn point_over_ui_boxes<'a>(
    boxes: impl Iterator<Item = (&'a ComputedNode, &'a UiGlobalTransform)>,
    pos: Vec2,
) -> bool {
    boxes
        .into_iter()
        .any(|(n, g)| crate::systems::settings::point_in_node(n, g, pos))
}

/// Da px LOGICI (posizione dei touch) a px FISICI viewport-relativi, la
/// convenzione dei riquadri UI (`ComputedNode.size` / `UiGlobalTransform`).
fn touch_physical(
    windows: &Query<&Window>,
    camera_view: &Query<&Camera, (With<Camera2d>, With<MainCamera>)>,
    logical: Vec2,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let viewport_min = camera_view
        .iter()
        .next()
        .and_then(|cam| cam.physical_viewport_rect())
        .map(|r| r.min.as_vec2())
        .unwrap_or_default();
    Some(logical * window.scale_factor() - viewport_min)
}

/// Sistemi di controllo camera (pan/zoom/pinch) in `Update`.
///
/// Serve come ancora di ordinamento: il follow camera deve girare DOPO (lo
/// snap vince sul pan nello stesso frame) e la parallasse DOPO il follow (le
/// stelle devono usare la camera di QUESTO frame, non quella del precedente:
/// con `follow_camera` in `PostUpdate` la parallasse lavorava con un frame di
/// ritardo, e l'errore in pixel cresce con lo zoom).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraControlSystems;

/// Plugin per il controllo della camera (pan & zoom)
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanState>()
            .init_resource::<TouchPanState>()
            .add_systems(
                Update,
                (pan_camera, zoom_camera, scroll_pan, touch_pan, touch_zoom)
                    .in_set(CameraControlSystems)
                    .ambiguous_with_all(),
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
    windows: Query<&Window>,
    camera_view: Query<&Camera, (With<Camera2d>, With<MainCamera>)>,
    ui_boxes: ui_scroll_boxes!(),
) {
    crate::mark_system("scroll_pan");

    if modals.iter().next().is_some() {
        return;
    }
    // Rotellina/trackpad sopra un riquadro scorrevole (pannello proprietà o
    // modale): il gesto è del pannello, la scena non si sposta. Fuori dal
    // riquadro resta pan della scena, come prima.
    if let Some(window) = windows.iter().next() {
        let viewport_min = camera_view
            .iter()
            .next()
            .and_then(|cam| cam.physical_viewport_rect())
            .map(|r| r.min.as_vec2())
            .unwrap_or_default();
        if let Some(cursor) = window.physical_cursor_position().map(|p| p - viewport_min) {
            if point_over_ui_boxes(ui_boxes.iter(), cursor) {
                return;
            }
        }
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
    windows: Query<&Window>,
    mut camera_query: Query<(&mut Projection, &Camera), (With<Camera2d>, With<MainCamera>)>,
    // I riquadri UI che possiedono il gesto: sopra di essi la rotellina (lo
    // stesso gesto a due dita del trackpad) è del pannello, non della scena.
    ui_boxes: ui_scroll_boxes!(),
) {
    crate::mark_system("zoom_camera");

    if drag_state.active {
        return;
    }
    if mouse_buttons.pressed(MouseButton::Right) {
        return;
    }
    // Cursore sopra un pannello modale → il gesto scorre i parametri e NON
    // deve zoomare la scena (senza questa guardia si facevano entrambe le cose
    // con lo stesso movimento a due dita).
    if let Some(window) = windows.iter().next() {
        let viewport_min = camera_query
            .single()
            .ok()
            .and_then(|(_, cam)| cam.physical_viewport_rect())
            .map(|r| r.min.as_vec2())
            .unwrap_or_default();
        if let Some(cursor) = window.physical_cursor_position().map(|p| p - viewport_min) {
            if point_over_ui_boxes(ui_boxes.iter(), cursor) {
                return;
            }
        }
    }
    if let Ok((mut projection, _)) = camera_query.single_mut() {
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
    windows: Query<&Window>,
    camera_view: Query<&Camera, (With<Camera2d>, With<MainCamera>)>,
    ui_boxes: ui_scroll_boxes!(),
    // Il gesto appartiene alla camera solo se è INIZIATO fuori dai riquadri
    // scorrevoli: un dito che parte sul pannello proprietà scorre i suoi campi
    // e la scena non si muove (prima facevano entrambe le cose).
    mut started_on_ui: Local<bool>,
) {
    crate::mark_system("touch_pan");

    if touches.iter().count() == 0 {
        *started_on_ui = false;
    }
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
                *started_on_ui = touch_physical(&windows, &camera_view, touch.position())
                    .map(|p| point_over_ui_boxes(ui_boxes.iter(), p))
                    .unwrap_or(false);
                state.active = !*started_on_ui;
                state.prev_pos = touch.position();
            }
            if *started_on_ui {
                state.active = false;
                return;
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
    windows: Query<&Window>,
    camera_view: Query<&Camera, (With<Camera2d>, With<MainCamera>)>,
    ui_boxes: ui_scroll_boxes!(),
    // Pinch iniziato sopra il pannello proprietà: il gesto non è della scena.
    mut on_ui: Local<bool>,
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
                // Le DUE dita sono entrambe sopra un riquadro scorrevole?
                // Allora il pinch è del pannello, non della scena.
                *on_ui = positions.iter().all(|p| {
                    touch_physical(&windows, &camera_view, *p)
                        .map(|q| point_over_ui_boxes(ui_boxes.iter(), q))
                        .unwrap_or(false)
                });
            } else {
                let delta = dist - state.prev_dist;
                state.prev_dist = dist;
                if delta.abs() > 2.0 && !*on_ui {
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
        *on_ui = false;
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::DVec2;
    use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};

    /// App minimale con una finestra (e un cursore) e la camera: i sistemi
    /// girano DAVVERO (niente GPU) → è anche il gate B0001 per le query qui.
    fn build(with_modal_box: bool, cursor: Vec2) -> App {
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
        let mut window = Window::default();
        window.set_physical_cursor_position(Some(DVec2::new(cursor.x as f64, cursor.y as f64)));
        app.world_mut().spawn(window);
        app.world_mut().spawn((
            Camera2d,
            MainCamera,
            Camera::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        if with_modal_box {
            // Riquadro modale scorrevole centrato in (200,200), 400×400:
            // il cursore (200,200) è DENTRO, (900,900) è fuori. Porta ANCHE il
            // marker del modale: è quello che congela la camera (nell'app
            // reale il riquadro è figlio del modale).
            app.world_mut().spawn((
                crate::systems::starfield_panel::StarfieldDialog,
                crate::systems::starfield_panel::StarfieldBox,
                ComputedNode {
                    size: Vec2::new(400.0, 400.0),
                    content_size: Vec2::new(400.0, 1200.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(200.0, 200.0),
                ScrollPosition::default(),
            ));
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

    fn camera_scale(app: &mut App) -> f32 {
        let world = app.world_mut();
        match world
            .query_filtered::<&Projection, With<MainCamera>>()
            .single(world)
            .expect("camera")
        {
            Projection::Orthographic(ortho) => ortho.scale,
            other => panic!("proiezione inattesa: {other:?}"),
        }
    }

    /// Con un modale aperto la camera è FERMA: il trascinamento serve a
    /// scorrere i parametri del pannello "Sfondo", la rotellina scorre il
    /// pannello. Senza la guardia si scorreva il pannello E si spostava la
    /// scena nello stesso gesto.
    #[test]
    fn camera_is_frozen_while_a_modal_is_open() {
        // 1) Senza modale: il drag col tasto destro sposta la camera.
        let mut free = build(false, Vec2::new(900.0, 900.0));
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
        let mut modal = build(true, Vec2::new(900.0, 900.0));
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

    /// La rotellina/trackpad a due dita (Mac) sopra un riquadro modale è del
    /// PANNELLO: lo zoom della scena non deve muoversi, altrimenti scorrendo i
    /// parametri si cambia anche lo zoom. Fuori dal riquadro, invece, lo zoom
    /// deve continuare a funzionare.
    #[test]
    fn wheel_zoom_is_ignored_over_a_modal_box() {
        let mut inside = build(true, Vec2::new(200.0, 200.0));
        inside
            .world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 3.0);
        inside.update();
        assert_eq!(
            camera_scale(&mut inside),
            1.0,
            "con il cursore sopra il pannello la rotellina non deve zoomare"
        );

        let mut outside = build(true, Vec2::new(900.0, 900.0));
        outside
            .world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 3.0);
        outside.update();
        assert!(
            (camera_scale(&mut outside) - 1.0).abs() > 1e-6,
            "fuori dal pannello la rotellina deve zoomare la scena"
        );
    }

    /// Il pannello proprietà NON è un modale (convive con la scena), ma è
    /// scorrevole: sopra di lui rotellina e trascinamento appartengono al
    /// pannello. Fuori dal riquadro la camera si muove come prima — senza
    /// questa distinzione si scorrevano i campi E si spostava la scena nello
    /// stesso gesto (stessa classe di bug della v0.14.143).
    #[test]
    fn property_panel_takes_the_gesture_and_the_camera_stays_put() {
        fn add_panel(app: &mut App) {
            // Riquadro centrato in (300,300), 200×400: cursore (300,300)
            // DENTRO, (900,900) fuori. Nessun marker di modale.
            app.world_mut().spawn((
                crate::systems::ui::PropertyPanel,
                ComputedNode {
                    size: Vec2::new(200.0, 400.0),
                    content_size: Vec2::new(200.0, 1200.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(300.0, 300.0),
                ScrollPosition::default(),
            ));
        }

        let mut over = build(false, Vec2::new(300.0, 300.0));
        add_panel(&mut over);
        over.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 3.0);
        over.update();
        assert_eq!(
            camera_scale(&mut over),
            1.0,
            "sopra il pannello la rotellina non deve zoomare la scena"
        );
        assert_eq!(
            camera_x(&mut over),
            0.0,
            "sopra il pannello la rotellina non deve spostare la scena"
        );

        let mut outside = build(false, Vec2::new(900.0, 900.0));
        add_panel(&mut outside);
        outside
            .world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 3.0);
        outside.update();
        assert!(
            (camera_scale(&mut outside) - 1.0).abs() > 1e-6,
            "fuori dal pannello la rotellina deve zoomare la scena"
        );
    }
}
