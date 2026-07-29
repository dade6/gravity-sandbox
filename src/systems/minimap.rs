use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

use crate::components::celestial::CelestialBody;
use crate::systems::camera::MainCamera;

/// Plugin per la minimap
pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_minimap)
            .add_systems(Update, (update_minimap_camera, handle_minimap_click, update_viewport_rect));
    }
}

#[derive(Component)]
struct MinimapCamera;

#[derive(Component)]
struct MinimapContainer;

/// Marker per il rettangolo viewport sulla minimap
#[derive(Component)]
struct ViewportRect;

const MAP_SIZE: f32 = 150.0;
const MAP_BORDER: f32 = 4.0;
const MAP_MARGIN: f32 = 12.0;
const BOUNDS_PADDING: f32 = 1.4;

fn setup_minimap(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let size = Extent3d {
        width: MAP_SIZE as u32,
        height: MAP_SIZE as u32,
        depth_or_array_layers: 1,
    };

    let image = Image {
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        data: Some(vec![0u8; MAP_SIZE as usize * MAP_SIZE as usize * 4]),
        ..default()
    };

    let image_handle = images.add(image);

    // Camera secondaria con render target
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.02, 0.06)),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::Fixed {
                width: MAP_SIZE,
                height: MAP_SIZE,
            },
            ..OrthographicProjection::default_2d()
        }),
        RenderTarget::Image(image_handle.clone().into()),
        MinimapCamera,
        RenderLayers::layer(0),
    ));

    // UI container
    commands
        .spawn((
            MinimapContainer,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(64.0),
                right: Val::Px(MAP_MARGIN),
                width: Val::Px(MAP_SIZE + MAP_BORDER),
                height: Val::Px(MAP_SIZE + MAP_BORDER),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::px(6.0, 6.0, 6.0, 6.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.06, 0.7)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.15)),
        ))
        .with_child((
            Node {
                width: Val::Px(MAP_SIZE),
                height: Val::Px(MAP_SIZE),
                ..default()
            },
            ImageNode::new(image_handle),
        ))
        // Viewport rect: child del container (sibling dell'immagine)
        .with_child((
            ViewportRect,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.45)),
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
        ));
}

fn update_minimap_camera(
    bodies: Query<(&CelestialBody, &GlobalTransform)>,
    mut camera_query: Query<(&mut Transform, &mut Projection), With<MinimapCamera>>,
) {
    let (mut transform, mut projection) = match camera_query.single_mut() {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut bounds_min = Vec2::splat(f32::MAX);
    let mut bounds_max = Vec2::splat(f32::MIN);
    let mut has_bodies = false;

    for (body, xform) in bodies.iter() {
        let pos = xform.translation().truncate();
        let r = body.radius;
        bounds_min = bounds_min.min(pos - Vec2::splat(r));
        bounds_max = bounds_max.max(pos + Vec2::splat(r));
        has_bodies = true;
    }

    if !has_bodies {
        return;
    }

    let center = (bounds_min + bounds_max) / 2.0;
    transform.translation.x = center.x;
    transform.translation.y = center.y;

    let extent_half = (bounds_max - bounds_min) / 2.0;
    let max_extent = extent_half.x.max(extent_half.y).max(1.0);
    let base_half = MAP_SIZE / 2.0;
    let target_scale = (max_extent * BOUNDS_PADDING) / base_half;

    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = target_scale.max(0.01);
    }
}

