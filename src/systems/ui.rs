use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};

/// Plugin per l'interfaccia utente del sandbox
pub struct SandboxUIPlugin;

impl Plugin for SandboxUIPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_ui);
    }
}

fn spawn_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                position_type: PositionType::Absolute,
                ..default()
            },
            ZIndex(10),
        ))
        .with_children(|parent| {
            toolbar(parent);
            timeline(parent);
        });
}

fn toolbar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(48.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                column_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.1, 0.7)),
        ))
        .with_children(|bar| {
            for label in &["Select (1)", "Add (2)", "Move (3)", "Delete (4)"] {
                bar.spawn((
                    Button,
                    Node {
                        height: Val::Px(32.0),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.2, 0.22, 0.3, 0.8)),
                ))
                .with_child((
                    Text::new(*label),
                    TextFont {
                        font: FontSource::default(),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.85, 0.9)),
                ));
            }

            bar.spawn(Node { flex_grow: 1.0, ..default() });

            bar.spawn((
                Text::new(format!("Sandbox {}", crate::version::VERSION)),
                TextFont {
                    font: FontSource::default(),
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.4)),
            ));
        });
}

fn timeline(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(56.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                padding: UiRect::horizontal(Val::Px(16.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|bar| {
            for label in &["▶ Play (Space)", "⏭ Step (.)"] {
                bar.spawn((
                    Button,
                    Node {
                        height: Val::Px(32.0),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.2, 0.22, 0.3, 0.8)),
                ))
                .with_child((
                    Text::new(*label),
                    TextFont {
                        font: FontSource::default(),
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.85, 0.9)),
                ));
            }

            bar.spawn((
                Text::new("Speed: 1.0×   +/-"),
                TextFont {
                    font: FontSource::default(),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.7, 0.8)),
            ));
        });
}
