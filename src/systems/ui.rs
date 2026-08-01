//! UI nativa Bevy — attiva su tutti i target (native e WASM).
//!
//! ## Vincolo progettuale
//!
//! L'interfaccia del sandbox usa **Bevy UI** (o altra UI compatibile Bevy
//! scritta in Rust). **Nessun overlay HTML**: tutta la UI è renderizzata dal
//! motore Bevy, così che l'esperienza sia identica su desktop e WASM.
//!
//! ## Safari WebGL2 — problema noto (sotto osservazione)
//!
//! In passato (v0.11.0) l'UI nativa era stata disabilitata su WASM per un
//! errore `INVALID_ENUM: framebufferTexture2D` segnalato su Safari WebGL2
//! (catena wgpu/ANGLE/Metal). Tuttavia in v0.8 la Bevy UI funzionava su
//! iPhone Safari, quindi il blocco è stato rimosso e la UI è di nuovo
//! attiva ovunque. Se il problema Safari si ripresenta, va investigato
//! insieme all'utente PRIMA di disabilitare l'UI nativa.
//!
//! ## Link utili (contesto storico)
//!
//! - Bevy #14710 — UI flickering su Android (WebGL2, stesso meccanismo)
//!   <https://github.com/bevyengine/bevy/issues/14710>
//! - Bevy #12678 — INVALID_ENUM su iOS Safari
//!   <https://github.com/bevyengine/bevy/discussions/12678>
//! - wgpu #2399 — framebuffer attachment su GL backend
//!   <https://github.com/gfx-rs/wgpu/issues/2399>

use avian2d::prelude::*;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};
use bevy::ui::widget::TextScroll;

use bevy::text::EditableText;

use crate::components::celestial::CelestialBody;
use crate::systems::reset::ResetMessage;
use crate::systems::selection::SelectedBody;
use crate::systems::timeline::{SimulationState, StepMessage};
use crate::systems::tools::{CurrentTool, PendingDelete, Tool, ToolBtn};

/// Plugin per l'interfaccia utente Bevy (solo build native/desktop).
///
/// Su WASM/WebGL2 l'UI nativa Bevy **non renderizza** correttamente su
/// Safari a causa di limitazioni nel backend wgpu/ANGLE/Metal. Il progetto
/// usa un overlay HTML (`index.html`) come alternativa cross-browser per il
/// target WASM. Vedi la doc del modulo per i dettagli.

pub struct SandboxUIPlugin;


impl Plugin for SandboxUIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_ui)
            .add_systems(Update, (
                handle_ui_buttons,
                update_property_panel,
                sync_property_input_to_body,
                update_timeline_buttons,
                manage_delete_dialog,
                handle_delete_dialog_buttons,
            ));
    }
}

// === Marker components (native-only) ===

#[derive(Component)]
pub struct TimelinBtn(pub &'static str);

/// Marker per il pannello proprietà

#[derive(Component)]
struct PropertyPanel;

/// Markers per i campi del property panel (label)

#[derive(Component)]
struct PropField(&'static str);

/// Marker per gli input editabili del property panel

#[derive(Component)]
struct PropInput(pub &'static str);

/// Marker per la velocità nella timeline

#[derive(Component)]
struct TimelinSpeed;

/// Marker per dialog di conferma cancellazione

#[derive(Component)]
struct DeleteDialog;

/// Marker per bottoni del dialog cancellazione

#[derive(Component)]
struct DeleteDialogBtn(&'static str);

// === Colori tema (solo nativo) ===

const TEXT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.75);
/// Colore testo campi proprietà quando NON si è in pausa (readonly visivo)
const TEXT_COLOR_READONLY: Color = Color::srgba(1.0, 1.0, 1.0, 0.3);
const BORDER_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
const BTN_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const BTN_PRESS: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);
/// Colore bottoni Add/Move/Delete quando la simulazione è in play (disabilitati)
const BTN_DISABLED: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);
const PANEL_BG: Color = Color::srgba(0.08, 0.08, 0.15, 0.85);
const INPUT_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.3);
const INPUT_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);
const OVERLAY_FG: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
const DIALOG_BG: Color = Color::srgba(0.12, 0.12, 0.22, 0.95);


