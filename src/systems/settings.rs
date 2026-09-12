//! Ticket 21 — pannello Impostazioni globali (Bevy UI nativa).
//!
//! Bottone "Settings" nella toolbar (dopo Reset/Salva, stesso stile) che apre
//! un modale centrato con TUTTI i parametri globali della sandbox:
//!
//! - GRAVITÀ: costante gravitazionale (`GravitationalConstant`)
//! - ILLUMINAZIONE GLOBALE: ambient intensity/color/range (`AmbientLight`)
//! - CURVA ALONE: falloff exp + soft edge (`GlowCurve`)
//! - TRAIETTORIE: enabled/history/prediction/sample (`TrajectoryConfig`)
//!
//! Pattern riutilizzati (decisioni prefissate del ticket):
//! - modale = delete dialog esistente: riquadro centrato, sfondo scuro
//!   semi-trasparente, X in alto a destra, UN dialog alla volta;
//! - campi = EditableText + PropInput del property panel (spawn_star_section);
//! - keypad iOS esistente per la digitazione (keypad.rs, `set_*` = numerico);
//! - ogni campo scrive la propria RISORSA al confermare (OK keypad / tap su
//!   altro campo / X modale / tap fuori dal riquadro) — effetto LIVE.
//!
//! Z-order: overlay settings = 300 (stesso livello del delete dialog, mai
//! entrambi visibili), keypad = 400 (sopra qualunque modale).

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontSize, FontSource, TextCursorStyle};
use bevy::ui::widget::TextScroll;

use crate::components::lighting::{AmbientLight, GlowCurve};
use crate::components::trajectory::TrajectoryConfig;
use crate::systems::keypad::Keypad;
use crate::systems::persistence::GravitationalConstant;
use crate::systems::tools::PendingDelete;
use crate::systems::ui::PropInput;

// === Marker components ===

/// Bottone "Settings" nella toolbar.
#[derive(Component)]
pub struct SettingsBtn;

/// Radice del modale settings (l'overlay fullscreen, come DeleteDialog).
#[derive(Component)]
pub struct SettingsDialog;

/// Il riquadro centrato del modale (figlio dell'overlay). Serve al
/// hit-test del tap-fuori: chiude solo se il tap è FUORI da questo box.
#[derive(Component)]
pub struct SettingsBox;

/// Bottone X (chiudi) del modale.
#[derive(Component)]
pub struct SettingsCloseBtn;

/// Bottone toggle ON/OFF di `TrajectoryConfig.enabled`.
#[derive(Component)]
pub struct SettingsToggleBtn;

/// Header di sezione del modale (stile ILLUMINAZIONE PIANETI / ALONE STELLA).
#[derive(Component)]
pub struct SettingsSection;

/// Colore testo dell'header di sezione (stesso giallo caldo del panel).
const SECTION_COLOR: Color = Color::srgba(1.0, 0.85, 0.4, 0.95);
/// Sfondo del riquadro modale (stesso del delete dialog in ui.rs).
const DIALOG_BG: Color = Color::srgba(0.12, 0.12, 0.22, 0.95);
/// Sfondo overlay (stesso del delete dialog in ui.rs).
const OVERLAY_FG: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
const TEXT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.75);
const BORDER_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
const INPUT_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.3);
const INPUT_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);
/// GlobalZIndex 300: stesso livello del delete dialog (ui.rs). UN dialog
/// alla volta: il gate in handle_settings_button / close_on_delete garantisce
/// che non siano mai visibili insieme, quindi lo stesso z va bene.
const SETTINGS_Z: i32 = 300;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_settings_button,
                apply_settings_on_confirm,
                update_settings_toggle,
                sync_settings_fields_on_resource_change,
                close_settings_on_delete_dialog,
            ),
        );
    }
}

// ============================================================
// Bottone Settings nella toolbar
// ============================================================

