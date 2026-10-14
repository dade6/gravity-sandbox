//! Pannello "Sfondo" — TUTTI i parametri dello starfield, in Bevy UI nativa.
//!
//! Stesso schema del pannello Impostazioni globali (settings.rs), che qui
//! viene riusato riga per riga: bottone in toolbar con lo stesso stile,
//! modale centrato con sfondo scuro, X in alto a destra, header di sezione,
//! campi `EditableText` + `PropInput` (keypad iOS per la digitazione), e
//! applicazione LIVE al confermare (OK del keypad / tap su un altro campo /
//! X / tap fuori dal riquadro).
//!
//! Il pannello non conosce la struct dei parametri: parla solo di chiavi
//! `set_sf_*`, mappate da `parallax::{starfield_field_value,
//! apply_starfield_field}`. Il prefisso `set_` le fa riconoscere al keypad
//! come campi numerici (keypad.rs::is_numeric_field).
//!
//! Effetto di un cambiamento: `refresh_starfield_on_tuning_change` (parallax)
//! rigenera le texture e butta giù i settori, che vengono ricreati con i
//! nuovi numeri nello stesso frame — quindi la modifica si vede subito.
//!
//! Z-order: overlay = 300 (come settings/delete: UN dialog alla volta),
//! keypad = 400.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontSize, FontSource, TextCursorStyle};
use bevy::ui::widget::TextScroll;
use bevy::ui::ScrollPosition;

use crate::systems::keypad::Keypad;
use crate::systems::parallax::{apply_starfield_field, starfield_field_value, StarfieldTuning};
use crate::systems::settings::{
    point_in_node, spawn_field_row, spawn_section_header, ui_press_position, BORDER_COLOR,
    DIALOG_BG, OVERLAY_FG, SETTINGS_Z, TEXT_COLOR,
};
use crate::systems::tools::PendingDelete;
use crate::systems::ui::PropInput;

// === Marker components ===

/// Bottone "Sfondo" nella toolbar (accanto a Settings).
#[derive(Component)]
pub struct StarfieldBtn;

/// Radice del modale Sfondo (overlay fullscreen).
#[derive(Component)]
pub struct StarfieldDialog;

/// Grace frame: il tap che ha APERTO il modale è `just_pressed` nello stesso
/// frame, e `apply_starfield_on_confirm` lo leggerebbe come tap-fuori
/// chiudendo il modale appena nato (bug "premo Sfondo e non compare nulla").
#[derive(Component)]
pub struct StarfieldDialogJustOpened;

/// Riquadro centrato: serve al hit-test del tap-fuori.
#[derive(Component)]
pub struct StarfieldBox;

/// Bottone X (chiudi).
#[derive(Component)]
pub struct StarfieldCloseBtn;

/// Bottone "Ripristina" (torna ai default spediti).
#[derive(Component)]
pub struct StarfieldResetBtn;

// === Campi, per sezione ===
//
// Etichette in italiano e nome del parametro fra parentesi: il nome è quello
// del codice, così si può chiedere una modifica chiamandolo per nome.

/// (chiave, etichetta) dei campi comuni ai tre layer. La gamma di dimensione
/// copre TUTTO (polvere → stelle grandi) e la parallasse ha la sua
/// oscillazione: ogni layer è un cielo completo, non "il livello delle piccole".
const LAYER_FIELDS: &[(&str, &str)] = &[
    ("factor", "Parallasse media (1 = incollato alla camera):"),
    ("spread", "Oscillazione parallasse (± per stella):"),
    ("stars", "Stelle per settore (densità):"),
    ("rmin", "Dimensione minima (polvere):"),
    ("rmax", "Dimensione massima (grandi):"),
    ("sizepow", "Sbilanciamento dimensioni (>1 = più piccole):"),
    ("big", "Quota stelle con alone+croce (0-1):"),
    ("bright", "Luminosità (1 = riferimento):"),
];