fn spawn_ui(mut commands: Commands) {
    // === Toolbar in alto ===
    spawn_toolbar(&mut commands);

    // === Timeline in basso ===
    spawn_timeline(&mut commands);

    // === Property Panel (a destra) ===
    spawn_property_panel(&mut commands);
}


fn spawn_toolbar(commands: &mut Commands) {
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
            // Bottone Reset: ripristina lo stato iniziale senza cambiare il
            // CurrentTool (usa TimelinBtn come marker per non toccare i tool).
            bar.spawn((
                Button,
                TimelinBtn("reset"),
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
                Text::new("Reset"),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(14.0), ..default() },
                TextColor(TEXT_COLOR),
            ));
            bar.spawn(Node { flex_grow: 1.0, ..default() });
            bar.spawn((
                Text::new(format!("Sandbox v{}", crate::version::VERSION)),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(11.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.2)),
            ));
        });
}


fn spawn_timeline(commands: &mut Commands) {
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
                TimelinSpeed,
                Text::new("Speed 1.0×"),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(13.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.35)),
            ));
        });
}


fn spawn_property_panel(commands: &mut Commands) {
    commands
        .spawn((
            PropertyPanel,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(60.0),
                right: Val::Px(10.0),
                width: Val::Px(220.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(5.0),
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

            // Status line (edit mode hint)
            panel.spawn((
                PropField("_status"),
                Text::new(""),
                TextFont { font: FontSource::default(), font_size: FontSize::Px(10.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.4)),
            ));

            // Field definitions: (label, field_key, is_editable)
            let fields: &[(&str, &str, bool)] = &[
                ("Name:",  "name",   true),
                ("Type:",  "_type",  false),
                ("Mass:",  "mass",   true),
                ("Radius:","radius", true),
                ("Pos X:", "pos_x",  true),
                ("Pos Y:", "pos_y",  true),
                ("Vel X:", "vel_x",  true),
                ("Vel Y:", "vel_y",  true),
                ("Color:", "color",  true),
            ];

            for &(label, key, editable) in fields {
                // Row container
                let mut row = panel.spawn((
                    PropField(label),
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                ));

                // Label
                row.with_child((
                    Text::new(label),
                    TextFont { font: FontSource::default(), font_size: FontSize::Px(11.0), ..default() },
                    TextColor(TEXT_COLOR),
                ));

                if editable {
                    // Input container with border
                    row.with_children(|input_container| {
                        input_container.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(22.0),
                                padding: UiRect::horizontal(Val::Px(4.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::px(3.0, 3.0, 3.0, 3.0),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(INPUT_BG),
                            BorderColor::all(INPUT_BORDER),
                        ))
                        .with_child((
                            EditableText::new(""),
                            PropInput(key),
                            TextFont {
                                font: FontSource::default(),
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(TEXT_COLOR),
                            TextScroll(Vec2::ZERO),
                            // Nodes are required but added automatically via Node requirement on child? 
                            // Let's be explicit:
                            Node {
                                width: Val::Percent(100.0),
                                ..default()
                            },
                        ));
                    });
                } else {
                    // Read-only value text (for Type: field)
                    row.with_child((
                        PropField(key),
                        Text::new(""),
                        TextFont { font: FontSource::default(), font_size: FontSize::Px(11.0), ..default() },
                        TextColor(TEXT_COLOR),
                    ));
                }
            }
        });
}

// === Timeline button sync ===

/// Aggiorna testo bottoni timeline (Play↔Pause) e display velocità

fn update_timeline_buttons(
    sim_state: Res<SimulationState>,
    mut btn_query: Query<(&TimelinBtn, &Children)>,
    mut text_queries: ParamSet<(
        Query<&mut Text>,
        Query<&mut Text, (With<TimelinSpeed>, Without<TimelinBtn>)>,
    )>,
) {
    // Update Play/Pause button text
    for (btn, children) in btn_query.iter_mut() {
        if btn.0 == "play" {
            let new_label = if sim_state.paused {
                "▶ Play"
            } else {
                "⏸ Pause"
            };
            for child in children.iter() {
                if let Ok(mut text) = text_queries.p0().get_mut(child) {
                    if text.0 != new_label {
                        text.0 = new_label.to_string();
                    }
                    break;
                }
            }
        }
    }

    // Update speed display
    if sim_state.is_changed() {
        if let Ok(mut speed_text) = text_queries.p1().single_mut() {
            speed_text.0 = format!("Speed {:.1}×", sim_state.speed);
        }
    }
}

// === Property panel update ===

/// Aggiorna pannello proprietà basato su corpo selezionato

fn update_property_panel(
    selected: Res<SelectedBody>,
    bodies_query: Query<(&CelestialBody, &GlobalTransform), Without<PropertyPanel>>,
    velocity_query: Query<&LinearVelocity>,
    sim_state: Res<SimulationState>,
    mut panel_query: Query<&mut Node, With<PropertyPanel>>,
    mut editable_inputs: Query<(&PropInput, &mut EditableText, &mut TextColor)>,
    mut text_labels: Query<(&PropField, &mut Text), (Without<PropInput>, Without<EditableText>)>,
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

    // Edit hint based on pause state
    let edit_hint = if sim_state.paused {
        "✏️ Edit mode"
    } else {
        "⏸ Pause to edit"
    };

    // Update EditableText fields
    for (prop, mut editable_text, mut text_color) in editable_inputs.iter_mut() {
        // Gray-out fields when not paused (readonly visivo)
        text_color.0 = if sim_state.paused {
            TEXT_COLOR
        } else {
            TEXT_COLOR_READONLY
        };
        let current_text = editable_text.value().to_string();
        let expected = match prop.0 {
            "name" => body.name.clone(),
            "mass" => format!("{:.1}", body.mass),
            "radius" => format!("{:.1}", body.radius),
            "pos_x" => format!("{:.1}", pos.x),
            "pos_y" => format!("{:.1}", pos.y),
            "vel_x" => format!("{:.1}", vel.x),
            "vel_y" => format!("{:.1}", vel.y),
            "color" => {
                let r = (body.color[0] * 255.0) as u8;
                let g = (body.color[1] * 255.0) as u8;
                let b = (body.color[2] * 255.0) as u8;
                format!("#{:02x}{:02x}{:02x}", r, g, b)
            }
            _ => continue,
        };
        // Only update if the text differs from expected (avoids cursor jumps while editing)
        if current_text != expected {
            editable_text.editor.set_text(&expected);
        }
    }

    // Update Text labels
    for (field, mut text) in text_labels.iter_mut() {
        let value = match field.0 {
            "_status" => edit_hint.to_string(),
            "_type" => format!("{}", body_type_str(body.body_type)),
            _ => continue,
        };
        text.0 = value;
    }
}

/// Legge modifiche da EditableText e le scrive al corpo selezionato

fn sync_property_input_to_body(
    input_query: Query<(&PropInput, &EditableText)>,
    selected: Res<SelectedBody>,
    sim_state: Res<SimulationState>,
    mut bodies: Query<(
        &mut CelestialBody,
        &mut Transform,
        &mut LinearVelocity,
        &mut Mass,
    )>,
) {
    if !sim_state.paused {
        return; // Don't apply edits while playing
    }

    let entity = match selected.0 {
        Some(e) => e,
        None => return,
    };

    let Ok((mut body, mut transform, mut velocity, mut mass_component)) = bodies.get_mut(entity)
    else {
        return;
    };

    for (prop, editable) in input_query.iter() {
        let text_value = editable.value().to_string();

        match prop.0 {
            "name" => {
                if body.name != text_value {
                    body.name = text_value;
                }
            }
            "mass" => {
                if let Ok(v) = text_value.parse::<f32>() {
                    let clamped = v.max(0.1);
                    if (body.mass - clamped).abs() > 0.001 {
                        body.mass = clamped;
                        mass_component.0 = clamped;
                    }
                }
            }
            "radius" => {
                if let Ok(v) = text_value.parse::<f32>() {
                    let clamped = v.max(1.0);
                    if (body.radius - clamped).abs() > 0.001 {
                        body.radius = clamped;
                    }
                }
            }
            "pos_x" => {
                if let Ok(v) = text_value.parse::<f32>() {
                    if (transform.translation.x - v).abs() > 0.001 {
                        transform.translation.x = v;
                    }
                }
            }
            "pos_y" => {
                if let Ok(v) = text_value.parse::<f32>() {
                    if (transform.translation.y - v).abs() > 0.001 {
                        transform.translation.y = v;
                    }
                }
            }
            "vel_x" => {
                if let Ok(v) = text_value.parse::<f32>() {
                    if (velocity.x - v).abs() > 0.001 {
                        velocity.x = v;
                    }
                }
            }
            "vel_y" => {
                if let Ok(v) = text_value.parse::<f32>() {
                    if (velocity.y - v).abs() > 0.001 {
                        velocity.y = v;
                    }
                }
            }
            "color" => {
                if let Some(rgb) = parse_hex_color(&text_value) {
                    if body.color != rgb {
                        body.color = rgb;
                    }
                }
            }
            _ => {}
        }
    }
}

// === Delete Dialog (native Bevy UI) ===

/// Gestisce spawn/despawn del dialog di cancellazione

fn manage_delete_dialog(
    pending: Res<PendingDelete>,
    current_tool: Res<CurrentTool>,
    dialog_query: Query<Entity, With<DeleteDialog>>,
    bodies: Query<&CelestialBody>,
    mut commands: Commands,
    windows: Query<&Window>,
) {
    let has_dialog = dialog_query.single().is_ok();

    // Always despawn dialog if pending is cleared
    if pending.0.is_none() && has_dialog {
        if let Ok(dialog_entity) = dialog_query.single() {
            commands.entity(dialog_entity).despawn();
        }
        return;
    }

    // Only spawn dialog if pending is set, no dialog exists, and we're in delete mode
    if let Some(entity) = pending.0 {
        if !has_dialog && current_tool.0 == Tool::Delete {
            let body_name = bodies
                .get(entity)
                .map(|b| b.name.clone())
                .unwrap_or_else(|_| "Body".to_string());

            let window_size = windows
                .single()
                .map(|w| Vec2::new(w.width(), w.height()))
                .unwrap_or(Vec2::new(800.0, 600.0));

            spawn_delete_dialog(&mut commands, window_size, &body_name);
        }
    }
}


fn spawn_delete_dialog(commands: &mut Commands, window_size: Vec2, body_name: &str) {
    commands
        .spawn((
            DeleteDialog,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(window_size.x),
                height: Val::Px(window_size.y),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(OVERLAY_FG),
        ))
        .with_children(|overlay| {
            // Dialog box
            overlay
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(12.0),
                        padding: UiRect::all(Val::Px(20.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::px(10.0, 10.0, 10.0, 10.0),
                        min_width: Val::Px(260.0),
                        ..default()
                    },
                    BackgroundColor(DIALOG_BG),
                    BorderColor::all(BORDER_COLOR),
                ))
                .with_children(|dialog| {
                    // Confirmation text
                    dialog.spawn((
                        Text::new(format!("Delete \"{}\"?", body_name)),
                        TextFont { font: FontSource::default(), font_size: FontSize::Px(15.0), ..default() },
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                    ));

                    // Button row
                    dialog
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|buttons| {
                            // Confirm button
                            buttons.spawn((
                                Button,
                                DeleteDialogBtn("confirm"),
                                Node {
                                    padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.8, 0.15, 0.15, 0.7)),
                                BorderColor::all(Color::srgba(1.0, 0.2, 0.2, 0.4)),
                            ))
                            .with_child((
                                Text::new("Delete"),
                                TextFont { font: FontSource::default(), font_size: FontSize::Px(13.0), ..default() },
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                            ));

                            // Cancel button
                            buttons.spawn((
                                Button,
                                DeleteDialogBtn("cancel"),
                                Node {
                                    padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.2, 0.2, 0.3, 0.6)),
                                BorderColor::all(BORDER_COLOR),
                            ))
                            .with_child((
                                Text::new("Annulla"),
                                TextFont { font: FontSource::default(), font_size: FontSize::Px(13.0), ..default() },
                                TextColor(TEXT_COLOR),
                            ));
                        });
                });
        });
}

