mod control;
#[cfg(not(target_arch = "wasm32"))]
mod llm;
mod orb;
#[cfg(not(target_arch = "wasm32"))]
mod voice;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::MaterialPlugin;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;

const BACKGROUND: Color = Color::srgb(0.019, 0.022, 0.035);
const INITIAL_ACTIVITY: f32 = 0.30;
const CAMERA_DISTANCE: f32 = 3.8;
const FIELD_OF_VIEW: f32 = 38.0;

fn main() {
    // Before anything spawns a thread: changing the environment is only safe while there is one.
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = dotenvy::dotenv() {
        if !error.not_found() {
            eprintln!("could not read .env: {error}");
        }
    }
    let mut app = App::new();
    app.add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Orb".into(),
                    canvas: Some("#orb".into()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            }),
            MaterialPlugin::<orb::OrbMaterial>::default(),
    ))
    .insert_resource(control::Activity::new(INITIAL_ACTIVITY))
    .init_resource::<control::Conversation>()
    .init_resource::<control::Glow>()
    .init_resource::<control::Spin>()
    .init_resource::<control::Reveal>()
    .add_systems(Startup, (spawn_camera, orb::spawn_orb))
    .add_systems(
        Update,
        (
            control::follow_conversation,
            control::turn,
            control::ease_activity,
            control::ease_glow,
            control::advance_reveal,
            orb::animate_orb,
        )
            .chain(),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(voice::VoicePlugin);
    app.run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(BACKGROUND),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: FIELD_OF_VIEW.to_radians(),
            near: 0.1,
            far: 100.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 0.0, CAMERA_DISTANCE).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::TonyMcMapface,
        // Additive rather than energy conserving, so overlapping ribbons pile light on top of each
        // other the way the reference does instead of averaging out.
        Bloom {
            intensity: 0.15,
            low_frequency_boost: 0.35,
            composite_mode: BloomCompositeMode::Additive,
            // Only the hot core should bleed; a low threshold smears glow over every ribbon and blurs
            // them.
            prefilter: BloomPrefilter {
                threshold: 0.9,
                threshold_softness: 0.35,
            },
            ..Bloom::NATURAL
        },
    ));
}
