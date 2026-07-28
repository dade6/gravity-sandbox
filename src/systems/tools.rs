use bevy::prelude::*;

/// Strumento attivo
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Tool {
    #[default]
    Select,
    Add,
    Move,
    Delete,
}

/// Risorsa: strumento correntemente selezionato
#[derive(Resource, Default)]
pub struct CurrentTool(pub Tool);

/// Plugin per la gestione degli strumenti
pub struct ToolPlugin;

impl Plugin for ToolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentTool>()
            .add_systems(Update, (handle_tool_shortcuts, sync_tool_buttons));
    }
}

/// Cambia tool con shortcut 1-4
fn handle_tool_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut current: ResMut<CurrentTool>,
) {
    let new_tool = if keys.just_pressed(KeyCode::Digit1) { Some(Tool::Select) }
    else if keys.just_pressed(KeyCode::Digit2) { Some(Tool::Add) }
    else if keys.just_pressed(KeyCode::Digit3) { Some(Tool::Move) }
    else if keys.just_pressed(KeyCode::Digit4) { Some(Tool::Delete) }
    else { None };

    if let Some(tool) = new_tool {
        current.0 = tool;
    }
}

/// Sincronizza bottoni toolbar con tool attivo (highlight)
fn sync_tool_buttons(
    current: Res<CurrentTool>,
    mut btn_query: Query<(&mut BackgroundColor, &crate::systems::ui::ToolBtn)>,
) {
    if !current.is_changed() {
        return;
    }
    for (mut bg, btn) in btn_query.iter_mut() {
        let is_active = match (btn.0, &current.0) {
            ("Select", Tool::Select) => true,
            ("Add", Tool::Add) => true,
            ("Move", Tool::Move) => true,
            ("Delete", Tool::Delete) => true,
            _ => false,
        };
        if is_active {
            bg.0 = Color::srgba(1.0, 1.0, 1.0, 0.15);
        } else {
            bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.0);
        }
    }
}
