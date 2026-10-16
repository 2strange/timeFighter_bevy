//! Chase camera behind the player's jet.

use bevy::prelude::*;

use crate::flight::{FlightState, FlightSystems, PlayerAircraft};

/// Chase camera tuning, in the jet's local frame (metres, +Z = behind).
#[derive(Resource, Clone, Debug)]
pub struct ChaseCameraConfig {
    /// Camera position relative to the jet.
    pub offset: Vec3,
    /// Point the camera looks at, relative to the jet.
    pub look_at: Vec3,
    /// Extra distance behind the jet per m/s of airspeed, for a sense of speed.
    pub pullback_per_speed: f32,
    /// Fraction of the remaining rotation lag left after one second.
    /// Smaller = stiffer camera. The earlier prototype used 0.01.
    pub lag_base: f32,
    /// Vertical field of view (rad).
    pub fov: f32,
}

impl Default for ChaseCameraConfig {
    // Starting values, to be tuned on test flights. The prototype's offset
    // (8 m up, 20 m back) suited a differently scaled model; for a 19 m jet
    // the camera sits about two jet lengths behind, high enough to see over
    // the canopy, and looks at a point 30 m ahead so the nose stays below the
    // screen centre. The pullback adds 12.5 m at top speed, so speed reads as
    // the jet pulling away a little instead of just the ground rushing by.
    fn default() -> Self {
        Self {
            offset: Vec3::new(0.0, 7.0, 34.0),
            look_at: Vec3::new(0.0, 3.0, -30.0),
            pullback_per_speed: 0.025,
            lag_base: 0.01,
            fov: 60_f32.to_radians(),
        }
    }
}

/// Marks the camera that follows the player.
#[derive(Component, Debug, Default)]
pub struct ChaseCamera {
    /// Smoothed copy of the jet's rotation the camera hangs from.
    rig_rotation: Option<Quat>,
}

pub struct ChaseCameraPlugin;

impl Plugin for ChaseCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChaseCameraConfig>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, follow_player.after(FlightSystems::Simulate));
    }
}

fn spawn_camera(mut commands: Commands, cfg: Res<ChaseCameraConfig>) {
    commands.spawn((
        Name::new("Chase camera"),
        ChaseCamera::default(),
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: cfg.fov,
            near: 0.5,
            far: 30_000.0,
            ..default()
        }),
        crate::world::sky_fog(),
    ));
}

/// The camera position is attached rigidly to a smoothed copy of the jet's
/// rotation, so it swings out in turns but never trails behind at high speed
/// (the prototype smoothed the position itself, which let the jet run away
/// from the camera at full throttle).
fn follow_player(
    cfg: Res<ChaseCameraConfig>,
    time: Res<Time>,
    player: Query<(&Transform, &FlightState), With<PlayerAircraft>>,
    mut camera: Query<(&mut ChaseCamera, &mut Transform), Without<PlayerAircraft>>,
) {
    let (Ok((jet, state)), Ok((mut chase, mut cam))) = (player.single(), camera.single_mut())
    else {
        return;
    };

    let t = 1.0 - cfg.lag_base.powf(time.delta_secs());
    let rig = match chase.rig_rotation {
        Some(previous) => previous.slerp(jet.rotation, t),
        None => jet.rotation,
    };
    chase.rig_rotation = Some(rig);

    let offset = cfg.offset + Vec3::Z * cfg.pullback_per_speed * state.speed;
    cam.translation = jet.translation + rig * offset;
    let target = jet.translation + rig * cfg.look_at;
    cam.look_at(target, rig * Vec3::Y);
}
