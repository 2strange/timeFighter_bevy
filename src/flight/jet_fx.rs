//! Engine flames and wing-tip vortex trails of the player's jet.
//!
//! Both are cheap on WebGL2: the flames are a few unlit additive cones that
//! grow with the throttle, the vortices are two short ribbon meshes rebuilt
//! each frame from the last second of wing-tip positions, white and fading,
//! visible when the jet pulls hard.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::{FlightConfig, FlightInput, FlightState, PlayerAircraft};
use crate::aircraft::AircraftModel;

/// Where the effects attach on the jet, in its local frame (m).
#[derive(Clone, Debug, PartialEq)]
pub struct JetFxPoints {
    /// Centre of each engine nozzle's exit.
    pub nozzles: Vec<Vec3>,
    /// Nozzle exit radius.
    pub nozzle_radius: f32,
    /// Wing tips, left then right.
    pub wing_tips: [Vec3; 2],
}

impl JetFxPoints {
    /// Measured on the normalised F-14 (swept wings): two nozzles side by
    /// side ending at z = 7.55, wing tips at the trailing edge.
    pub fn f14() -> Self {
        Self {
            nozzles: vec![Vec3::new(-1.3, -0.65, 7.6), Vec3::new(1.3, -0.65, 7.6)],
            nozzle_radius: 0.45,
            wing_tips: [Vec3::new(-6.84, 0.08, 7.5), Vec3::new(6.84, 0.08, 7.5)],
        }
    }

    /// The built-in primitive jet: one exhaust at the tail, 13 m span.
    pub fn primitive_jet() -> Self {
        Self {
            nozzles: vec![Vec3::new(0.0, 0.0, 9.2)],
            nozzle_radius: 0.85,
            wing_tips: [Vec3::new(-6.5, -0.2, 3.5), Vec3::new(6.5, -0.2, 3.5)],
        }
    }

    pub fn for_model(model: Option<AircraftModel>) -> Self {
        match model {
            Some(AircraftModel::F14Tomcat) => Self::f14(),
            Some(other) => {
                let size = other.size();
                Self {
                    nozzles: vec![Vec3::new(0.0, 0.0, size.max.z)],
                    nozzle_radius: 0.5,
                    wing_tips: [
                        Vec3::new(size.min.x, 0.0, 0.1 * size.length()),
                        Vec3::new(size.max.x, 0.0, 0.1 * size.length()),
                    ],
                }
            }
            None => Self::primitive_jet(),
        }
    }
}

/// Tuning of the jet effects.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct JetFxConfig {
    /// Flame length (m) at minimum and at maximum speed.
    pub flame_min: f32,
    pub flame_max: f32,
    /// Extra flame length while the throttle is pushed (afterburner).
    pub flame_boost: f32,
    /// How long a vortex trail is (s of flight).
    pub trail_seconds: f32,
    /// Trail ribbon width (m).
    pub trail_width: f32,
    /// Trail opacity at full strength.
    pub trail_alpha: f32,
}

impl Default for JetFxConfig {
    fn default() -> Self {
        Self {
            flame_min: 0.6,
            flame_max: 4.0,
            flame_boost: 2.5,
            trail_seconds: 0.8,
            trail_width: 0.35,
            trail_alpha: 0.55,
        }
    }
}

/// How strongly the tips leave vortices (0..1): needs a hard pull (pitch,
/// partly yaw) and grows with speed; straight flight leaves none.
pub fn vortex_strength(flight: &FlightConfig, state: &FlightState) -> f32 {
    let pull = (state.pitch_rate.abs() / flight.max_pitch_rate)
        .max(0.6 * state.yaw_rate.abs() / flight.max_yaw_rate);
    let speed = (state.speed / flight.speed_max).clamp(0.0, 1.0);
    let load = pull * (0.4 + 0.6 * speed);
    let x = ((load - 0.15) / 0.55).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Flame length (m) for the current speed and throttle.
pub fn flame_length(
    cfg: &JetFxConfig,
    flight: &FlightConfig,
    state: &FlightState,
    input: &FlightInput,
) -> f32 {
    let range = (flight.speed_max - flight.speed_min).max(f32::EPSILON);
    let speed = ((state.speed - flight.speed_min) / range).clamp(0.0, 1.0);
    cfg.flame_min
        + (cfg.flame_max - cfg.flame_min) * speed
        + cfg.flame_boost * input.throttle.max(0.0)
}

/// A flame cone; its base stays on the nozzle exit as it grows.
#[derive(Component)]
pub(super) struct EngineFlame {
    nozzle: Vec3,
}

/// One wing tip's recent positions (world space) with their age and strength.
#[derive(Component, Default)]
pub(super) struct VortexTrail {
    tip: usize,
    samples: Vec<(Vec3, f32, f32)>,
}

pub(super) struct JetFxPlugin;

impl Plugin for JetFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JetFxConfig>()
            .add_systems(Startup, spawn_jet_fx.after(super::jet::spawn_player_jet))
            .add_systems(
                Update,
                (update_flames, update_trails).after(super::FlightSystems::Simulate),
            );
    }
}