/// Click sulla minimap → centra la camera principale
fn handle_minimap_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    minimap_camera_query: Query<(&Transform, &Projection), With<MinimapCamera>>,
    mut main_camera_query: Query<&mut Transform, (With<MainCamera>, Without<MinimapCamera>)>,
) {
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };
    let cursor = match window.cursor_position() {
        Some(p) => p,
        None => return,
    };

    // Screen rect della minimap (calcolata dalle posizioni note)
    let w = window.width();
    let h = window.height();

    let container_left = w - MAP_MARGIN - (MAP_SIZE + MAP_BORDER);
    let container_top = h - 64.0 - (MAP_SIZE + MAP_BORDER);
    // content area starts at container_left + border(1), container_top + border(1)
    let image_left = container_left + 1.0;
    let image_top = container_top + 1.0;
    let image_right = image_left + MAP_SIZE;
    let image_bottom = image_top + MAP_SIZE;

    if cursor.x < image_left || cursor.x > image_right
        || cursor.y < image_top || cursor.y > image_bottom
    {
        return; // click fuori dalla minimap
    }

    // Posizione del click in pixel relativi alla minimap (0,0 = top-left)
    let px = cursor.x - image_left;   // [0, MAP_SIZE]
    let py = cursor.y - image_top;    // [0, MAP_SIZE]

    let (mm_transform, mm_projection) = match minimap_camera_query.single() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mm_center = mm_transform.translation.truncate();
    let mm_scale = if let Projection::Orthographic(ortho) = mm_projection {
        ortho.scale
    } else {
        return;
    };

    // Converti pixel minimap → coordinate mondo
    let half = MAP_SIZE / 2.0;
    let world_x = mm_center.x + (px - half) * mm_scale;
    let world_y = mm_center.y - (py - half) * mm_scale;

    // Muovi camera principale
    if let Ok(mut main_transform) = main_camera_query.single_mut() {
        main_transform.translation.x = world_x;
        main_transform.translation.y = world_y;
    }
}

/// Aggiorna il rettangolo viewport sulla minimap
fn update_viewport_rect(
    windows: Query<&Window>,
    minimap_camera_query: Query<(&Transform, &Projection), With<MinimapCamera>>,
    main_camera_query: Query<(&Transform, &Projection), (With<MainCamera>, Without<MinimapCamera>)>,
    mut viewport_query: Query<&mut Node, With<ViewportRect>>,
) {
    let (mm_transform, mm_projection) = match minimap_camera_query.single() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mm_center = mm_transform.translation.truncate();
    let mm_scale = if let Projection::Orthographic(ortho) = mm_projection {
        ortho.scale
    } else {
        return;
    };

    let (main_transform, main_projection) = match main_camera_query.single() {
        Ok(c) => c,
        Err(_) => return,
    };
    let main_center = main_transform.translation.truncate();
    let main_scale = if let Projection::Orthographic(ortho) = main_projection {
        ortho.scale
    } else {
        return;
    };

    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };

    // Visibile world rect della camera principale (ScalingMode::WindowSize)
    let half_w = window.width() / 2.0 * main_scale;
    let half_h = window.height() / 2.0 * main_scale;
    let world_left = main_center.x - half_w;
    let world_right = main_center.x + half_w;
    let world_top = main_center.y + half_h;   // y-up in Bevy
    let world_bottom = main_center.y - half_h;

    // Converti world → minimap pixel coordinate (nel sistema di coordinate dell'immagine)
    // pixel_x = (world_x - mm_center.x) / mm_scale + 75
    // pixel_y = -(world_y - mm_center.y) / mm_scale + 75   (y invertita)
    let half = MAP_SIZE / 2.0;
    let px = |wx: f32| (wx - mm_center.x) / mm_scale + half;
    let py = |wy: f32| -(wy - mm_center.y) / mm_scale + half;

    let left_px = px(world_left).clamp(0.0, MAP_SIZE);
    let right_px = px(world_right).clamp(0.0, MAP_SIZE);
    let top_px = py(world_top).clamp(0.0, MAP_SIZE);
    let bottom_px = py(world_bottom).clamp(0.0, MAP_SIZE);

    let rect_left = left_px.min(right_px);
    let rect_top = top_px.min(bottom_px);
    let rect_width = (right_px - left_px).abs().max(2.0);
    let rect_height = (bottom_px - top_px).abs().max(2.0);

    // Aggiorna il Node del viewport rect
    if let Ok(mut node) = viewport_query.single_mut() {
        node.left = Val::Px(rect_left);
        node.top = Val::Px(rect_top);
        node.width = Val::Px(rect_width);
        node.height = Val::Px(rect_height);
    }
}