/// Gestisce click su bottoni del dialog di cancellazione

fn handle_delete_dialog_buttons(
    mut interaction_query: Query<(&Interaction, &DeleteDialogBtn, &mut BackgroundColor)>,
    mut pending: ResMut<PendingDelete>,
    mut selected: ResMut<SelectedBody>,
    mut commands: Commands,
    dialog_query: Query<Entity, With<DeleteDialog>>,
) {
    let mut action: Option<&'static str> = None;

    for (interaction, btn, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = BTN_PRESS.into();
                action = Some(btn.0);
            }
            Interaction::Hovered => { *bg = BTN_HOVER.into(); }
            Interaction::None => {
                // Reset to default colors
                *bg = match btn.0 {
                    "confirm" => Color::srgba(0.8, 0.15, 0.15, 0.7).into(),
                    "cancel" => Color::srgba(0.2, 0.2, 0.3, 0.6).into(),
                    _ => Color::srgba(0.0, 0.0, 0.0, 0.0).into(),
                };
            }
        }
    }

    match action {
        Some("confirm") => {
            if let Some(entity) = pending.0.take() {
                if selected.0 == Some(entity) {
                    selected.0 = None;
                }
                commands.entity(entity).despawn();
            }
            // Despawn dialog
            if let Ok(dialog_entity) = dialog_query.single() {
                commands.entity(dialog_entity).despawn();
            }
        }
        Some("cancel") => {
            pending.0 = None;
            selected.0 = None;
            // Despawn dialog
            if let Ok(dialog_entity) = dialog_query.single() {
                commands.entity(dialog_entity).despawn();
            }
        }
        _ => {}
    }
}