/// Chiave completa di un campo di layer: `set_sf_l{n}_{field}`.
fn layer_key_at(layer_no: usize, field: &str) -> &'static str {
    // Le stringhe sono statiche e poche: la tabella evita allocazioni nel
    // percorso di spawn del pannello.
    const KEYS: [[&str; 8]; 3] = [
        [
            "set_sf_l1_factor",
            "set_sf_l1_spread",
            "set_sf_l1_stars",
            "set_sf_l1_rmin",
            "set_sf_l1_rmax",
            "set_sf_l1_sizepow",
            "set_sf_l1_big",
            "set_sf_l1_bright",
        ],
        [
            "set_sf_l2_factor",
            "set_sf_l2_spread",
            "set_sf_l2_stars",
            "set_sf_l2_rmin",
            "set_sf_l2_rmax",
            "set_sf_l2_sizepow",
            "set_sf_l2_big",
            "set_sf_l2_bright",
        ],
        [
            "set_sf_l3_factor",
            "set_sf_l3_spread",
            "set_sf_l3_stars",
            "set_sf_l3_rmin",
            "set_sf_l3_rmax",
            "set_sf_l3_sizepow",
            "set_sf_l3_big",
            "set_sf_l3_bright",
        ],
    ];
    let idx = LAYER_FIELDS
        .iter()
        .position(|(k, _)| *k == field)
        .expect("campo di layer sconosciuto");
    KEYS[layer_no][idx]
}

/// Campi globali: forma delle stelle, galassie, settori.
const SHAPE_FIELDS: &[(&str, &str)] = &[
    ("set_sf_dot_scale", "dot_scale (quad del puntino):"),
    ("set_sf_spike_scale", "spike_scale (quad stella grande):"),
    ("set_sf_spike_core", "spike_core (nucleo saturo):"),
    ("set_sf_spike_bloom", "spike_bloom (strettezza alone):"),
    ("set_sf_spike_width", "spike_width (spessore bracci):"),
    ("set_sf_spike_strength", "spike_strength (forza bracci):"),
    (
        "set_sf_spike_taper",
        "spike_taper (lunghezza bracci, <1 = lunghi):",
    ),
];
const GALAXY_FIELDS: &[(&str, &str)] = &[
    ("set_sf_galaxy_chance", "Probabilità per settore (0-1):"),
    ("set_sf_galaxy_min", "Dimensione minima:"),
    ("set_sf_galaxy_max", "Dimensione massima:"),
];
const SECTOR_FIELDS: &[(&str, &str)] = &[
    ("set_sf_sector", "Lato del settore (unità mondo):"),
    ("set_sf_spawn_margin", "Margine di spawn:"),
    ("set_sf_keep_margin", "Margine di keep (isteresi):"),
    ("set_sf_max_sectors", "Settori massimi per layer:"),
];

/// Tutte le chiavi del pannello (test di copertura).
pub(crate) fn all_field_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();
    for layer in 0..3 {
        for (field, _) in LAYER_FIELDS {
            keys.push(layer_key_at(layer, field));
        }
    }
    for (key, _) in SHAPE_FIELDS
        .iter()
        .chain(GALAXY_FIELDS)
        .chain(SECTOR_FIELDS)
    {
        keys.push(key);
    }
    keys
}

pub struct StarfieldPanelPlugin;

impl Plugin for StarfieldPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_starfield_button,
                apply_starfield_on_confirm,
                scroll_starfield_panel,
                handle_starfield_reset,
                sync_starfield_fields_on_tuning_change,
                close_starfield_on_delete_dialog,
                close_starfield_when_settings_open,
            ),
        );
    }
}

// ============================================================
// Bottone nella toolbar
// ============================================================

