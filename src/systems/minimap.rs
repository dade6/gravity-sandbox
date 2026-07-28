use bevy::prelude::*;

use crate::components::celestial::CelestialBody;

/// Plugin per la minimap (disegnata come UI)
pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_minimap_bg)
            .add_systems(Update, update_minimap_dots);
    }
}

#[derive(Component)]
struct MinimapBg;

const MAP_SIZE: f32 = 120.0;
const MAP_MARGIN: f32 = 12.0;

fn spawn_minimap_bg(mut commands: Commands) {
    commands.spawn((
        MinimapBg,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(64.0),
            left: Val::Px(MAP_MARGIN),
            width: Val::Px(MAP_SIZE + 8.0),
            height: Val::Px(MAP_SIZE + 8.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.05, 0.1, 0.7)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.15)),
    ));
}

/// Aggiorna marker corpi nella minimap
fn update_minimap_dots(
    bodies: Query<(&CelestialBody, &GlobalTransform)>,
    mut gizmos: Gizmos,
    minimap_q: Query<&Node, With<MinimapBg>>,
    main_cam_pos: Query<&Transform, (With<Camera2d>, Without<MinimapBg>)>,
) {
    // We draw gizmo dots at the minimap position
    // In a real implementation, we would use UI nodes
    // For now this is a placeholder that will be rendered as gizmos
}