// === Gestione unificata click e hover ===


fn handle_ui_buttons(
    mut interaction_query: Query<(
        &Interaction,
        Option<&ToolBtn>,
        Option<&TimelinBtn>,
        &mut BackgroundColor,
    ), (Without<DeleteDialogBtn>, Without<DeleteDialog>)>,
    mut sim_state: ResMut<SimulationState>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut physics_time: ResMut<Time<Physics>>,
    mut current_tool: ResMut<CurrentTool>,
    mut pending: ResMut<PendingDelete>,
    mut step_writer: MessageWriter<StepMessage>,
    mut reset_writer: MessageWriter<ResetMessage>,
) {
    for (interaction, tool, timeline, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = BTN_PRESS.into();
                if let Some(t) = timeline {
                    match t.0 {
                        "play" => {
                            sim_state.paused = !sim_state.paused;
                            if sim_state.paused {
                                virtual_time.pause();
                                physics_time.pause();
                            } else {
                                virtual_time.unpause();
                                physics_time.unpause();
                                // Clear pending delete when unpausing
                                pending.0 = None;
                            }
                        }
                        "step" => {
                            if sim_state.paused {
                                virtual_time.unpause();
                                step_writer.write(StepMessage);
                            }
                        }
                        "reset" => {
                            // Ripristina lo stato iniziale (play o pausa),
                            // senza cambiare il CurrentTool né lo stato di pausa.
                            reset_writer.write(ResetMessage);
                        }
                        _ => {}
                    }
                }
                if let Some(t) = tool {
                    // Select sempre permesso; Add/Move/Delete solo in pausa
                    let can_switch = t.0 == "Select" || sim_state.paused;
                    if can_switch {
                        match t.0 {
                            "Select" => current_tool.0 = Tool::Select,
                            "Add" => current_tool.0 = Tool::Add,
                            "Move" => current_tool.0 = Tool::Move,
                            "Delete" => current_tool.0 = Tool::Delete,
                            _ => {}
                        }
                    }
                }
            }
            Interaction::Hovered => { *bg = BTN_HOVER.into(); }
            Interaction::None => {
                if let Some(t) = tool {
                    // Mantieni l'highlight del tool attivo anche senza interazione
                    let is_active = match (t.0, &current_tool.0) {
                        ("Select", Tool::Select) => true,
                        ("Add", Tool::Add) => true,
                        ("Move", Tool::Move) => true,
                        ("Delete", Tool::Delete) => true,
                        _ => false,
                    };
                    // Add/Move/Delete sembrano disabilitati quando si è in play
                    let disabled = !sim_state.paused && t.0 != "Select";
                    *bg = if is_active {
                        BTN_PRESS.into()
                    } else if disabled {
                        BTN_DISABLED.into()
                    } else {
                        Color::srgba(0.0, 0.0, 0.0, 0.0).into()
                    };
                } else {
                    *bg = Color::srgba(0.0, 0.0, 0.0, 0.0).into();
                }
            }
        }
    }
}

// === Utility ===

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

/// Parse a hex color string like "#ff6600" into [f32; 3] RGB.
fn parse_hex_color(hex: &str) -> Option<[f32; 3]> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
        ])
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
        let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
        let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
        Some([
            (r as f32 * 17.0) / 255.0,
            (g as f32 * 17.0) / 255.0,
            (b as f32 * 17.0) / 255.0,
        ])
    } else {
        None
    }
}
