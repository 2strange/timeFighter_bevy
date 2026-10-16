//! Chase camera behind the player's jet, and the effects that make its
//! speed visible.

mod speed_fx;

use bevy::prelude::*;

pub use speed_fx::{SpeedFxConfig, shake};

use crate::{
    flight::{FlightConfig, FlightState, FlightSystems, PlayerAircraft},
    world::{Target, WEB_CAMERA_FAR},
};

/// Native far plane (m): well beyond the fog, nothing is culled early.
pub const NATIVE_CAMERA_FAR: f32 = 30_000.0;

/// Chase camera tuning, in the jet's local frame (metres, +Z = behind).
///
/// Distance and field of view follow the airspeed: close and narrow at
/// minimum speed, far and slightly wider at top speed, blended with a
/// smoothstep so throttle changes never make the camera jump.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ChaseCameraConfig {
    /// Camera position relative to the jet at minimum airspeed.
    pub slow_offset: Vec3,
    /// Camera position relative to the jet at maximum airspeed.
    pub fast_offset: Vec3,
    /// Point the camera looks at, relative to the jet.
    pub look_at: Vec3,
    /// Fraction of the remaining rotation lag left after one second.
    /// Smaller = stiffer camera. The earlier prototype used 0.01.
    pub lag_base: f32,
    /// Vertical field of view at minimum airspeed (rad).
    pub fov: f32,
    /// Extra field of view at maximum airspeed (rad). A few degrees make the
    /// ground stream past the screen edges faster; 0 turns it off.
    pub fov_speed_boost: f32,
    /// Far clipping plane (m). On the web it sits just behind the fog's
    /// visibility, so frustum culling skips what the fog hides anyway (the
    /// web build has no per-chunk visibility ranges); see
    /// `world::max_fog_visibility`.
    pub far: f32,
}

impl Default for ChaseCameraConfig {
    fn default() -> Self {
        Self::for_target(Target::CURRENT)
    }
}

impl ChaseCameraConfig {
    // Starting values, tuned on test flights. For a 19 m jet the slow camera
    // sits one jet length behind, just above the fins; at top speed it falls
    // back to about two and a half jet lengths, so the jet seems to pull
    // away when accelerating. The look-at point 30 m ahead keeps the nose
    // below the screen centre.
    pub fn for_target(target: Target) -> Self {
        Self {
            slow_offset: Vec3::new(0.0, 4.5, 21.0),
            fast_offset: Vec3::new(0.0, 7.0, 46.5),
            look_at: Vec3::new(0.0, 3.0, -30.0),
            lag_base: 0.01,
            fov: 60_f32.to_radians(),
            // Austin barely felt the difference between slow and fast with
            // 5 degrees; 12 makes the ground rush past the edges at speed.
            fov_speed_boost: 12_f32.to_radians(),
            far: match target {
                Target::Web => WEB_CAMERA_FAR,
                Target::Native => NATIVE_CAMERA_FAR,
            },
        }
    }
}

impl ChaseCameraConfig {
    /// 0 at minimum airspeed, 1 at top speed, smoothstepped.
    pub fn speed_blend(&self, flight: &FlightConfig, speed: f32) -> f32 {
        let range = (flight.speed_max - flight.speed_min).max(f32::EPSILON);
        let x = ((speed - flight.speed_min) / range).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }

    /// Camera offset and vertical field of view for an airspeed.
    pub fn framing(&self, flight: &FlightConfig, speed: f32) -> (Vec3, f32) {
        let blend = self.speed_blend(flight, speed);
        (
            self.slow_offset.lerp(self.fast_offset, blend),
            self.fov + self.fov_speed_boost * blend,
        )
    }
}

