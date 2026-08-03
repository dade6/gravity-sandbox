//! Tastierino numerico in Bevy UI per l'editing dei valori numerici su
//! dispositivi touch (iPhone).
//!
//! By-passa completamente la tastiera di sistema (limite di iOS Safari: la
//! tastiera virtuale si apre solo con un input HTML): niente HTML, niente
//! focus DOM — tutto Bevy UI nativa, come da vincolo progettuale.
//!
//! Flusso: tap su un campo numerico (mass/radius/pos/vel) -> il campo si
//! attiva (InputFocus) -> appare il keypad -> i tasti modificano il testo
//! del campo via `TextEdit` (l'aggiornamento del display è live) -> il sync
//! è sospeso durante l'editing (TEXT_INPUT_ACTIVE) -> alla chiusura (OK o
//! tap fuori) il valore viene applicato al corpo.

use avian2d::prelude::{LinearVelocity, Mass};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

use crate::components::celestial::CelestialBody;
use crate::systems::selection::SelectedBody;
use crate::systems::ui::{apply_prop_value, PropInput};

/// Azione di un tasto del keypad
#[derive(Component, Clone, Copy)]
pub enum KeypadAction {
    Digit(char),
    Backspace,
    Done,
}

/// Marker del pannello keypad (radice)
#[derive(Component)]
pub struct Keypad;

/// Display del valore in editing (testo sopra i tasti)
#[derive(Component)]
pub struct KeypadDisplay;

pub struct KeypadPlugin;

impl Plugin for KeypadPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (keypad_visibility, keypad_display_sync, keypad_buttons),
        );
    }
}

/// Campi numerici editabili dal keypad (Name è testo -> escluso)
fn is_numeric_field(prop: &str) -> bool {
    matches!(prop, "mass" | "radius" | "pos_x" | "pos_y" | "vel_x" | "vel_y")
}

const KP_BG: Color = Color::srgba(0.10, 0.10, 0.20, 0.94);
const KP_BTN: Color = Color::srgba(0.22, 0.22, 0.34, 1.0);
const KP_BTN_PRESS: Color = Color::srgba(0.45, 0.55, 0.9, 1.0);
const KP_BORDER: Color = Color::srgba(0.5, 0.6, 1.0, 0.4);

/// Mostra il keypad quando un campo numerico è attivo su dispositivo mobile,
/// lo nasconde (e riattiva il sync) quando il campo si disattiva.
fn keypad_visibility(
    input_focus: Res<InputFocus>,
    fields: Query<(&PropInput, &EditableText)>,
    keypad_query: Query<Entity, With<Keypad>>,
    mut commands: Commands,
) {
    crate::mark_system("keypad_visibility");
    let mobile = crate::js_bridge::MOBILE_DEVICE
        .lock()
        .map(|m| *m)
        .unwrap_or(false);
    let mut should_show = false;
    if mobile {
        if let Some(f) = input_focus.get() {
            if let Ok((prop, _)) = fields.get(f) {
                should_show = is_numeric_field(prop.0);
            }
        }
    }
    let exists = keypad_query.iter().next().is_some();
    if should_show && !exists {
        spawn_keypad(&mut commands);
        if let Ok(mut a) = crate::js_bridge::TEXT_INPUT_ACTIVE.lock() {
            *a = true;
        }
    } else if !should_show && exists {
        for e in keypad_query.iter() {
            commands.entity(e).despawn();
        }
        if let Ok(mut a) = crate::js_bridge::TEXT_INPUT_ACTIVE.lock() {
            *a = false;
        }
    }
}