/// Spawna il bottone Settings DOPO Reset/Salva, con lo stesso stile esatto
/// degli altri bottoni (36px, padding 14px, border-radius 8, font 14).
/// Chiamato da spawn_toolbar (ui.rs) dopo i bottoni Salva.
pub(crate) fn spawn_settings_button(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands) {
    parent
        .spawn((
            Button,
            SettingsBtn,
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
            Text::new("Settings"),
            TextFont {
                font: FontSource::default(),
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(TEXT_COLOR),
        ));
}

/// Click sul bottone Settings: apre il modale se non c'è già un dialog aperto
/// (delete dialog compreso — UN dialog alla volta). Il bottone è gestito QUI e
/// non in handle_ui_buttons (che esclude i SettingsBtn per evitare doppio
/// handling dei colori).
fn handle_settings_button(
    mut interaction_query: Query<
        (&Interaction, &SettingsBtn, &mut BackgroundColor),
        Changed<Interaction>,
    >,
    pending: Res<PendingDelete>,
    dialog_query: Query<Entity, With<SettingsDialog>>,
    mut commands: Commands,
    windows: Query<&Window>,
    grav: Res<GravitationalConstant>,
    ambient: Res<AmbientLight>,
    glow: Res<GlowCurve>,
    trajectory: Res<TrajectoryConfig>,
) {
    crate::mark_system("handle_settings_button");
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
    // UN dialog alla volta: se il delete dialog è aperto (o un settings
    // esiste già), NON aprire.
    if pending.0.is_some() || dialog_query.iter().next().is_some() {
        return;
    }
    let window_size = windows
        .single()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(800.0, 600.0));
    spawn_settings_dialog(
        &mut commands,
        window_size,
        (&grav, &ambient, &glow, &trajectory),
    );
}

// ============================================================
// Spawn del modale
// ============================================================

/// Chiavi dei campi per sezione. Le chiavi iniziano con `set_` (riconosciute
/// dal keypad come numeriche e da update_property_panel come campi settings).
const GRAVITY_FIELDS: &[(&str, &str)] = &[("set_gravity", "Costante gravitazionale:")];
const AMBIENT_FIELDS: &[(&str, &str)] = &[
    ("set_ambient_intensity", "Ambient Intensity:"),
    ("set_ambient_r", "Ambient R:"),
    ("set_ambient_g", "Ambient G:"),
    ("set_ambient_b", "Ambient B:"),
    ("set_ambient_range", "Ambient Range:"),
];
const GLOW_FIELDS: &[(&str, &str)] = &[
    ("set_glow_falloff", "Falloff Exp:"),
    ("set_glow_soft", "Soft Edge:"),
];
const TRAJECTORY_FIELDS: &[(&str, &str)] = &[
    ("set_traj_history", "History Length:"),
    ("set_traj_prediction", "Prediction Steps:"),
    ("set_traj_sample", "Sample Interval:"),
];

/// Snapshot delle 4 risorse globali, clonato dalle `Res` (i valori CORRENTI
/// all'apertura del modale — mai default statici).
pub(crate) struct SettingsSnapshot {
    pub gravity: GravitationalConstant,
    pub ambient: AmbientLight,
    pub glow: GlowCurve,
    pub trajectory: TrajectoryConfig,
}

/// Valore corrente della risorsa per una chiave campo `set_` (usato allo
/// spawn e dal sync post-Load/Reset).
pub(crate) fn settings_field_value(key: &str, ctx: &SettingsSnapshot) -> String {
    match key {
        "set_gravity" => format!("{}", ctx.gravity.0),
        "set_ambient_intensity" => format!("{:.3}", ctx.ambient.intensity),
        "set_ambient_r" => format!("{:.3}", ctx.ambient.color[0]),
        "set_ambient_g" => format!("{:.3}", ctx.ambient.color[1]),
        "set_ambient_b" => format!("{:.3}", ctx.ambient.color[2]),
        "set_ambient_range" => format!("{}", ctx.ambient.range),
        "set_glow_falloff" => format!("{:.2}", ctx.glow.falloff_exp),
        "set_glow_soft" => format!("{:.3}", ctx.glow.soft_edge),
        "set_traj_history" => format!("{}", ctx.trajectory.history_length),
        "set_traj_prediction" => format!("{}", ctx.trajectory.prediction_steps),
        "set_traj_sample" => format!("{}", ctx.trajectory.sample_interval),
        _ => String::new(),
    }
}