/// Spawna il bottone "Sfondo" con lo stesso stile di Settings (36px,
/// padding 14, radius 8, font 14). Chiamato da spawn_toolbar (ui.rs)
/// subito dopo `spawn_settings_button`.
pub(crate) fn spawn_starfield_button(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands) {
    parent
        .spawn((
            Button,
            StarfieldBtn,
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
            Text::new("Sfondo"),
            TextFont {
                font: FontSource::default(),
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(TEXT_COLOR),
        ));
}

// ============================================================
// Apertura / chiusura
// ============================================================

fn handle_starfield_button(
    mut interaction_query: Query<
        (&Interaction, &StarfieldBtn, &mut BackgroundColor),
        Changed<Interaction>,
    >,
    pending: Res<PendingDelete>,
    panels: Query<Entity, With<StarfieldDialog>>,
    settings: Query<Entity, With<crate::systems::settings::SettingsDialog>>,
    tuning: Res<StarfieldTuning>,
    mut commands: Commands,
) {
    crate::mark_system("handle_starfield_button");
    let mut pressed = false;
    for (interaction, _, mut bg) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *bg = Color::srgba(1.0, 1.0, 1.0, 0.15).into();
                pressed = true;
            }
            Interaction::Hovered => {
                *bg = Color::srgba(1.0, 1.0, 1.0, 0.08).into();
            }
            Interaction::None => {
                *bg = Color::srgba(0.0, 0.0, 0.0, 0.0).into();
            }
        }
    }
    if !pressed {
        return;
    }
    // Toggle: se è già aperto, il tap lo chiude.
    if let Some(panel) = panels.iter().next() {
        commands.entity(panel).despawn();
        return;
    }
    // UN dialog alla volta: niente panel mentre si conferma una delete.
    if pending.0.is_some() {
        return;
    }
    // Se il pannello Impostazioni è aperto lo si chiude: mai due modali
    // sovrapposti (hanno lo stesso z).
    for dialog in settings.iter() {
        commands.entity(dialog).despawn();
    }
    spawn_starfield_dialog(&mut commands, &tuning);
}

/// Chiude il pannello quando il delete dialog si apre (rete di sicurezza).
fn close_starfield_on_delete_dialog(
    pending: Res<PendingDelete>,
    panels: Query<Entity, With<StarfieldDialog>>,
    mut commands: Commands,
) {
    crate::mark_system("close_starfield_on_delete_dialog");
    if pending.0.is_none() {
        return;
    }
    for panel in panels.iter() {
        commands.entity(panel).despawn();
    }
}

/// Mutua esclusione nella direzione opposta: Impostazioni aperto mentre il
/// pannello Sfondo è aperto → via il pannello Sfondo (il tap non può
/// attraversare il modale, ma copriamo gli spawn programmatici).
fn close_starfield_when_settings_open(
    panels: Query<Entity, With<StarfieldDialog>>,
    settings: Query<(), With<crate::systems::settings::SettingsDialog>>,
    mut commands: Commands,
) {
    crate::mark_system("close_starfield_when_settings_open");
    if settings.iter().next().is_none() {
        return;
    }
    for panel in panels.iter() {
        commands.entity(panel).despawn();
    }
}

// ============================================================
// Spawn del modale
// ============================================================

