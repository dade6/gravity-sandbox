//! Camera follow: aggancia la camera principale a un corpo celeste.
//!
//! UI nativa Bevy (niente overlay HTML): un bottone "Camera: ..." in alto a
//! sinistra sotto la toolbar apre un menu a tendina con "Libera" + tutti i
//! corpi. Selezionando un corpo, la camera ne segue la posizione ogni frame.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};

use crate::components::celestial::CelestialBody;
use crate::systems::camera::MainCamera;

/// Risorsa: corpo agganciato (None = camera libera).
#[derive(Default, Resource)]
pub struct CameraFollow(pub Option<Entity>);

/// Risorsa: tendina aperta/chiusa.
#[derive(Default, Resource)]
pub struct CameraFollowOpen(pub bool);

/// Marker sul container radice (per riposizionamento adattivo).
#[derive(Component)]
pub struct FollowRoot;

/// Marker sul bottone toggle "Camera: ...".
#[derive(Component)]
pub struct FollowToggle;

/// Marker sul testo del bottone toggle.
#[derive(Component)]
pub struct FollowLabel;

/// Marker sul container della tendina.
#[derive(Component)]
pub struct FollowDropdown;

/// Marker su ogni voce della tendina (None = "Libera").
#[derive(Component)]
pub struct FollowOption {
    pub target: Option<Entity>,
    pub name: String,
}

pub struct CameraFollowPlugin;

impl Plugin for CameraFollowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFollow>()
            .init_resource::<CameraFollowOpen>()
            .add_systems(Startup, spawn_camera_follow_ui)
            .add_systems(
                Update,
                (
                    toggle_follow_dropdown,
                    select_follow_option,
                    refresh_follow_options,
                    update_follow_label,
                    update_follow_dropdown_visibility,
                    adapt_follow_position,
                ),
            )
            // PostUpdate, DOPO pan/zoom (Update): lo snap vince sul pan e
            // lo zoom resta sempre agganciato (vedi follow_camera).
            .add_systems(PostUpdate, follow_camera);
    }
}

const TEXT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.75);
const BORDER_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
const PANEL_BG: Color = Color::srgba(0.08, 0.08, 0.15, 0.95);
const BTN_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const BTN_PRESS: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);