fn spawn_settings_dialog(
    commands: &mut Commands,
    window_size: Vec2,
    snapshot: (&GravitationalConstant, &AmbientLight, &GlowCurve, &TrajectoryConfig),
) {
    let ctx = SettingsSnapshot {
        gravity: GravitationalConstant(snapshot.0.0),
        ambient: snapshot.1.clone(),
        glow: snapshot.2.clone(),
        trajectory: snapshot.3.clone(),
    };
    commands
        .spawn((
            SettingsDialog,
            GlobalZIndex(SETTINGS_Z),
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
            // Riquadro centrato (stesso stile del delete dialog).
            // scroll_y: con 11 campi + 4 sezioni su iPhone il contenuto può
            // superare l'80% dello schermo — clip lo renderebbe irraggiungibile.
            overlay
                .spawn((
                    SettingsBox,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::px(10.0, 10.0, 10.0, 10.0),
                        width: Val::Px(300.0),
                        max_height: Val::Px(window_size.y * 0.8),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(DIALOG_BG),
                    BorderColor::all(BORDER_COLOR),
                ))
                .with_children(|dialog| {
                    // Header: titolo + X in alto a destra
                    dialog
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            width: Val::Percent(100.0),
                            ..default()
                        })
                        .with_children(|header| {
                            header.spawn((
                                Text::new("Impostazioni globali"),
                                TextFont {
                                    font: FontSource::default(),
                                    font_size: FontSize::Px(15.0),
                                    ..default()
                                },
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                            ));
                            header
                                .spawn((
                                    Button,
                                    SettingsCloseBtn,
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

                    // === GRAVITÀ ===
                    spawn_section_header(dialog, "GRAVITÀ");
                    for (key, label) in GRAVITY_FIELDS {
                        spawn_field_row(dialog, key, label, &ctx);
                    }

                    // === ILLUMINAZIONE GLOBALE ===
                    spawn_section_header(dialog, "ILLUMINAZIONE GLOBALE");
                    for (key, label) in AMBIENT_FIELDS {
                        spawn_field_row(dialog, key, label, &ctx);
                    }

                    // === CURVA ALONE ===
                    spawn_section_header(dialog, "CURVA ALONE");
                    for (key, label) in GLOW_FIELDS {
                        spawn_field_row(dialog, key, label, &ctx);
                    }

                    // === TRAIETTORIE ===
                    spawn_section_header(dialog, "TRAIETTORIE");
                    // Enabled: toggle-bottone ON/OFF (NON campo testo)
                    dialog
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(4.0),
                            width: Val::Percent(100.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                Text::new("Enabled:"),
                                TextFont {
                                    font: FontSource::default(),
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(TEXT_COLOR),
                            ));
                            row.spawn((
                                Button,
                                SettingsToggleBtn,
                                Node {
                                    margin: UiRect::left(Val::Px(8.0)),
                                    padding: UiRect::axes(Val::Px(14.0), Val::Px(4.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
                                    ..default()
                                },
                                BackgroundColor(toggle_color(ctx.trajectory.enabled)),
                                BorderColor::all(BORDER_COLOR),
                            ))
                            .with_child((
                                Text::new(toggle_label(ctx.trajectory.enabled)),
                                TextFont {
                                    font: FontSource::default(),
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                            ));
                        });
                    for (key, label) in TRAJECTORY_FIELDS {
                        spawn_field_row(dialog, key, label, &ctx);
                    }
                });
        });
}

fn toggle_label(enabled: bool) -> &'static str {
    if enabled { "ON" } else { "OFF" }
}

fn toggle_color(enabled: bool) -> Color {
    if enabled {
        Color::srgba(0.15, 0.5, 0.25, 0.7)
    } else {
        Color::srgba(0.45, 0.15, 0.15, 0.7)
    }
}

/// Header di sezione (stile ILLUMINAZIONE PIANETI / ALONE STELLA del panel).
fn spawn_section_header(dialog: &mut bevy::ecs::hierarchy::ChildSpawnerCommands, title: &str) {
    dialog.spawn((
        SettingsSection,
        Text::new(title),
        TextFont {
            font: FontSource::default(),
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(SECTION_COLOR),
        Node {
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        },
    ));
}

/// Riga campo: label + input EditableText con PropInput(key), stesso pattern
/// di spawn_star_section. Il valore iniziale è quello CORRENTE della risorsa.
fn spawn_field_row(
    dialog: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    key: &'static str,
    label: &str,
    ctx: &SettingsSnapshot,
) {
    let mut row = dialog.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        width: Val::Percent(100.0),
        ..default()
    });
    row.with_child((
        Text::new(label),
        TextFont {
            font: FontSource::default(),
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(TEXT_COLOR),
    ));
    row.with_children(|input_container| {
        input_container
            .spawn((
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
                EditableText::new(settings_field_value(key, ctx)),
                PropInput(key),
                TextFont {
                    font: FontSource::default(),
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
                TextScroll(Vec2::ZERO),
                TextCursorStyle {
                    color: Color::WHITE,
                    selection_color: Color::srgba(0.45, 0.75, 1.0, 0.6),
                    unfocused_selection_color: Color::srgba(0.45, 0.75, 1.0, 0.2),
                    selected_text_color: None,
                },
                Node {
                    width: Val::Percent(100.0),
                    ..default()
                },
            ));
    });
}

// ============================================================
// Applicazione LIVE dei valori
// ============================================================

/// Posizione del click/tap corrente in pixel FISICI viewport-relativi
/// (stessa convenzione di ui_focus_system / selection_system:
/// `physical_cursor_position() - physical_viewport_rect().min`;
/// touch: `position() * scale_factor - viewport_min`). Nessun viewport
/// (= nessuna camera con render target) → None.
fn ui_press_position(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform), (With<Camera2d>, With<crate::systems::camera::MainCamera>)>,
    touches: &Touches,
    mouse_buttons: &Res<ButtonInput<MouseButton>>,
) -> Option<Vec2> {
    let w = windows.single().ok()?;
    let Ok((camera, _)) = camera_query.single() else {
        return None;
    };
    let viewport_min = camera
        .physical_viewport_rect()
        .map(|r| r.min.as_vec2())
        .unwrap_or_default();
    if mouse_buttons.just_pressed(MouseButton::Left) {
        w.physical_cursor_position()
            .map(|p| p - viewport_min)
    } else {
        touches
            .iter_just_pressed()
            .next()
            .map(|t| t.position() * w.scale_factor() - viewport_min)
    }
}

/// True se il punto (pixel fisici viewport-relativi) cade dentro il rect di
/// un nodo (stesso hit-test di selection.rs).
fn point_in_node(node: &ComputedNode, gt: &UiGlobalTransform, pos: Vec2) -> bool {
    let half = node.size / 2.0;
    pos.x >= gt.translation.x - half.x
        && pos.x <= gt.translation.x + half.x
        && pos.y >= gt.translation.y - half.y
        && pos.y <= gt.translation.y + half.y
}

/// Applica i valori digitati quando il campo viene CONFERMATO:
/// - OK keypad / tap su altro campo: il focus lascia il campo `set_` →
///   si applica subito (la digitazione non si perde mai);
/// - X del modale o tap fuori dal riquadro: chiude e applica anche i campi
///   rimasti (idempotente: i valori già applicati non riscrivono nulla).
///
/// B0001-safe: UNA ResMut per tipo risorsa in questo sistema (i quattro tipi
/// sono distinti; altrove le stesse risorse sono solo `Res`).
fn apply_settings_on_confirm(
    dialog_query: Query<Entity, With<SettingsDialog>>,
    input_focus: Res<InputFocus>,
    fields: Query<(Entity, &PropInput, &EditableText)>,
    close_btns: Query<&Interaction, (With<SettingsCloseBtn>, Changed<Interaction>)>,
    touches: Res<Touches>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<
        (&Camera, &GlobalTransform),
        (With<Camera2d>, With<crate::systems::camera::MainCamera>),
    >,
    // Il riquadro modale per il hit-test del tap-fuori (il keypad sta sopra
    // il modale ma NON dentro il riquadro: senza contarlo, ogni tasto del
    // keypad chiuderebbe il modale su iPhone).
    box_nodes: Query<(&ComputedNode, &UiGlobalTransform), (With<SettingsBox>, Without<Keypad>)>,
    keypad_nodes: Query<
        (&ComputedNode, &UiGlobalTransform),
        (With<Keypad>, Without<SettingsBox>),
    >,
    mut commands: Commands,
    mut grav: ResMut<GravitationalConstant>,
    mut ambient: ResMut<AmbientLight>,
    mut glow: ResMut<GlowCurve>,
    mut trajectory: ResMut<TrajectoryConfig>,
    // ultimo campo `set_` focussato (per l'apply on focus-loss)
    mut last_focused: Local<Option<Entity>>,
) {
    crate::mark_system("apply_settings_on_confirm");

    // Il modale non esiste: azzera il tracking e basta.
    let Ok(dialog) = dialog_query.single() else {
        *last_focused = None;
        return;
    };

    // --- 1) Focus lasciato un campo `set_`: applica quel campo subito ---
    let current_focus = input_focus.get();
    if let Some(prev) = *last_focused {
        if Some(prev) != current_focus {
            if let Ok((_, prop, editable)) = fields.get(prev) {
                if prop.0.starts_with("set_") {
                    apply_settings_field(
                        prop.0,
                        &editable.value().to_string(),
                        &mut grav,
                        &mut ambient,
                        &mut glow,
                        &mut trajectory,
                    );
                }
            }
        }
    }
    // aggiorna il tracking (solo se il nuovo focus è un campo del settings)
    *last_focused = current_focus.filter(|e| {
        fields
            .get(*e)
            .map(|(_, p, _)| p.0.starts_with("set_"))
            .unwrap_or(false)
    });

    // --- 2) X premuta: applica TUTTI i campi e chiude ---
    let mut close_pressed = false;
    for interaction in close_btns.iter() {
        if *interaction == Interaction::Pressed {
            close_pressed = true;
        }
    }
    if close_pressed {
        close_and_apply_all(
            &mut commands,
            dialog,
            &fields,
            &mut grav,
            &mut ambient,
            &mut glow,
            &mut trajectory,
        );
        return;
    }

    // --- 3) Tap FUORI dal riquadro (sull'overlay): applica tutto e chiude.
    //     Il tap NON seleziona corpi né spawna nulla: l'overlay è un nodo UI
    //     fullscreen che la guardia UI di selection/tools vede (il tap NON
    //     attraversa). Il tap sui tasti del KEYPAD non conta (il keypad sta
    //     sopra il modale ma fuori dal riquadro).
    let Some(pos) = ui_press_position(&windows, &camera_query, &touches, &mouse_buttons) else {
        return;
    };
    let inside_box = box_nodes.iter().any(|(node, gt)| point_in_node(node, gt, pos));
    let over_keypad = keypad_nodes.iter().any(|(node, gt)| point_in_node(node, gt, pos));
    if !inside_box && !over_keypad {
        // tap sull'overlay (fuori dal riquadro e dal keypad): chiudi applicando
        close_and_apply_all(
            &mut commands,
            dialog,
            &fields,
            &mut grav,
            &mut ambient,
            &mut glow,
            &mut trajectory,
        );
    }
}

/// Chiude il modale applicando TUTTI i campi `set_*` correnti.
fn close_and_apply_all(
    commands: &mut Commands,
    dialog: Entity,
    fields: &Query<(Entity, &PropInput, &EditableText)>,
    grav: &mut ResMut<GravitationalConstant>,
    ambient: &mut ResMut<AmbientLight>,
    glow: &mut ResMut<GlowCurve>,
    trajectory: &mut ResMut<TrajectoryConfig>,
) {
    for (_, prop, editable) in fields.iter() {
        if prop.0.starts_with("set_") {
            apply_settings_field(
                prop.0,
                &editable.value().to_string(),
                grav,
                ambient,
                glow,
                trajectory,
            );
        }
    }
    commands.entity(dialog).despawn();
}

/// Applica il testo di UN campo alla risorsa globale corrispondente, con
/// clamp dei range prefissati dal ticket. Prende riferimenti SEMPLICI (non
/// ResMut) così è testabile senza app; dai sistemi si chiamano i ResMut
/// con deref automatico (`&mut grav` deref-coerces a `&mut GravitationalConstant`).
/// Ritorna true se il valore è cambiato.
pub(crate) fn apply_settings_field(
    key: &str,
    text: &str,
    grav: &mut GravitationalConstant,
    ambient: &mut AmbientLight,
    glow: &mut GlowCurve,
    trajectory: &mut TrajectoryConfig,
) -> bool {
    let Ok(v) = text.trim().parse::<f32>() else {
        return false; // testo non valido: ignora, non rompere il modale
    };
    match key {
        "set_gravity" => {
            let clamped = v.clamp(0.1, 1.0e7);
            if grav.0 != clamped {
                grav.0 = clamped;
                return true;
            }
        }
        "set_ambient_intensity" => {
            let clamped = v.clamp(0.0, 1.0);
            if ambient.intensity != clamped {
                ambient.intensity = clamped;
                return true;
            }
        }
        "set_ambient_r" | "set_ambient_g" | "set_ambient_b" => {
            let clamped = v.clamp(0.0, 1.0);
            let idx = match key {
                "set_ambient_r" => 0,
                "set_ambient_g" => 1,
                _ => 2,
            };
            if ambient.color[idx] != clamped {
                ambient.color[idx] = clamped;
                return true;
            }
        }
        "set_ambient_range" => {
            let clamped = v.max(0.0);
            if ambient.range != clamped {
                ambient.range = clamped;
                return true;
            }
        }
        "set_glow_falloff" => {
            let clamped = v.clamp(0.5, 8.0);
            if glow.falloff_exp != clamped {
                glow.falloff_exp = clamped;
                return true;
            }
        }
        "set_glow_soft" => {
            let clamped = v.clamp(0.0, 0.1);
            if glow.soft_edge != clamped {
                glow.soft_edge = clamped;
                return true;
            }
        }
        "set_traj_history" => {
            let clamped = v.max(0.0) as usize;
            if trajectory.history_length != clamped {
                trajectory.history_length = clamped;
                return true;
            }
        }
        "set_traj_prediction" => {
            let clamped = v.max(0.0) as usize;
            if trajectory.prediction_steps != clamped {
                trajectory.prediction_steps = clamped;
                return true;
            }
        }
        "set_traj_sample" => {
            let clamped = v.max(1.0) as usize;
            if trajectory.sample_interval != clamped {
                trajectory.sample_interval = clamped;
                return true;
            }
        }
        _ => {}
    }
    false
}

// ============================================================
// Toggle ON/OFF traiettorie
// ============================================================

/// Click sul toggle: inverte `TrajectoryConfig.enabled` IMMEDIATAMENTE (i
/// sistemi trajectory leggono la config per-frame: scie e predizione
/// spariscono/ricompaiono al frame dopo, senza restart).
/// Label e colore restano coerenti con la risorsa anche dopo Load/Reset.
fn update_settings_toggle(
    toggle: Query<(&Interaction, &SettingsToggleBtn, &Children), Changed<Interaction>>,
    mut texts: Query<&mut Text>,
    mut bgs: Query<&mut BackgroundColor, With<SettingsToggleBtn>>,
    // UNA sola ResMut<TrajectoryConfig> in questo sistema (mai Res+ResMut
    // dello stesso tipo nello stesso sistema: B0001).
    mut config: ResMut<TrajectoryConfig>,
) {
    crate::mark_system("update_settings_toggle");
    let mut pressed = false;
    for (interaction, _, _) in toggle.iter() {
        if *interaction == Interaction::Pressed {
            pressed = true;
        }
    }
    if pressed {
        config.enabled = !config.enabled;
    }
    // Aggiorna SOLO quando qualcosa cambia (pressed o is_changed della
    // risorsa da Load/Reset) — non ogni frame (evita trigger a cascata di
    // change detection).
    if !pressed && !config.is_changed() {
        return;
    }
    let label = toggle_label(config.enabled);
    let color = toggle_color(config.enabled);
    for (_, _, children) in toggle.iter() {
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                if text.0 != label {
                    text.0 = label.to_string();
                }
            }
        }
    }
    for mut bg in bgs.iter_mut() {
        bg.0 = color;
    }
}