fn spawn_starfield_dialog(commands: &mut Commands, tuning: &StarfieldTuning) {
    commands
        .spawn((
            StarfieldDialog,
            StarfieldDialogJustOpened,
            GlobalZIndex(SETTINGS_Z),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(OVERLAY_FG),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    StarfieldBox,
                    // Il riquadro è SCORREVOLE: 38 campi non stanno in nessuno
                    // schermo di iPhone. `Overflow::scroll_y` fa il clip e
                    // `ScrollPosition` sposta il contenuto; chi li muove è
                    // `scroll_starfield_panel` (dito/mouse) — Bevy NON scorre
                    // da solo le aree in overflow.
                    ScrollPosition::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::px(10.0, 10.0, 10.0, 10.0),
                        width: Val::Percent(92.0),
                        max_width: Val::Px(380.0),
                        max_height: Val::Percent(82.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(DIALOG_BG),
                    BorderColor::all(BORDER_COLOR),
                ))
                .with_children(|dialog| {
                    // Header: titolo + Ripristina + X
                    // Header FISSO: con 38 campi il contenuto scorre, ma il
                    // titolo, "Ripristina" e la X devono restare raggiungibili
                    // anche in fondo (`IgnoreScroll` = non scorre col padre).
                    dialog
                        .spawn((
                            IgnoreScroll(BVec2::new(true, true)),
                            Node {
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::SpaceBetween,
                                align_items: AlignItems::Center,
                                width: Val::Percent(100.0),
                                ..default()
                            },
                        ))
                        .with_children(|header| {
                            header.spawn((
                                Text::new("Sfondo stellato"),
                                TextFont {
                                    font: FontSource::default(),
                                    font_size: FontSize::Px(15.0),
                                    ..default()
                                },
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                            ));
                            header
                                .spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(6.0),
                                    align_items: AlignItems::Center,
                                    ..default()
                                })
                                .with_children(|buttons| {
                                    buttons
                                        .spawn((
                                            Button,
                                            StarfieldResetBtn,
                                            Node {
                                                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
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
                                            Text::new("Ripristina"),
                                            TextFont {
                                                font: FontSource::default(),
                                                font_size: FontSize::Px(11.0),
                                                ..default()
                                            },
                                            TextColor(TEXT_COLOR),
                                        ));
                                    buttons
                                        .spawn((
                                            Button,
                                            StarfieldCloseBtn,
                                            Node {
                                                width: Val::Px(26.0),
                                                height: Val::Px(26.0),
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
                                            Text::new("X"),
                                            TextFont {
                                                font: FontSource::default(),
                                                font_size: FontSize::Px(13.0),
                                                ..default()
                                            },
                                            TextColor(TEXT_COLOR),
                                        ));
                                });
                        });

                    // Nota: come si comportano i cambiamenti + come si scorre.
                    dialog.spawn((
                        Text::new(
                            "Le modifiche si applicano subito: il cielo viene ricostruito \
                             con gli stessi semi (stesse stelle, nuovo aspetto). \
                             Scorri trascinando per vedere tutti i parametri.",
                        ),
                        TextFont {
                            font: FontSource::default(),
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                    ));

                    // I tre layer: sono lo stesso cielo a tre distanze.
                    let titles = [
                        "POLVERE (L1) — piccole, incollate alla camera",
                        "MEDIE (L2)",
                        "GRANDI (L3) — vicine, con croce di diffrazione",
                    ];
                    for (layer, title) in titles.iter().enumerate() {
                        spawn_section_header(dialog, title);
                        for (field, label) in LAYER_FIELDS {
                            let key = layer_key_at(layer, field);
                            spawn_field_row(dialog, key, label, starfield_field_value(key, tuning));
                        }
                    }

                    spawn_section_header(dialog, "FORMA DELLE STELLE");
                    for (key, label) in SHAPE_FIELDS {
                        spawn_field_row(dialog, key, label, starfield_field_value(key, tuning));
                    }

                    spawn_section_header(dialog, "GALASSIE (weenies)");
                    for (key, label) in GALAXY_FIELDS {
                        spawn_field_row(dialog, key, label, starfield_field_value(key, tuning));
                    }

                    spawn_section_header(dialog, "SETTORI (struttura)");
                    for (key, label) in SECTOR_FIELDS {
                        spawn_field_row(dialog, key, label, starfield_field_value(key, tuning));
                    }
                });
        });
}

// ============================================================
// Applicazione LIVE
// ============================================================

/// Applica i valori digitati quando il campo viene CONFERMATO:
/// - OK del keypad / tap su altro campo → applica subito (niente digitazione
///   persa);
/// - X del modale o tap fuori dal riquadro → chiude applicando anche i campi
///   rimasti (idempotente: ciò che è già applicato non riscrive nulla).
fn apply_starfield_on_confirm(
    dialog_query: Query<(Entity, Has<StarfieldDialogJustOpened>), With<StarfieldDialog>>,
    input_focus: Res<InputFocus>,
    fields: Query<(Entity, &PropInput, &EditableText)>,
    close_btns: Query<&Interaction, (With<StarfieldCloseBtn>, Changed<Interaction>)>,
    touches: Res<Touches>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<
        (&Camera, &GlobalTransform),
        (With<Camera2d>, With<crate::systems::camera::MainCamera>),
    >,
    box_nodes: Query<(&ComputedNode, &UiGlobalTransform), (With<StarfieldBox>, Without<Keypad>)>,
    keypad_nodes: Query<(&ComputedNode, &UiGlobalTransform), (With<Keypad>, Without<StarfieldBox>)>,
    mut tuning: ResMut<StarfieldTuning>,
    mut commands: Commands,
    mut last_focused: Local<Option<Entity>>,
) {
    crate::mark_system("apply_starfield_on_confirm");

    let Ok((dialog, just_opened)) = dialog_query.single() else {
        *last_focused = None;
        return;
    };

    // 1) Il focus ha lasciato un campo: applica quel campo subito.
    let current_focus = input_focus.get();
    if let Some(prev) = *last_focused {
        if Some(prev) != current_focus {
            if let Ok((_, prop, editable)) = fields.get(prev) {
                if prop.0.starts_with("set_sf_") {
                    apply_starfield_field(prop.0, &editable.value().to_string(), &mut tuning);
                }
            }
        }
    }
    *last_focused = current_focus.filter(|e| {
        fields
            .get(*e)
            .map(|(_, p, _)| p.0.starts_with("set_sf_"))
            .unwrap_or(false)
    });

    // 2) X: applica tutti i campi e chiude.
    if close_btns.iter().any(|i| *i == Interaction::Pressed) {
        close_and_apply_all(&mut commands, dialog, &fields, &mut tuning);
        return;
    }

    // 3) Tap fuori dal riquadro: applica tutto e chiude. Nel grace frame si
    //    ignora (è il tap che ha aperto il modale). Il keypad non conta come
    //    "fuori": sta sopra il modale ma non dentro il riquadro.
    if just_opened {
        commands
            .entity(dialog)
            .remove::<StarfieldDialogJustOpened>();
        return;
    }
    let Some(pos) = ui_press_position(&windows, &camera_query, &touches, &mouse_buttons) else {
        return;
    };
    let inside_box = box_nodes
        .iter()
        .any(|(node, gt)| point_in_node(node, gt, pos));
    let over_keypad = keypad_nodes
        .iter()
        .any(|(node, gt)| point_in_node(node, gt, pos));
    if !inside_box && !over_keypad {
        close_and_apply_all(&mut commands, dialog, &fields, &mut tuning);
    }
}

