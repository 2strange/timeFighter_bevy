//! Making speed visible: air streaks streaming past the camera and a slight
//! buffeting of the camera, both fading in with airspeed.
//!
//! Streaks matter most where the ground is far away (high up, in the sky):
//! there the world barely moves on screen, and the streaks are the only
//! close-up reference. They are thin unlit boxes, a hundred or so, recycled
//! in a box ahead of the camera; one shared material, so cheap on WebGL2.

use bevy::{light::NotShadowCaster, prelude::*};

use super::{CameraSystems, ChaseCamera, ChaseCameraConfig};
use crate::flight::{FlightConfig, FlightState, PlayerAircraft};

/// Tuning of the speed effects.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct SpeedFxConfig {
    /// Number of streaks around the camera; 0 turns them off.
    pub streaks: usize,
    /// Half size of the region the streaks fill, in camera space (m):
    /// sideways, up/down, and how far ahead they appear.
    pub region: Vec3,
    /// No streak closer than this to the line of sight (m), so none
    /// crosses the jet.
    pub clear_radius: f32,
    /// Streak length per m/s of airspeed (s): 500 m/s gives 15 m.
    pub length_per_speed: f32,
    /// Streak opacity at top speed.
    pub max_alpha: f32,
    /// Speed blend (0 = slowest, 1 = fastest) where streaks and buffeting
    /// start to fade in.
    pub fade_in_from: f32,
    /// Camera buffeting at top speed: position (m) and roll (rad).
    pub shake_offset: f32,
    pub shake_roll: f32,
}

impl Default for SpeedFxConfig {
    fn default() -> Self {
        Self {
            streaks: 120,
            region: Vec3::new(45.0, 30.0, 160.0),
            clear_radius: 7.0,
            length_per_speed: 0.03,
            max_alpha: 0.35,
            fade_in_from: 0.3,
            shake_offset: 0.06,
            shake_roll: 0.15_f32.to_radians(),
        }
    }
}

impl SpeedFxConfig {
    /// 0 below `fade_in_from`, rising smoothly to 1 at top speed.
    pub fn intensity(&self, speed_blend: f32) -> f32 {
        let x = ((speed_blend - self.fade_in_from) / (1.0 - self.fade_in_from).max(f32::EPSILON))
            .clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }
}

/// Camera buffeting at time `t` for `intensity` (0..1): sideways/up offset
/// in camera space and roll. Two unrelated frequencies per axis, so it does
/// not look like a regular wobble.
pub fn shake(cfg: &SpeedFxConfig, intensity: f32, t: f32) -> (Vec2, f32) {
    let wave = |a: f32, b: f32, phase: f32| {
        0.6 * (t * a + phase).sin() + 0.4 * (t * b + 1.7 * phase).sin()
    };
    let strength = intensity * intensity;
    (
        Vec2::new(wave(17.0, 29.0, 0.3), wave(13.0, 23.0, 1.1)) * cfg.shake_offset * strength,
        wave(11.0, 19.0, 2.3) * cfg.shake_roll * strength,
    )
}

#[derive(Component)]
pub(super) struct SpeedStreak;

#[derive(Resource)]
pub(super) struct StreakMaterial(Handle<StandardMaterial>);

pub(super) struct SpeedFxPlugin;

impl Plugin for SpeedFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpeedFxConfig>()
            .add_systems(Startup, spawn_streaks)
            .add_systems(
                Update,
                (shake_camera, move_streaks).chain().after(CameraSystems),
            );
    }
}

fn spawn_streaks(
    mut commands: Commands,
    cfg: Res<SpeedFxConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Cuboid::new(0.05, 0.05, 1.0));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        fog_enabled: false,
        ..default()
    });
    commands.insert_resource(StreakMaterial(material.clone()));
    for _ in 0..cfg.streaks {
        commands.spawn((
            SpeedStreak,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            NotShadowCaster,
            // Far away and hidden until the first update places them.
            Transform::from_xyz(0.0, -10_000.0, 0.0),
            Visibility::Hidden,
        ));
    }
}

fn speed_blend(cam: &ChaseCameraConfig, flight: &FlightConfig, speed: f32) -> f32 {
    cam.speed_blend(flight, speed)
}

fn shake_camera(
    cfg: Res<SpeedFxConfig>,
    chase: Res<ChaseCameraConfig>,
    flight: Res<FlightConfig>,
    time: Res<Time>,
    player: Query<&FlightState, With<PlayerAircraft>>,
    mut camera: Query<&mut Transform, With<ChaseCamera>>,
) {
    let (Ok(state), Ok(mut cam)) = (player.single(), camera.single_mut()) else {
        return;
    };
    let intensity = cfg.intensity(speed_blend(&chase, &flight, state.speed));
    if intensity == 0.0 {
        return;
    }
    let (offset, roll) = shake(&cfg, intensity, time.elapsed_secs());
    let right = cam.right();
    let up = cam.up();
    cam.translation += right * offset.x + up * offset.y;
    cam.rotate_local_z(roll);
}

/// Simple deterministic random numbers for streak placement.
#[derive(Default)]
pub(super) struct StreakRng(u32);