// ============================================================
// Sync campi dopo Load/Reset (pattern update_property_panel)
// ============================================================

/// Se una delle 4 risorse globali cambia mentre il modale è APERTO (Load/
/// Reset via JS, apply di un altro campo), i campi `set_` si risincronizzano
/// al valore corrente della risorsa — tranne il campo con il focus (non si
/// ruba la digitazione in corso). Il toggle è gestito da
/// update_settings_toggle.
fn sync_settings_fields_on_resource_change(
    dialog_query: Query<(), With<SettingsDialog>>,
    input_focus: Res<InputFocus>,
    mut fields: Query<(Entity, &PropInput, &mut EditableText)>,
    grav: Res<GravitationalConstant>,
    ambient: Res<AmbientLight>,
    glow: Res<GlowCurve>,
    trajectory: Res<TrajectoryConfig>,
) {
    crate::mark_system("sync_settings_fields_on_resource_change");
    if dialog_query.single().is_err() {
        return; // modale chiuso: i campi non esistono, si ricreano all'apertura
    }
    if !grav.is_changed() && !ambient.is_changed() && !glow.is_changed() && !trajectory.is_changed()
    {
        return; // nessuna risorsa è cambiata: i campi sono già coerenti
    }
    let ctx = SettingsSnapshot {
        gravity: GravitationalConstant(grav.0),
        ambient: ambient.clone(),
        glow: glow.clone(),
        trajectory: trajectory.clone(),
    };
    let focused = input_focus.get();
    for (entity, prop, mut editable) in fields.iter_mut() {
        if !prop.0.starts_with("set_") {
            continue; // campi del property panel: gestiti da update_property_panel
        }
        // Non sovrascrivere il campo che l'utente sta editando (digitazione
        // in corso: pattern update_property_panel).
        if Some(entity) == focused {
            continue;
        }
        let expected = settings_field_value(prop.0, &ctx);
        let current = editable.value().to_string();
        if current != expected {
            editable.editor.set_text(&expected);
        }
    }
}