fn close_and_apply_all(
    commands: &mut Commands,
    dialog: Entity,
    fields: &Query<(Entity, &PropInput, &EditableText)>,
    tuning: &mut ResMut<StarfieldTuning>,
) {
    for (_, prop, editable) in fields.iter() {
        if prop.0.starts_with("set_sf_") {
            apply_starfield_field(prop.0, &editable.value().to_string(), tuning);
        }
    }
    commands.entity(dialog).despawn();
}

/// "Ripristina": rimette i default spediti (e riscrive i campi aperti).
fn handle_starfield_reset(
    reset_btns: Query<&Interaction, (With<StarfieldResetBtn>, Changed<Interaction>)>,
    dialog_query: Query<(), With<StarfieldDialog>>,
    input_focus: Res<InputFocus>,
    mut fields: Query<(Entity, &PropInput, &mut EditableText)>,
    mut tuning: ResMut<StarfieldTuning>,
) {
    crate::mark_system("handle_starfield_reset");
    if dialog_query.single().is_err() {
        return;
    }
    if !reset_btns.iter().any(|i| *i == Interaction::Pressed) {
        return;
    }
    *tuning = StarfieldTuning::default();
    let focused = input_focus.get();
    for (entity, prop, mut editable) in fields.iter_mut() {
        if prop.0.starts_with("set_sf_") && Some(entity) != focused {
            editable
                .editor
                .set_text(&starfield_field_value(prop.0, &tuning));
        }
    }
}

/// Riallinea i campi aperti quando il tuning cambia da fuori (Ripristina,
/// clamp di un valore fuori range, Load). Non tocca il campo in editing.
fn sync_starfield_fields_on_tuning_change(
    dialog_query: Query<(), With<StarfieldDialog>>,
    input_focus: Res<InputFocus>,
    mut fields: Query<(Entity, &PropInput, &mut EditableText)>,
    tuning: Res<StarfieldTuning>,
) {
    crate::mark_system("sync_starfield_fields_on_tuning_change");
    if dialog_query.single().is_err() || !tuning.is_changed() {
        return;
    }
    let focused = input_focus.get();
    for (entity, prop, mut editable) in fields.iter_mut() {
        if !prop.0.starts_with("set_sf_") {
            continue;
        }
        if Some(entity) == focused {
            continue;
        }
        let expected = starfield_field_value(prop.0, &tuning);
        if editable.value().to_string() != expected {
            editable.editor.set_text(&expected);
        }
    }
}

// ============================================================
// Scroll del pannello
// ============================================================
//
// Bevy NON fa scorrere da sé le aree `Overflow::scroll_*`: il widget
// `ScrollArea` di bevy_ui_widgets gestisce SOLO la rotellina/trackpad (eventi
// di picking `Pointer<Scroll>`), che su iPhone non esiste. Con 38 campi il
// pannello non entra in nessuno schermo di telefono, quindi lo scroll è
// implementato qui: il contenuto segue il DITO (drag) e la rotellina funziona
// su Mac. La matematica del clamp sta in `clamped_scroll_y` (pura, testata).

