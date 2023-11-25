mod player;
pub mod regino;
pub mod terrain;

use bevy::{
    core_pipeline::{bloom::BloomSettings, experimental::taa::TemporalAntiAliasBundle},
    prelude::*,
    window::close_on_esc,
};
use bevy_debug_text_overlay::OverlayPlugin;
use bevy_inspector_egui::quick::WorldInspectorPlugin;
// use bevy_editor_pls::EditorPlugin;
// use bevy_framepace::FramepacePlugin;
use bevy_xpbd_3d::prelude::*;
use player::PlayerFollowingCamera;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // .add_plugins(OverlayPlugin::default())
        // .add_plugins(EditorPlugin::default())
        // .add_plugins(FramepacePlugin)
        // .add_plugins(WorldInspectorPlugin::new())
        .add_plugins(PhysicsPlugins::default())
        // .add_plugins(PhysicsDebugPlugin::default())
        .add_plugins(regino::ReginoPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, close_on_esc)
        .run();
}

fn setup(mut commands: Commands) {
    commands
        .spawn(Camera3dBundle {
            transform: Transform::from_translation(Vec3::new(0.0, 4.0, 6.0))
                .looking_at(Vec3::ZERO, Vec3::Y),
            camera: Camera {
                hdr: true,
                ..default()
            },
            ..default()
        })
        .insert(BloomSettings {
            intensity: 0.1,
            ..default()
        })
        .insert(TemporalAntiAliasBundle::default())
        .insert(Name::new("MainCamera"))
        .insert(PlayerFollowingCamera);

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 0.2,
    });
}