fn spawn_keypad(commands: &mut Commands) {
    commands
        .spawn((
            Keypad,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(48.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|outer| {
            outer
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::px(12.0, 12.0, 12.0, 12.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(KP_BG),
                    BorderColor::all(KP_BORDER),
                ))
                .with_children(|pad| {
                    // Display del valore in editing (live)
                    pad.spawn((
                        KeypadDisplay,
                        Text::new(""),
                        TextFont {
                            font: FontSource::default(),
                            font_size: FontSize::Px(22.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                    // Righe di tasti (testi ASCII, come da vincolo)
                    let rows: [&[KeypadAction]; 4] = [
                        &[
                            KeypadAction::Digit('7'),
                            KeypadAction::Digit('8'),
                            KeypadAction::Digit('9'),
                            KeypadAction::Backspace,
                        ],
                        &[
                            KeypadAction::Digit('4'),
                            KeypadAction::Digit('5'),
                            KeypadAction::Digit('6'),
                            KeypadAction::Digit('-'),
                        ],
                        &[
                            KeypadAction::Digit('1'),
                            KeypadAction::Digit('2'),
                            KeypadAction::Digit('3'),
                            KeypadAction::Digit('.'),
                        ],
                        &[KeypadAction::Digit('0'), KeypadAction::Done],
                    ];
                    for row in rows {
                        pad.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(6.0),
                            ..default()
                        })
                        .with_children(|r| {
                            for &action in row {
                                spawn_button(r, action);
                            }
                        });
                    }
                });
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands, action: KeypadAction) {
    let (label, wide) = match action {
        KeypadAction::Digit(c) => (c.to_string(), false),
        KeypadAction::Backspace => ("Del".to_string(), false),
        KeypadAction::Done => ("OK".to_string(), true),
    };
    let mut node = Node {
        width: Val::Px(58.0),
        height: Val::Px(58.0),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::px(8.0, 8.0, 8.0, 8.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    };
    if wide {
        node.width = Val::Px(122.0);
    }
    parent
        .spawn((
            Button,
            action,
            node,
            BackgroundColor(KP_BTN),
            BorderColor::all(KP_BORDER),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font: FontSource::default(),
                font_size: FontSize::Px(24.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

/// Il display mostra in tempo reale il testo del campo attivo.
fn keypad_display_sync(
    input_focus: Res<InputFocus>,
    fields: Query<(&PropInput, &EditableText)>,
    mut display: Query<&mut Text, With<KeypadDisplay>>,
) {
    crate::mark_system("keypad_display_sync");
    let Ok(mut t) = display.single_mut() else {
        return;
    };
    let text = input_focus
        .get()
        .and_then(|f| fields.get(f).ok())
        .map(|(_, ed)| ed.value().to_string())
        .unwrap_or_default();
    if t.0 != text {
        t.0 = text;
    }
}

/// Gestione dei tasti: digit/backspace modificano il testo del campo attivo
/// (editing "live" sul display); OK applica il valore al corpo e chiude.
fn keypad_buttons(
    buttons: Query<(&Interaction, &KeypadAction), Changed<Interaction>>,
    mut input_focus: ResMut<InputFocus>,
    selected: Res<SelectedBody>,
    mut editable_query: Query<&mut EditableText>,
    mut bodies: Query<(
        &mut CelestialBody,
        &mut Transform,
        &mut LinearVelocity,
        &mut Mass,
    )>,
    fields: Query<(&PropInput, &EditableText)>,
) {
    crate::mark_system("keypad_buttons");
    for (interaction, action) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(f) = input_focus.get() else {
            continue;
        };
        let Ok(mut editable) = editable_query.get_mut(f) else {
            continue;
        };
        match action {
            KeypadAction::Digit(c) => {
                editable.queue_edit(bevy::text::TextEdit::Insert(
                    smol_str::SmolStr::new(&c.to_string()),
                ));
            }
            KeypadAction::Backspace => {
                editable.queue_edit(bevy::text::TextEdit::Backspace);
            }
            KeypadAction::Done => {
                // Applica il valore finale al corpo selezionato (come il sync),
                // poi chiudi: clear del focus -> keypad_visibility despawna ->
                // TEXT_INPUT_ACTIVE torna false (il sync desktop riprende).
                if let Some(e) = selected.0 {
                    if let Ok((mut body, mut transform, mut velocity, mut mass)) = bodies.get_mut(e) {
                        if let Ok((prop, _)) = fields.get(f) {
                            let text_value = editable.value().to_string();
                            apply_prop_value(
                                prop.0,
                                &text_value,
                                &mut body,
                                &mut transform,
                                &mut velocity,
                                &mut mass,
                            );
                        }
                    }
                }
                *input_focus = InputFocus::default();
            }
        }
    }
}