/// Posizione di scroll dopo un delta, mai oltre i bordi. `visible`/`content`
/// sono in pixel logici e `delta` è positivo quando il contenuto deve salire.
fn clamped_scroll_y(current: f32, delta: f32, visible: f32, content: f32) -> f32 {
    let max = (content - visible).max(0.0);
    (current + delta).clamp(0.0, max)
}

/// Posizione CORRENTE del dito/cursore (pixel fisici, viewport-relativi)
/// mentre è premuto; `None` se non c'è nessun contatto. Diversa da
/// `ui_press_position` (che risponde solo al PRIMO frame di pressione): lo
/// scroll deve seguire il movimento.
fn held_position(
    windows: &Query<&Window>,
    cameras: &Query<
        (&Camera, &GlobalTransform),
        (With<Camera2d>, With<crate::systems::camera::MainCamera>),
    >,
    touches: &Touches,
    mouse: &ButtonInput<MouseButton>,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let viewport_min = cameras
        .iter()
        .next()
        .and_then(|(cam, _)| cam.physical_viewport_rect())
        .map(|r| r.min.as_vec2())
        .unwrap_or_default();
    if let Some(touch) = touches.iter().next() {
        return Some(touch.position() * window.scale_factor() - viewport_min);
    }
    if mouse.pressed(MouseButton::Left) {
        return window.physical_cursor_position().map(|p| p - viewport_min);
    }
    None
}

/// Posizione del cursore in pixel fisici viewport-relativi (stessa convenzione
/// dei riquadri UI); `None` se il cursore non è sulla finestra.
fn cursor_position(
    windows: &Query<&Window>,
    cameras: &Query<
        (&Camera, &GlobalTransform),
        (With<Camera2d>, With<crate::systems::camera::MainCamera>),
    >,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let viewport_min = cameras
        .iter()
        .next()
        .and_then(|(cam, _)| cam.physical_viewport_rect())
        .map(|r| r.min.as_vec2())
        .unwrap_or_default();
    window.physical_cursor_position().map(|p| p - viewport_min)
}