// ============================================================
// Gate un-dialog-alla-volta (lato delete)
// ============================================================

/// Se il delete dialog si APRE mentre il settings è aperto, chiudi prima il
/// settings. Rete di sicurezza: con la guardia UI di delete_tool_system il
/// tap che apre il delete non può attraversare il modale settings, ma
/// copriamo anche gli spawn programmatici / edge case.
fn close_settings_on_delete_dialog(
    pending: Res<PendingDelete>,
    dialog_query: Query<Entity, With<SettingsDialog>>,
    mut commands: Commands,
) {
    crate::mark_system("close_settings_on_delete_dialog");
    if pending.0.is_some() {
        for entity in dialog_query.iter() {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La funzione di apply dei campi clampa i range del ticket e scrive la
    /// risorsa giusta per ogni chiave (riferimenti semplici: testabile pura).
    #[test]
    fn apply_settings_field_clamps_and_routes() {
        let mut g = GravitationalConstant(5000.0);
        let mut a = AmbientLight::default();
        let mut gl = GlowCurve::default();
        let mut t = TrajectoryConfig::default();

        // gravità: clamp min 0.1 / max 1e7
        assert!(apply_settings_field("set_gravity", "50", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(g.0, 50.0);
        assert!(apply_settings_field("set_gravity", "0.0001", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(g.0, 0.1, "gravity clamp min 0.1");
        assert!(apply_settings_field("set_gravity", "9e9", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(g.0, 1.0e7, "gravity clamp max 1e7");

        // ambient intensity 0..1
        assert!(apply_settings_field("set_ambient_intensity", "1.5", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(a.intensity, 1.0);
        assert!(apply_settings_field("set_ambient_intensity", "-3", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(a.intensity, 0.0);

        // ambient RGB 0..1 per canale. N.B. il default è [1,1,1]: R/B cambiano
        // davvero, G "2.0" clampa a 1.0 che è GIA' il valore corrente →
        // apply_settings_field ritorna false (idempotente, no riscrittura).
        assert!(apply_settings_field("set_ambient_r", "0.2", &mut g, &mut a, &mut gl, &mut t));
        assert!(!apply_settings_field("set_ambient_g", "2.0", &mut g, &mut a, &mut gl, &mut t));
        assert!(apply_settings_field("set_ambient_b", "0.2", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(a.color, [0.2, 1.0, 0.2]);
        // un valore DIVERSO dal corrente su G: cambia e clampa
        assert!(apply_settings_field("set_ambient_g", "0.4", &mut g, &mut a, &mut gl, &mut t));
        assert!(apply_settings_field("set_ambient_g", "2.0", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(a.color, [0.2, 1.0, 0.2]);

        // ambient range >= 0
        assert!(apply_settings_field("set_ambient_range", "2000", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(a.range, 2000.0);
        assert!(apply_settings_field("set_ambient_range", "-5", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(a.range, 0.0);

        // glow falloff 0.5..8, soft edge 0..0.1
        assert!(apply_settings_field("set_glow_falloff", "0.1", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(gl.falloff_exp, 0.5);
        assert!(apply_settings_field("set_glow_falloff", "50", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(gl.falloff_exp, 8.0);
        assert!(apply_settings_field("set_glow_soft", "0.5", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(gl.soft_edge, 0.1);
        assert!(apply_settings_field("set_glow_soft", "-1", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(gl.soft_edge, 0.0);

        // trajectory: history/prediction min 0, sample min 1
        assert!(apply_settings_field("set_traj_history", "50", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(t.history_length, 50);
        assert!(apply_settings_field("set_traj_history", "-4", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(t.history_length, 0);
        assert!(apply_settings_field("set_traj_prediction", "300", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(t.prediction_steps, 300);
        assert!(apply_settings_field("set_traj_sample", "0", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(t.sample_interval, 1, "sample interval min 1");
        assert!(apply_settings_field("set_traj_sample", "5", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(t.sample_interval, 5);

        // testo invalido: nessun panic, nessun cambio
        assert!(!apply_settings_field("set_gravity", "abc", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(g.0, 1.0e7);

        // idempotenza: stesso valore → false (nessuna riscrittura)
        assert!(!apply_settings_field("set_traj_sample", "5", &mut g, &mut a, &mut gl, &mut t));

        // testo con spazi → trim
        assert!(apply_settings_field("set_traj_history", "  120  ", &mut g, &mut a, &mut gl, &mut t));
        assert_eq!(t.history_length, 120);
    }

    /// La formattazione dei valori per chiave copre TUTTI i campi numerici
    /// del modale (nessun campo con stringa vuota per sbaglio).
    #[test]
    fn settings_field_value_covers_all_fields() {
        let ctx = SettingsSnapshot {
            gravity: GravitationalConstant(5000.0),
            ambient: AmbientLight::default(),
            glow: GlowCurve::default(),
            trajectory: TrajectoryConfig::default(),
        };
        let all: Vec<&str> = GRAVITY_FIELDS
            .iter()
            .chain(AMBIENT_FIELDS)
            .chain(GLOW_FIELDS)
            .chain(TRAJECTORY_FIELDS)
            .map(|(k, _)| *k)
            .collect();
        assert_eq!(all.len(), 11);
        for key in all {
            let v = settings_field_value(key, &ctx);
            assert!(!v.is_empty(), "campo {key} senza valore");
        }
        assert_eq!(settings_field_value("set_gravity", &ctx), "5000");
        assert_eq!(settings_field_value("set_ambient_intensity", &ctx), "0.120");
        assert_eq!(settings_field_value("set_traj_history", &ctx), "500");
    }
}