impl StreakRng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A point in the region ahead of the camera, outside the clear radius
    /// around the line of sight (camera space, −Z ahead).
    fn ahead(&mut self, cfg: &SpeedFxConfig, near: bool) -> Vec3 {
        loop {
            let x = (self.next() * 2.0 - 1.0) * cfg.region.x;
            let y = (self.next() * 2.0 - 1.0) * cfg.region.y;
            if Vec2::new(x, y).length() < cfg.clear_radius {
                continue;
            }
            // New streaks appear far ahead; the first fill spreads them out.
            let depth = if near {
                self.next()
            } else {
                0.7 + 0.3 * self.next()
            };
            return Vec3::new(x, y, -depth * cfg.region.z);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn move_streaks(
    cfg: Res<SpeedFxConfig>,
    chase: Res<ChaseCameraConfig>,
    flight: Res<FlightConfig>,
    material: Option<Res<StreakMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: Local<StreakRng>,
    player: Query<&FlightState, With<PlayerAircraft>>,
    camera: Query<&Transform, (With<ChaseCamera>, Without<SpeedStreak>)>,
    mut streaks: Query<(&mut Transform, &mut Visibility), With<SpeedStreak>>,
) {
    let (Ok(state), Ok(cam), Some(material)) = (player.single(), camera.single(), material) else {
        return;
    };
    let intensity = cfg.intensity(speed_blend(&chase, &flight, state.speed));
    if let Some(mut m) = materials.get_mut(&material.0) {
        let alpha = cfg.max_alpha * intensity;
        if (m.base_color.alpha() - alpha).abs() > 0.005 {
            m.base_color.set_alpha(alpha);
        }
    }
    let visible = intensity > 0.0;
    let direction = state.velocity.try_normalize().unwrap_or(*cam.forward());
    let length = (state.speed * cfg.length_per_speed).max(0.1);
    let to_camera = cam.compute_affine().inverse();

    for (mut transform, mut visibility) in &mut streaks {
        let wanted = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(wanted);
        if !visible {
            continue;
        }
        let local = to_camera.transform_point3(transform.translation);
        let lost = local.z > 2.0
            || local.z < -cfg.region.z * 1.2
            || local.x.abs() > cfg.region.x * 1.2
            || local.y.abs() > cfg.region.y * 1.2;
        if lost {
            let first_fill = transform.translation.y < -5_000.0;
            let at = cam.transform_point(rng.ahead(&cfg, first_fill));
            transform.translation = at;
        }
        let up = if direction.y.abs() > 0.99 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        transform.rotation = Transform::IDENTITY.looking_to(direction, up).rotation;
        transform.scale = Vec3::new(1.0, 1.0, length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GamePlugin, test_support::headless_app};

    #[test]
    fn effects_fade_in_with_speed() {
        let cfg = SpeedFxConfig::default();
        assert_eq!(cfg.intensity(0.0), 0.0);
        assert_eq!(cfg.intensity(cfg.fade_in_from), 0.0);
        assert_eq!(cfg.intensity(1.0), 1.0);
        let mut previous = 0.0;
        for i in 0..=100 {
            let now = cfg.intensity(i as f32 / 100.0);
            assert!(now >= previous);
            previous = now;
        }
    }

    #[test]
    fn shake_is_off_when_slow_and_small_when_fast() {
        let cfg = SpeedFxConfig::default();
        for i in 0..200 {
            let t = i as f32 * 0.037;
            assert_eq!(shake(&cfg, 0.0, t), (Vec2::ZERO, 0.0));
            let (offset, roll) = shake(&cfg, 1.0, t);
            assert!(offset.x.abs() <= cfg.shake_offset && offset.y.abs() <= cfg.shake_offset);
            assert!(roll.abs() <= cfg.shake_roll);
        }
    }

    fn set_speed(app: &mut App, speed: f32) {
        let mut q = app
            .world_mut()
            .query_filtered::<&mut FlightState, With<PlayerAircraft>>();
        let mut state = q.single_mut(app.world_mut()).unwrap();
        state.speed = speed;
    }

    fn streaks(app: &mut App) -> Vec<(Transform, Visibility)> {
        app.world_mut()
            .query_filtered::<(&Transform, &Visibility), With<SpeedStreak>>()
            .iter(app.world())
            .map(|(t, v)| (*t, *v))
            .collect()
    }

    #[test]
    fn streaks_appear_ahead_of_the_camera_only_when_fast() {
        let mut app = headless_app(GamePlugin);
        app.update();
        let flight = app.world().resource::<FlightConfig>().clone();
        let cfg = app.world().resource::<SpeedFxConfig>().clone();

        set_speed(&mut app, flight.speed_min);
        app.update();
        let slow = streaks(&mut app);
        assert_eq!(slow.len(), cfg.streaks);
        assert!(slow.iter().all(|(_, v)| *v == Visibility::Hidden));

        set_speed(&mut app, flight.speed_max);
        app.update();
        let camera = *app
            .world_mut()
            .query_filtered::<&Transform, With<ChaseCamera>>()
            .single(app.world())
            .unwrap();
        let to_camera = camera.compute_affine().inverse();
        for (transform, visibility) in streaks(&mut app) {
            assert_eq!(visibility, Visibility::Visible);
            let local = to_camera.transform_point3(transform.translation);
            assert!(local.z < 0.0, "streak behind the camera: {local}");
            assert!(
                local.truncate().length() >= cfg.clear_radius * 0.99,
                "streak on the line of sight"
            );
            assert!(
                transform.scale.z > 10.0,
                "streak not stretched at top speed"
            );
        }
        let material = app.world().resource::<StreakMaterial>().0.clone();
        let alpha = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .base_color
            .alpha();
        assert!((alpha - cfg.max_alpha).abs() < 0.01);
    }
}
