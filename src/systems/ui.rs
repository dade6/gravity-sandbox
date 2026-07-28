use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};

use crate::components::celestial::CelestialBody;
use crate::systems::selection::SelectedBody;
use crate::systems::timeline::SimulationState;

/// Plugin per l'interfaccia utente del sandbox
pub struct SandboxUIPlugin;

impl Plugin for SandboxUIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_ui)
            .add_systems(Update, (handle_ui_buttons, update_property_panel));
    }
}

// === Marker components ===
#[derive(Component)]
pub struct ToolBtn(pub &'static str);

#[derive(Component)]
pub struct TimelinBtn(pub &'static str);

/// Marker per il pannello proprietà
#[derive(Component)]
struct PropertyPanel;

/// Markers per i campi del property panel
#[derive(Component)]
struct PropField(&'static str);

// === Colori tema ===
const TEXT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.75);
const BORDER_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
const BTN_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const BTN_PRESS: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);
const PANEL_BG: Color = Color::srgba(0.08, 0.08, 0.15, 0.85);

fn spawn_ui(mut commands: Commands) {
    // === Toolbar in alto ===
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(52.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                column_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        ))
        .with_children(|bar| {
            for name in &["Select", "Add", "Move", "Delete"] {
                bar.spawn((
                    Button,
                    ToolBtn(name),
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
                    Text::new(*name),
                    TextFont { font: FontSource::default(), font_size: FontSize::Px(14.0), ..default() },
                    TextColor(TEXT_COLOR),
                ));
            }
            bar.spawn(Node { flex_grow: 1.0, ..default() });
            bar.spawn((
                Text::new(format!("Sandbox v{}", crate::version::VERSION)),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(11.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.2)),
            ));
        });

    // === Timeline in basso ===
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(56.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(14.0),
                padding: UiRect::horizontal(Val::Px(16.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        ))
        .with_children(|bar| {
            for (label, action) in &[("▶ Play", "play"), ("⏭ Step", "step")] {
                bar.spawn((
                    Button,
                    TimelinBtn(action),
                    Node {
                        height: Val::Px(38.0),
                        padding: UiRect::horizontal(Val::Px(16.0)),
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
                    Text::new(*label),
                    TextFont { font: FontSource::default(), font_size: FontSize::Px(14.0), ..default() },
                    TextColor(TEXT_COLOR),
                ));
            }
            bar.spawn((
                Text::new("Speed 1.0×"),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(13.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.35)),
            ));
        });

    // === Property Panel (a destra) ===
    commands
        .spawn((
            PropertyPanel,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(60.0),
                right: Val::Px(10.0),
                width: Val::Px(200.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::px(8.0, 8.0, 8.0, 8.0),
                display: Display::None,  // hidden initially
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(BORDER_COLOR),
        ))
        .with_children(|panel| {
            // Title
            panel.spawn((
                Text::new("Properties"),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(14.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
            ));

            // Field labels (values will be set by update system)
            for field_name in &["Type:", "Mass:", "Radius:", "Pos X:", "Pos Y:", "Vel X:", "Vel Y:"] {
                panel.spawn((
                    PropField(field_name),
                    Text::new(*field_name),
                    TextFont { font: FontSource::default(), font_size: FontSize::Px(11.0), ..default() },
                    TextColor(TEXT_COLOR),
                ));
            }
        });
}

/// Aggiorna pannello proprietà basato su corpo selezionato
fn update_property_panel(
    selected: Res<SelectedBody>,
    bodies_query: Query<(&CelestialBody, &GlobalTransform), Without<PropertyPanel>>,
    velocity_query: Query<&LinearVelocity>,
    sim_state: Res<SimulationState>,
    mut panel_query: Query<&mut Node, With<PropertyPanel>>,
    mut field_query: Query<(&PropField, &mut Text)>,
) {
    // Show/hide panel
    if let Ok(mut panel_node) = panel_query.single_mut() {
        if selected.0.is_some() {
            panel_node.display = Display::Flex;
        } else {
            panel_node.display = Display::None;
            return;
        }
    }

    let entity = match selected.0 {
        Some(e) => e,
        None => return,
    };

    let (body, transform) = match bodies_query.get(entity) {
        Ok(b) => b,
        Err(_) => return,
    };

    let vel = match velocity_query.get(entity) {
        Ok(v) => v.0,
        Err(_) => Vec2::ZERO,
    };

    let pos = transform.translation().truncate();

    // Edit suffix based on pause state
    let _edit_hint = if sim_state.paused { " [edit]" } else { "" };

    // Update text fields
    for (field, mut text) in field_query.iter_mut() {
        let value = match field.0 {
            "Type:" => format!("{}", body_type_str(body.body_type)),
            "Mass:" => format!("{:.1}", body.mass),
            "Radius:" => format!("{:.1}", body.radius),
            "Pos X:" => format!("{:.1}", pos.x),
            "Pos Y:" => format!("{:.1}", pos.y),
            "Vel X:" => format!("{:.1}", vel.x),
            "Vel Y:" => format!("{:.1}", vel.y),
            _ => continue,
        };
        text.0 = value;
    }
}

fn body_type_str(t: crate::components::celestial::BodyType) -> &'static str {
    use crate::components::celestial::BodyType::*;
    match t {
        Star => "Star",
        Planet => "Planet",
        Moon => "Moon",
        Asteroid => "Asteroid",
        Spaceship => "Ship",
    }
}

// === Gestione unificata click e hover ===
fn handle_ui_buttons(
    mut interaction_query: Query<(&Interaction, Option<&ToolBtn>, Option<&TimelinBtn>, &mut BackgroundColor)>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    for (interaction, tool, timeline, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = BTN_PRESS.into();
                if let Some(t) = timeline {
                    match t.0 {
                        "play" => {
                            sim_state.paused = !sim_state.paused;
                            if sim_state.paused { virtual_time.pause(); }
                            else { virtual_time.unpause(); }
                        }
                        "step" => {
                            if sim_state.paused { virtual_time.unpause(); }
                        }
                        _ => {}
                    }
                }
                if let Some(_t) = tool {
                    // Tool selection handled by tool system
                }
            }
            Interaction::Hovered => { *bg = BTN_HOVER.into(); }
            Interaction::None => { *bg = Color::srgba(0.0, 0.0, 0.0, 0.0).into(); }
        }
    }
}