/// Systems that place the camera. Anything overriding the camera view
/// (e.g. the showroom) runs after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraSystems;

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
            .add_plugins(speed_fx::SpeedFxPlugin)
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                follow_player
                    .in_set(CameraSystems)
                    .after(FlightSystems::Simulate),
            );
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
            far: cfg.far,
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
    flight: Res<FlightConfig>,
    time: Res<Time>,
    player: Query<(&Transform, &FlightState), With<PlayerAircraft>>,
    mut camera: Query<(&mut ChaseCamera, &mut Transform, &mut Projection), Without<PlayerAircraft>>,
) {
    let (Ok((jet, state)), Ok((mut chase, mut cam, mut projection))) =
        (player.single(), camera.single_mut())
    else {
        return;
    };

    let t = 1.0 - cfg.lag_base.powf(time.delta_secs());
    let rig = match chase.rig_rotation {
        Some(previous) => previous.slerp(jet.rotation, t),
        None => jet.rotation,
    };
    chase.rig_rotation = Some(rig);

    let (offset, fov) = cfg.framing(&flight, state.speed);
    cam.translation = jet.translation + rig * offset;
    let target = jet.translation + rig * cfg.look_at;
    cam.look_at(target, rig * Vec3::Y);
    if let Projection::Perspective(perspective) = projection.as_mut() {
        perspective.fov = fov;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::flight::{FlightInput, FlightPlugin};

    #[test]
    fn far_plane_per_target() {
        let web = ChaseCameraConfig::for_target(Target::Web);
        let native = ChaseCameraConfig::for_target(Target::Native);
        assert_eq!(web.far, WEB_CAMERA_FAR);
        assert_eq!(native.far, NATIVE_CAMERA_FAR);
        // Behind the farthest fog of any lighting preset, so nothing the fog
        // still shows is clipped; native keeps its long range.
        let fog = crate::world::max_fog_visibility();
        assert!(web.far > fog && web.far < native.far);
        assert!(native.far > fog);
        // Everything else is the same on both targets.
        assert_eq!(
            ChaseCameraConfig { far: 0.0, ..web },
            ChaseCameraConfig { far: 0.0, ..native }
        );
    }

    #[test]
    fn far_plane_reaches_the_camera() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ChaseCameraConfig::for_target(Target::Web))
            .add_systems(Startup, spawn_camera);
        app.update();
        let mut projections = app.world_mut().query::<&Projection>();
        let Projection::Perspective(p) = projections.single(app.world()).unwrap() else {
            panic!("chase camera must be perspective");
        };
        assert_eq!(p.far, WEB_CAMERA_FAR);
    }

    #[test]
    fn framing_spans_slow_to_fast_without_jumps() {
        let cam = ChaseCameraConfig::default();
        let flight = FlightConfig::default();

        let (slow, slow_fov) = cam.framing(&flight, flight.speed_min);
        let (fast, fast_fov) = cam.framing(&flight, flight.speed_max);
        assert_eq!((slow, slow_fov), (cam.slow_offset, cam.fov));
        assert_eq!(
            (fast, fast_fov),
            (cam.fast_offset, cam.fov + cam.fov_speed_boost)
        );
        // Never further out than at top speed, even beyond the speed range.
        assert_eq!(
            cam.framing(&flight, flight.speed_max * 2.0).0,
            cam.fast_offset
        );
        // Clearly closer when slow.
        assert!(slow.length() < 0.6 * fast.length());

        // Monotonic and continuous: 1 m/s more speed moves the camera by
        // well under 10 cm and widens the view by well under 0.05°.
        let mut previous = cam.framing(&flight, flight.speed_min);
        let mut speed = flight.speed_min;
        while speed < flight.speed_max {
            speed += 1.0;
            let next = cam.framing(&flight, speed);
            assert!(next.0.length() >= previous.0.length());
            assert!(next.1 >= previous.1);
            assert!(next.0.distance(previous.0) < 0.1);
            assert!((next.1 - previous.1) < 0.05_f32.to_radians());
            previous = next;
        }
    }

    #[test]
    fn camera_moves_back_smoothly_while_accelerating() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, FlightPlugin))
            .init_resource::<ChaseCameraConfig>()
            .add_systems(Update, follow_player.after(FlightSystems::Simulate))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )));
        let flight = app.world().resource::<FlightConfig>().clone();
        let jet = app
            .world_mut()
            .spawn((
                PlayerAircraft,
                FlightInput {
                    throttle: 1.0,
                    ..default()
                },
                FlightState::new(&flight),
                flight.spawn_transform(),
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                ChaseCamera::default(),
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
            ))
            .id();

        let distance = |app: &App| {
            let jet = app.world().get::<Transform>(jet).unwrap().translation;
            app.world()
                .get::<Transform>(camera)
                .unwrap()
                .translation
                .distance(jet)
        };
        let fov = |app: &App| match app.world().get::<Projection>(camera).unwrap() {
            Projection::Perspective(p) => p.fov,
            _ => unreachable!(),
        };

        app.update();
        app.update();
        let start = distance(&app);
        assert!((start - ChaseCameraConfig::default().slow_offset.length()).abs() < 1.0);

        // Full throttle from minimum to maximum speed: about five seconds.
        let mut previous = start;
        for _ in 0..(6 * 60) {
            app.update();
            let now = distance(&app);
            assert!(now >= previous - 1e-3, "camera moved in while accelerating");
            assert!(
                now - previous < 0.2,
                "camera jumped {} m in one frame",
                now - previous
            );
            previous = now;
        }
        let cam = ChaseCameraConfig::default();
        assert!((previous - cam.fast_offset.length()).abs() < 0.5);
        assert!((fov(&app) - (cam.fov + cam.fov_speed_boost)).abs() < 1e-4);
    }
}