fn spawn_jet_fx(
    mut commands: Commands,
    flight: Res<FlightConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player: Query<Entity, With<PlayerAircraft>>,
) {
    let Ok(jet) = player.single() else {
        return;
    };
    let points = JetFxPoints::for_model(flight.player_model);

    // Flames: a cone pointing backwards (+Z), base at the nozzle exit;
    // length set per frame by scaling, so the mesh is a 1 m unit cone.
    let flame_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.55, 0.2, 0.9),
        emissive: LinearRgba::rgb(6.0, 2.2, 0.6),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    let cone = meshes.add(Cone::new(1.0, 1.0));
    let pointing_back = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    for nozzle in &points.nozzles {
        commands.entity(jet).with_child((
            EngineFlame { nozzle: *nozzle },
            Mesh3d(cone.clone()),
            MeshMaterial3d(flame_material.clone()),
            NotShadowCaster,
            Transform::from_translation(*nozzle)
                .with_rotation(pointing_back)
                .with_scale(Vec3::new(points.nozzle_radius, 1.0, points.nozzle_radius)),
        ));
    }

    // Vortex ribbons, one mesh per wing tip, in world space.
    let trail_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    for tip in 0..2 {
        commands.spawn((
            VortexTrail {
                tip,
                samples: Vec::new(),
            },
            Mesh3d(meshes.add(empty_ribbon())),
            MeshMaterial3d(trail_material.clone()),
            NotShadowCaster,
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

fn empty_ribbon() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
    .with_inserted_indices(Indices::U32(Vec::new()))
}

fn update_flames(
    cfg: Res<JetFxConfig>,
    flight: Res<FlightConfig>,
    time: Res<Time>,
    player: Query<(&FlightState, &FlightInput), With<PlayerAircraft>>,
    mut flames: Query<(&mut Transform, &EngineFlame)>,
) {
    let Ok((state, input)) = player.single() else {
        return;
    };
    let length = flame_length(&cfg, &flight, state, input);
    // A little flicker, so the flame looks alive.
    let t = time.elapsed_secs();
    let flicker = 1.0 + 0.06 * (t * 37.0).sin() + 0.04 * (t * 61.0).sin();
    for (mut transform, flame) in &mut flames {
        let scaled = length * flicker;
        transform.scale.y = scaled;
        // The unit cone is centred on its middle: put the base on the nozzle.
        transform.translation = flame.nozzle + Vec3::Z * (scaled / 2.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_trails(
    cfg: Res<JetFxConfig>,
    flight: Res<FlightConfig>,
    time: Res<Time>,
    player: Query<(&Transform, &FlightState), With<PlayerAircraft>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut trails: Query<(&mut VortexTrail, &Mesh3d, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok((jet, state)) = player.single() else {
        return;
    };
    let now = time.elapsed_secs();
    let strength = vortex_strength(&flight, state);
    let tips = JetFxPoints::for_model(flight.player_model).wing_tips;
    let eye = camera.iter().next().map(|c| c.translation());

    for (mut trail, mesh, mut visibility) in &mut trails {
        let tip = jet.transform_point(tips[trail.tip]);
        if time.delta_secs() > 0.0 {
            trail.samples.push((tip, now, strength));
        }
        trail
            .samples
            .retain(|(_, at, _)| now - at <= cfg.trail_seconds);
        let any = trail.samples.iter().any(|(_, _, s)| *s > 0.01);
        visibility.set_if_neq(if any {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
        if !any {
            continue;
        }
        let Some(mut mesh) = meshes.get_mut(&mesh.0) else {
            continue;
        };
        build_ribbon(&mut mesh, &trail.samples, now, &cfg, eye);
    }
}

/// Rebuilds a ribbon through `samples`, facing `eye`, fading with age.
fn build_ribbon(
    mesh: &mut Mesh,
    samples: &[(Vec3, f32, f32)],
    now: f32,
    cfg: &JetFxConfig,
    eye: Option<Vec3>,
) {
    let mut positions = Vec::with_capacity(samples.len() * 2);
    let mut colors = Vec::with_capacity(samples.len() * 2);
    for (i, &(point, at, strength)) in samples.iter().enumerate() {
        let next = samples
            .get(i + 1)
            .or(samples.get(i.wrapping_sub(1)))
            .map_or(point, |s| s.0);
        let along = (next - point).try_normalize().unwrap_or(Vec3::Z);
        let to_eye = eye.map_or(Vec3::Y, |e| (e - point).normalize_or(Vec3::Y));
        let side = along.cross(to_eye).try_normalize().unwrap_or(Vec3::X) * cfg.trail_width / 2.0;
        let fade = 1.0 - ((now - at) / cfg.trail_seconds).clamp(0.0, 1.0);
        let alpha = cfg.trail_alpha * strength * fade;
        for p in [point - side, point + side] {
            positions.push(p.to_array());
            colors.push([1.0, 1.0, 1.0, alpha]);
        }
    }
    let mut indices = Vec::with_capacity(samples.len().saturating_sub(1) * 6);
    for i in 0..samples.len().saturating_sub(1) as u32 {
        let a = i * 2;
        indices.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GamePlugin, test_support::headless_app};

    fn state(speed: f32, pitch_rate: f32) -> FlightState {
        FlightState {
            speed,
            pitch_rate,
            ..default()
        }
    }

    #[test]
    fn vortices_need_a_hard_pull_and_grow_with_speed() {
        let f = FlightConfig::default();
        assert_eq!(
            vortex_strength(&f, &state(f.speed_max, 0.0)),
            0.0,
            "straight flight"
        );
        let hard_fast = vortex_strength(&f, &state(f.speed_max, f.max_pitch_rate));
        let hard_slow = vortex_strength(&f, &state(f.speed_min, f.max_pitch_rate));
        let gentle_fast = vortex_strength(&f, &state(f.speed_max, 0.1 * f.max_pitch_rate));
        assert_eq!(hard_fast, 1.0);
        assert!(
            hard_slow < hard_fast && gentle_fast < 0.05,
            "{hard_slow} {gentle_fast}"
        );
    }

    #[test]
    fn flame_grows_with_speed_and_throttle() {
        let (cfg, f) = (JetFxConfig::default(), FlightConfig::default());
        let idle = FlightInput::default();
        let push = FlightInput {
            throttle: 1.0,
            ..default()
        };
        let slow = flame_length(&cfg, &f, &state(f.speed_min, 0.0), &idle);
        let fast = flame_length(&cfg, &f, &state(f.speed_max, 0.0), &idle);
        let burner = flame_length(&cfg, &f, &state(f.speed_max, 0.0), &push);
        assert!(slow < fast && fast < burner, "{slow} {fast} {burner}");
        assert_eq!(slow, cfg.flame_min);
    }

    fn player(app: &mut App) -> Entity {
        app.world_mut()
            .query_filtered::<Entity, With<PlayerAircraft>>()
            .single(app.world())
            .unwrap()
    }

    #[test]
    fn f14_gets_two_flames_that_follow_the_throttle() {
        let mut app = headless_app(GamePlugin);
        app.update();
        let flames: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<EngineFlame>>()
            .iter(app.world())
            .collect();
        assert_eq!(flames.len(), 2);
        let length = |app: &App| app.world().get::<Transform>(flames[0]).unwrap().scale.y;
        app.update();
        let idle = length(&app);
        let jet = player(&mut app);
        app.world_mut()
            .get_mut::<FlightInput>(jet)
            .unwrap()
            .throttle = 1.0;
        app.world_mut().get_mut::<FlightState>(jet).unwrap().speed = 450.0;
        app.update();
        assert!(length(&app) > idle * 2.0, "{idle} -> {}", length(&app));
    }

    #[test]
    fn vortex_trails_show_only_under_load() {
        let mut app = headless_app(GamePlugin);
        app.update();
        let jet = player(&mut app);
        let trails = |app: &mut App| {
            app.world_mut()
                .query::<(&VortexTrail, &Visibility, &Mesh3d)>()
                .iter(app.world())
                .map(|(t, v, m)| (t.samples.len(), *v, m.0.clone()))
                .collect::<Vec<_>>()
        };
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.update();
        }
        assert!(
            trails(&mut app)
                .iter()
                .all(|(_, v, _)| *v == Visibility::Hidden)
        );

        let max_pitch = app.world().resource::<FlightConfig>().max_pitch_rate;
        for _ in 0..8 {
            {
                let mut state = app.world_mut().get_mut::<FlightState>(jet).unwrap();
                state.speed = 450.0;
                state.pitch_rate = max_pitch;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.update();
        }
        let shown = trails(&mut app);
        assert_eq!(shown.len(), 2);
        for (samples, visibility, mesh) in shown {
            assert_eq!(visibility, Visibility::Visible);
            assert!(samples >= 2);
            let vertices = app
                .world()
                .resource::<Assets<Mesh>>()
                .get(&mesh)
                .unwrap()
                .count_vertices();
            assert_eq!(vertices, samples * 2);
        }
    }
}