fn spawn_camera_follow_ui(mut commands: Commands) {
    crate::mark_system("spawn_camera_follow_ui");
    commands
        .spawn((
            FollowRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(60.0),
                left: Val::Px(12.0),
                // Larghezza hug-content come i bottoni toolbar (niente
                // 200px fissi): il bottone si allarga/restringe col testo.
                width: Val::Auto,
                max_width: Val::Vw(90.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|root| {
            // Bottone toggle: stesse metriche dei bottoni toolbar
            // (h 36, padding orizzontale 14, radius 8, font 14, centrato).
            root.spawn((
                Button,
                FollowToggle,
                Node {
                    height: Val::Px(36.0),
                    padding: UiRect::horizontal(Val::Px(14.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::px(8.0, 8.0, 8.0, 8.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                BorderColor::all(BORDER_COLOR),
            ))
            .with_child((
                FollowLabel,
                Text::new("Camera: Libera"),
                TextFont {
                    font: FontSource::default(),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
            // Tendina (nascosta all'inizio, popolata da refresh_follow_options)
            root.spawn((
                FollowDropdown,
                Node {
                    flex_direction: FlexDirection::Column,
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(4.0)),
                    row_gap: Val::Px(2.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::px(8.0, 8.0, 8.0, 8.0),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                BorderColor::all(BORDER_COLOR),
                GlobalZIndex(20),
            ));
        });
}

/// Click sul toggle: apre/chiude la tendina (+ feedback hover/press).
fn toggle_follow_dropdown(
    mut open: ResMut<CameraFollowOpen>,
    mut query: Query<
        (&Interaction, &mut BackgroundColor),
        (With<FollowToggle>, Changed<Interaction>),
    >,
) {
    crate::mark_system("toggle_follow_dropdown");
    for (interaction, mut bg) in query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = BTN_PRESS.into();
                open.0 = !open.0;
            }
            Interaction::Hovered => {
                *bg = BTN_HOVER.into();
            }
            Interaction::None => {
                *bg = if open.0 {
                    BTN_PRESS.into()
                } else {
                    Color::srgba(0.0, 0.0, 0.0, 0.0).into()
                };
            }
        }
    }
}

/// Click su una voce: aggancia la camera (o libera) e chiude la tendina.
fn select_follow_option(
    mut follow: ResMut<CameraFollow>,
    mut open: ResMut<CameraFollowOpen>,
    mut query: Query<(&Interaction, &FollowOption, &mut BackgroundColor), Changed<Interaction>>,
) {
    crate::mark_system("select_follow_option");
    for (interaction, option, mut bg) in query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = BTN_PRESS.into();
                follow.0 = option.target;
                open.0 = false;
            }
            Interaction::Hovered => {
                *bg = BTN_HOVER.into();
            }
            Interaction::None => {
                let is_active = follow.0 == option.target;
                *bg = if is_active {
                    BTN_PRESS.into()
                } else {
                    Color::srgba(0.0, 0.0, 0.0, 0.0).into()
                };
            }
        }
    }
}

/// Ricostruisce le voci della tendina quando i corpi cambiano
/// (spawn/despawn/rinomina). Prima voce sempre "Libera".
fn refresh_follow_options(
    mut commands: Commands,
    bodies: Query<(Entity, &CelestialBody)>,
    dropdown: Query<Entity, With<FollowDropdown>>,
    old_options: Query<Entity, With<FollowOption>>,
    mut last_snapshot: Local<Vec<(Entity, String)>>,
) {
    crate::mark_system("refresh_follow_options");
    let mut snapshot: Vec<(Entity, String)> =
        bodies.iter().map(|(e, b)| (e, b.name.clone())).collect();
    snapshot.sort_by(|a, b| a.1.cmp(&b.1));
    if *last_snapshot == snapshot {
        return;
    }
    *last_snapshot = snapshot.clone();
    let Ok(dropdown_entity) = dropdown.single() else {
        return;
    };
    for entity in old_options.iter() {
        commands.entity(entity).despawn();
    }
    commands.entity(dropdown_entity).with_children(|menu| {
        menu.spawn((
            Button,
            FollowOption {
                target: None,
                name: "Libera".to_string(),
            },
            Node {
                height: Val::Px(36.0),
                padding: UiRect::horizontal(Val::Px(14.0)),
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                border_radius: BorderRadius::px(4.0, 4.0, 4.0, 4.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        ))
        .with_child((
            Text::new("Libera"),
            TextFont {
                font: FontSource::default(),
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(TEXT_COLOR),
        ));
        for (entity, name) in &snapshot {
            menu.spawn((
                Button,
                FollowOption {
                    target: Some(*entity),
                    name: name.clone(),
                },
                Node {
                    height: Val::Px(36.0),
                    padding: UiRect::horizontal(Val::Px(14.0)),
                    align_items: AlignItems::Center,
                    width: Val::Percent(100.0),
                    border_radius: BorderRadius::px(4.0, 4.0, 4.0, 4.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            ))
            .with_child((
                Text::new(name.clone()),
                TextFont {
                    font: FontSource::default(),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
        }
    });
}

/// Etichetta del toggle: "Camera: <nome>" o "Camera: Libera".
fn update_follow_label(
    follow: Res<CameraFollow>,
    bodies: Query<&CelestialBody>,
    mut label: Query<&mut Text, With<FollowLabel>>,
) {
    crate::mark_system("update_follow_label");
    if !follow.is_changed() && !bodies.is_empty() {
        // Aggiorna anche quando un corpo viene rinominato: il check sotto
        // confronta comunque il testo, costo trascurabile (una entity).
    }
    let name = follow
        .0
        .and_then(|e| bodies.get(e).ok().map(|b| b.name.clone()))
        .unwrap_or_else(|| "Libera".to_string());
    let mut short = name.clone();
    if short.chars().count() > 14 {
        short = format!("{}..", short.chars().take(12).collect::<String>());
    }
    let expected = format!("Camera: {} v", short);
    if let Ok(mut text) = label.single_mut() {
        if text.0 != expected {
            text.0 = expected;
        }
    }
}

/// Riposizionamento adattivo: su schermi stretti la toolbar va su due
/// righe (wrap), quindi il selettore scende sotto di essa invece di
/// sovrapporsi alla seconda riga. Soglia 640px come convenzione mobile.
fn adapt_follow_position(
    windows: Query<&Window>,
    mut root: Query<&mut Node, With<FollowRoot>>,
    mut last_top: Local<f32>,
) {
    crate::mark_system("adapt_follow_position");
    let width = windows.single().map(|w| w.width()).unwrap_or(800.0);
    let expected = if width < 640.0 { 104.0 } else { 60.0 };
    if *last_top == expected {
        return;
    }
    *last_top = expected;
    if let Ok(mut node) = root.single_mut() {
        node.top = Val::Px(expected);
    }
}

/// Mostra/nasconde la tendina in base a CameraFollowOpen.
fn update_follow_dropdown_visibility(
    open: Res<CameraFollowOpen>,
    mut dropdown: Query<&mut Node, With<FollowDropdown>>,
) {
    crate::mark_system("update_follow_dropdown_visibility");
    if !open.is_changed() {
        return;
    }
    if let Ok(mut node) = dropdown.single_mut() {
        node.display = if open.0 { Display::Flex } else { Display::None };
    }
}

/// Soglia (px) oltre la quale un touch a un dito e' un drag (sgancia).
const TOUCH_UNHOOK_PX: f32 = 10.0;

/// Snap della camera sul corpo agganciato (x/y, z invariata).
/// Gira in PostUpdate, DOPO pan/zoom (Update): mentre il follow e' attivo
/// qualsiasi pan applicato in Update viene sovrascritto qui, cosi' lo zoom
/// (rotella verticale, pinch) resta sempre agganciato senza jitter da
/// ordine ambiguo dei sistemi. Solo un pan manuale VERO sgancia e torna
/// libera: in quel caso si esce SENZA snappare e il pan dell'Update resta.
fn follow_camera(
    mut follow: ResMut<CameraFollow>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    touches: Res<Touches>,
    mut touch_origin: Local<Option<Vec2>>,
    mut camera_query: Query<&mut Transform, (With<Camera2d>, With<MainCamera>)>,
    bodies: Query<&GlobalTransform>,
) {
    crate::mark_system("follow_camera");
    let Some(target) = follow.0 else {
        *touch_origin = None;
        return;
    };
    // Tasto destro + trascina = sgancia.
    let right_dragging =
        mouse_buttons.pressed(MouseButton::Right) && mouse_motion.delta.length() > 0.5;
    // Pan trackpad = sgancia, MA solo se NON e' una gesture di zoom
    // (stessa euristica di zoom_camera: verticale pura o pinch diagonale
    // restano agganciate).
    let d = scroll.delta;
    let is_zoom_gesture = (d.x == 0.0 && d.y != 0.0) || (d.x.abs() > 1.0 && d.y.abs() > 1.0);
    let trackpad_pan = (d.x != 0.0 || d.y != 0.0) && !is_zoom_gesture;
    // Drag a un dito (touch) = sgancia; il tap semplice resta agganciato.
    let mut touch_drag = false;
    if touches.iter().count() == 1 {
        if let Some(touch) = touches.iter().next() {
            if touches.just_pressed(touch.id()) {
                *touch_origin = Some(touch.position());
            } else if let Some(origin) = *touch_origin {
                if touch.position().distance(origin) > TOUCH_UNHOOK_PX {
                    touch_drag = true;
                }
            }
        }
    } else {
        *touch_origin = None;
    }
    if right_dragging || trackpad_pan || touch_drag {
        follow.0 = None;
        *touch_origin = None;
        return;
    }
    let Ok(body_pos) = bodies.get(target).map(|t| t.translation()) else {
        // Corpo cancellato: torna libera invece di restare appesa.
        follow.0 = None;
        return;
    };
    if let Ok(mut transform) = camera_query.single_mut() {
        transform.translation.x = body_pos.x;
        transform.translation.y = body_pos.y;
    }
}
