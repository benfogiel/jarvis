use crate::control::{Activity, Glow, Reveal, Spin};
use bevy::asset::Asset;
use bevy::pbr::{Material, MeshMaterial3d};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub const ORB_RADIUS: f32 = 1.0;
const DEMO_IMAGE: &str = "demo/result.png";
/// The mesh only has to contain the orb; the surface itself comes from the ray march. The margin
/// past the glass is where the halo is drawn, so the edge can fade into the background and the glow
/// has room to spill out when the orb hears a voice.
const BOUNDING_MARGIN: f32 = 1.4;

const SWIRL_PER_SECOND: (f32, f32) = (0.25, 2.60);
const DRIFT_PER_SECOND: (f32, f32) = (0.08, 0.95);

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct OrbMaterial {
    #[uniform(0)]
    pub rotation: Mat4,
    #[uniform(0)]
    pub swirl: f32,
    #[uniform(0)]
    pub drift: f32,
    #[uniform(0)]
    pub activity: f32,
    #[uniform(0)]
    pub radius: f32,
    /// How far the image has materialised, from hidden at zero to fully shown at one.
    #[uniform(0)]
    pub reveal: f32,
    /// How brightly the orb is lit by the voice it is hearing, from dark at zero to full at one.
    #[uniform(0)]
    pub glow: f32,
    #[texture(1)]
    #[sampler(2)]
    pub image: Option<Handle<Image>>,
}

impl Material for OrbMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/orb.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

pub fn spawn_orb(
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OrbMaterial>>,
    mut commands: Commands,
) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(ORB_RADIUS * BOUNDING_MARGIN).mesh().ico(4).unwrap())),
        MeshMaterial3d(materials.add(OrbMaterial {
            rotation: Mat4::IDENTITY,
            swirl: 0.0,
            drift: 0.0,
            activity: 0.0,
            radius: ORB_RADIUS,
            reveal: 0.0,
            glow: 0.0,
            image: Some(asset_server.load(DEMO_IMAGE)),
        })),
    ));
}

/// The two phases advance at their own rate rather than being multiplied by activity directly, so
/// a change in activity changes how fast the light moves without making it jump.
pub fn animate_orb(
    time: Res<Time>,
    activity: Res<Activity>,
    reveal: Res<Reveal>,
    glow: Res<Glow>,
    spin: Res<Spin>,
    orbs: Query<&MeshMaterial3d<OrbMaterial>>,
    mut materials: ResMut<Assets<OrbMaterial>>,
    mut swirl: Local<f32>,
    mut drift: Local<f32>,
) {
    let eased = activity.eased;
    let lerp = |range: (f32, f32)| range.0 + (range.1 - range.0) * eased;
    *swirl += lerp(SWIRL_PER_SECOND) * time.delta_secs();
    *drift += lerp(DRIFT_PER_SECOND) * time.delta_secs();

    for handle in orbs.iter() {
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.rotation = Mat4::from_quat(spin.orientation().inverse());
        material.swirl = *swirl;
        material.drift = *drift;
        material.activity = eased;
        material.reveal = reveal.amount;
        material.glow = glow.0;
    }
}