/// Scroll del pannello: trascinamento col dito/mouse + rotellina.
fn scroll_starfield_panel(
    mut panels: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<StarfieldBox>>,
    // I bottoni DENTRO il modale: un tap su X/Ripristina non deve diventare
    // un trascinamento (il resto dei bottoni dell'app ha rect fuori dal box).
    buttons: Query<(&ComputedNode, &UiGlobalTransform), (With<Interaction>, Without<StarfieldBox>)>,
    mut wheel: MessageReader<MouseWheel>,
    touches: Res<Touches>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<
        (&Camera, &GlobalTransform),
        (With<Camera2d>, With<crate::systems::camera::MainCamera>),
    >,
    mut last: Local<Option<Vec2>>,
) {
    crate::mark_system("scroll_starfield_panel");
    let Ok((node, gt, mut scroll)) = panels.single_mut() else {
        *last = None;
        return;
    };
    let inv = node.inverse_scale_factor;
    let visible = node.size() * inv;
    let content = node.content_size() * inv;

    // 1) Dito/mouse: il contenuto segue il trascinamento.
    match held_position(&windows, &cameras, &touches, &mouse_buttons) {
        None => *last = None,
        Some(current) => match *last {
            None => {
                if point_in_node(node, gt, current)
                    && !buttons.iter().any(|(n, g)| point_in_node(n, g, current))
                {
                    *last = Some(current);
                }
            }
            Some(prev) => {
                *last = Some(current);
                // Dito che sale → delta positivo → il contenuto sale.
                let delta_y = (prev.y - current.y) * inv;
                if delta_y.abs() > 0.1 {
                    scroll.y = clamped_scroll_y(scroll.y, delta_y, visible.y, content.y);
                }
            }
        },
    }

    // 2) Rotellina/trackpad (Mac): SOLO se il cursore è sopra il riquadro.
    //    Lo stesso gesto a due dita è ANCHE lo zoom della scena
    //    (`zoom_camera` legge la stessa `AccumulatedMouseScroll`): fuori dal
    //    pannello deve restare alla scena, dentro al pannello deve scorrere i
    //    parametri e NON zoomare (vedi la guardia in `zoom_camera`).
    //    I messaggi si consumano SEMPRE: senza `read` resterebbero in coda e
    //    scatterebbero appena il cursore entra nel pannello.
    let over_box = cursor_position(&windows, &cameras)
        .map(|p| point_in_node(node, gt, p))
        .unwrap_or(false);
    for ev in wheel.read() {
        if !over_box {
            continue;
        }
        let step = match ev.unit {
            MouseScrollUnit::Line => 24.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        scroll.y = clamped_scroll_y(scroll.y, -ev.y * step, visible.y, content.y);
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_field_is_handled_by_the_tuning_mapping() {
        // Il pannello è una vista: ogni chiave che espone DEVE essere letta e
        // scritta dal tuning, altrimenti il campo mostra vuoto o non applica.
        let mut t = StarfieldTuning::default();
        for key in all_field_keys() {
            let shown = starfield_field_value(key, &t);
            assert!(!shown.is_empty(), "campo {key} senza valore");
            // Round-trip: riapplicare il valore MOSTRATO è un no-op (l'apply
            // ritorna "cambiato" solo se cambia qualcosa)...
            assert!(
                !apply_starfield_field(key, &shown, &mut t),
                "campo {key}: riapplicare il valore corrente non è un no-op"
            );
            assert_eq!(
                starfield_field_value(key, &t),
                shown,
                "campo {key} instabile nel round-trip"
            );
            // ...e un valore DIVERSO cambia davvero il parametro. Si confronta
            // il TUNING, non la stringa mostrata: il campo è arrotondato a 2-3
            // decimali, quindi una differenza piccola non si vede nel testo.
            let before = t.clone();
            let current: f32 = shown.parse().expect("il campo mostra un numero");
            let changed = current * 0.5 + 0.123;
            let changed_txt = format!("{changed:.4}");
            if changed_txt != shown {
                assert!(
                    apply_starfield_field(key, &changed_txt, &mut t),
                    "campo {key}: valore diverso non applicato"
                );
                assert_ne!(t, before, "campo {key}: valore diverso ma tuning invariato");
            }
            t = StarfieldTuning::default();
        }
    }

    #[test]
    fn panel_scroll_stays_within_the_content() {
        // Il clamp: mai sopra 0, mai oltre `contenuto − visibile`, e con
        // contenuto più corto del riquadro non si scorre affatto.
        assert_eq!(clamped_scroll_y(0.0, 50.0, 400.0, 1200.0), 50.0);
        assert_eq!(clamped_scroll_y(10.0, -50.0, 400.0, 1200.0), 0.0);
        assert_eq!(clamped_scroll_y(790.0, 100.0, 400.0, 1200.0), 800.0);
        assert_eq!(clamped_scroll_y(800.0, 500.0, 400.0, 1200.0), 800.0);
        // Contenuto che entra: nessuno scroll.
        assert_eq!(clamped_scroll_y(0.0, 120.0, 600.0, 500.0), 0.0);
        // Contenuto identico al riquadro: nessuno scroll.
        assert_eq!(clamped_scroll_y(0.0, 10.0, 400.0, 400.0), 0.0);
    }

    #[test]
    fn wheel_scrolls_only_when_the_pointer_is_over_the_panel() {
        // Il gesto a due dita del trackpad è ANCHE lo zoom della scena
        // (zoom_camera legge la stessa AccumulatedMouseScroll): il pannello
        // deve scorrere SOLO se il cursore è sopra di lui, altrimenti il
        // cursore fuori dal pannello (dove l'utente vuole zoomare la scena)
        // farebbe scorrere i parametri.
        use bevy::math::DVec2;
        use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};

        fn build(cursor: Vec2) -> App {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .init_resource::<InputFocus>()
                .init_resource::<Touches>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<PendingDelete>()
                .init_resource::<StarfieldTuning>()
                .add_message::<MouseWheel>()
                .add_systems(Update, scroll_starfield_panel);
            let mut window = Window::default();
            window
                .set_physical_cursor_position(Some(DVec2::new(cursor.x as f64, cursor.y as f64)));
            app.world_mut().spawn(window);
            app.world_mut().spawn((
                StarfieldBox,
                ComputedNode {
                    size: Vec2::new(300.0, 200.0),
                    content_size: Vec2::new(300.0, 1200.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(150.0, 100.0),
                ScrollPosition::default(),
            ));
            app
        }

        fn wheel(app: &mut App) {
            app.world_mut()
                .resource_mut::<Messages<MouseWheel>>()
                .write(MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    y: -3.0,
                    window: Entity::PLACEHOLDER,
                    phase: bevy::input::touch::TouchPhase::Moved,
                });
        }

        fn scroll_y(app: &mut App) -> f32 {
            let world = app.world_mut();
            world
                .query_filtered::<&ScrollPosition, With<StarfieldBox>>()
                .single(world)
                .expect("box")
                .y
        }

        // Cursore SOPRA il riquadro: la rotellina scorre.
        let mut over = build(Vec2::new(150.0, 100.0));
        wheel(&mut over);
        over.update();
        assert!(
            scroll_y(&mut over) > 0.0,
            "col cursore sopra il pannello la rotellina deve scorrere"
        );

        // Cursore FUORI: la rotellina è della scena (zoom), il pannello fermo.
        let mut away = build(Vec2::new(900.0, 900.0));
        wheel(&mut away);
        away.update();
        assert_eq!(
            scroll_y(&mut away),
            0.0,
            "col cursore fuori dal pannello la rotellina non deve scorrere i parametri"
        );
    }

    #[test]
    fn panel_systems_run_without_query_conflicts() {
        // GATE B0001: i sistemi del pannello toccano Interaction, EditableText,
        // ComputedNode/UiGlobalTransform e StarfieldTuning. Un conflitto di
        // query esplode solo ESEGUENDO il sistema (su WASM = "Unreachable"),
        // quindi qui girano davvero, su un'app minimale (niente GPU).
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<InputFocus>()
            .init_resource::<Touches>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<PendingDelete>()
            .init_resource::<StarfieldTuning>()
            .add_message::<MouseWheel>()
            .add_systems(
                Startup,
                |mut commands: Commands, tuning: Res<StarfieldTuning>| {
                    commands.spawn(Window::default());
                    spawn_starfield_dialog(&mut commands, &tuning);
                },
            )
            .add_systems(
                Update,
                (
                    handle_starfield_button,
                    apply_starfield_on_confirm,
                    scroll_starfield_panel,
                    handle_starfield_reset,
                    sync_starfield_fields_on_tuning_change,
                    close_starfield_on_delete_dialog,
                    close_starfield_when_settings_open,
                ),
            );
        app.update();
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<StarfieldDialog>>()
                .iter(world)
                .count(),
            1,
            "il modale deve restare aperto senza tap"
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<PropInput>>()
                .iter(world)
                .count(),
            38,
            "tutti i campi del pannello devono esistere"
        );
    }

    #[test]
    fn panel_field_keys_are_unique_and_prefixed() {
        let keys = all_field_keys();
        assert_eq!(
            keys.len(),
            38,
            "24 di layer (8 × 3) + 7 forma + 3 galassie + 4 settori"
        );
        let mut seen = std::collections::HashSet::new();
        for key in keys {
            assert!(key.starts_with("set_sf_"), "chiave fuori prefisso: {key}");
            assert!(seen.insert(key), "chiave duplicata: {key}");
        }
    }

    #[test]
    fn panel_has_a_button_and_no_dialog_before_the_tap() {
        // Boot: il bottone esiste nella toolbar e nessun modale è aperto.
        // (Il tap è coperto dal test end-to-end in lib.rs.)
        let mut app = App::new();
        app.add_systems(Startup, |mut commands: Commands| {
            commands
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|bar| spawn_starfield_button(bar));
        });
        app.update();
        let world = app.world_mut();
        let btns = world
            .query_filtered::<Entity, With<StarfieldBtn>>()
            .iter(world)
            .count();
        assert_eq!(btns, 1, "un solo bottone Sfondo nella toolbar");
        assert_eq!(
            world
                .query_filtered::<Entity, With<StarfieldDialog>>()
                .iter(world)
                .count(),
            0
        );
    }
}
