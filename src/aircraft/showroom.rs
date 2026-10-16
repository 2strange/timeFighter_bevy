//! Showroom: all aircraft models side by side with scale references, for
//! checking size and orientation by eye. Toggle with M; the game pauses.

use bevy::prelude::*;

use super::AircraftModel;
use crate::{camera::CameraSystems, game_state::Pause};

/// Where the showroom floats, well away from the flying area.
const ORIGIN: Vec3 = Vec3::new(0.0, 1_500.0, -1_500.0);
/// Distance between neighbouring models along X (m).
const SPACING: f32 = 26.0;
/// Floor top, just below the lowest model (origins sit on the fuselage axis).
const FLOOR_Y: f32 = -2.5;

#[derive(Resource, Debug)]
pub struct ShowroomState {
    pub active: bool,
    spawned: bool,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for ShowroomState {
    fn default() -> Self {
        Self {
            active: false,
            spawned: false,
            yaw: 0.5,
            pitch: 0.35,
            distance: 110.0,
        }
    }
}

#[derive(Component)]
struct ShowroomHelp;

pub struct ShowroomPlugin;

impl Plugin for ShowroomPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowroomState>()
            .add_systems(Startup, spawn_help)
            .add_systems(
                Update,
                (
                    toggle,
                    spawn_once,
                    orbit_camera
                        .after(CameraSystems)
                        .run_if(|state: Res<ShowroomState>| state.active),
                )
                    .chain(),
            );
    }
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ShowroomState>,
    mut pause: ResMut<Pause>,
    mut help: Query<&mut Visibility, With<ShowroomHelp>>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    state.active = !state.active;
    pause.showroom = state.active;
    for mut visibility in &mut help {
        *visibility = if state.active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// Builds the showroom the first time it is opened, so its models are only
/// downloaded when somebody looks at them.
fn spawn_once(
    mut commands: Commands,
    mut state: ResMut<ShowroomState>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if state.active && !state.spawned {
        spawn_showroom(&mut commands, &assets, &mut meshes, &mut materials);
        state.spawned = true;
    }
}

/// A/D or Left/Right orbit, W/S or Up/Down tilt, Q/E zoom.
fn orbit_camera(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut state: ResMut<ShowroomState>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let axis = |positive: &[KeyCode], negative: &[KeyCode]| -> f32 {
        f32::from(keys.any_pressed(positive.iter().copied()))
            - f32::from(keys.any_pressed(negative.iter().copied()))
    };
    state.yaw += axis(
        &[KeyCode::KeyD, KeyCode::ArrowRight],
        &[KeyCode::KeyA, KeyCode::ArrowLeft],
    ) * dt;
    state.pitch = (state.pitch
        + axis(
            &[KeyCode::KeyW, KeyCode::ArrowUp],
            &[KeyCode::KeyS, KeyCode::ArrowDown],
        ) * dt)
        .clamp(-0.2, 1.5);
    state.distance =
        (state.distance * (1.0 + axis(&[KeyCode::KeyE], &[KeyCode::KeyQ]) * dt)).clamp(15.0, 300.0);

    let direction = Vec3::new(
        state.yaw.sin() * state.pitch.cos(),
        state.pitch.sin(),
        -state.yaw.cos() * state.pitch.cos(),
    );
    for (mut transform, mut projection) in &mut camera {
        *transform = Transform::from_translation(ORIGIN + direction * state.distance)
            .looking_at(ORIGIN, Vec3::Y);
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = 45_f32.to_radians();
        }
    }
}

fn spawn_showroom(
    commands: &mut Commands,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mut part = |mesh: Mesh, color: Color, at: Vec3| {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(color)),
            Transform::from_translation(ORIGIN + at),
        ));
    };

    // Floor.
    part(
        Cuboid::new(140.0, 0.2, 60.0).mesh().build(),
        Color::srgb(0.32, 0.34, 0.36),
        Vec3::new(0.0, FLOOR_Y - 0.1, 0.0),
    );

    // Two 10 m bars in 1 m steps: along X in front of the noses, along Z at the left.
    let white = Color::srgb(0.95, 0.95, 0.95);
    let red = Color::srgb(0.85, 0.12, 0.1);
    for i in 0..10 {
        let color = if i % 2 == 0 { white } else { red };
        let step = i as f32 - 4.5;
        part(
            Cuboid::new(1.0, 0.25, 0.25).mesh().build(),
            color,
            Vec3::new(step, FLOOR_Y + 0.125, -18.0),
        );
        part(
            Cuboid::new(0.25, 0.25, 1.0).mesh().build(),
            color,
            Vec3::new(-62.0, FLOOR_Y + 0.125, step),
        );
    }

    // Axis cross at the right end: +X red, +Y green, +Z blue, 6 m each.
    let axes = Vec3::new(62.0, FLOOR_Y, 0.0);
    part(
        Cuboid::new(6.0, 0.3, 0.3).mesh().build(),
        Color::srgb(0.9, 0.1, 0.1),
        axes + Vec3::X * 3.0,
    );
    part(
        Cuboid::new(0.3, 6.0, 0.3).mesh().build(),
        Color::srgb(0.1, 0.8, 0.1),
        axes + Vec3::Y * 3.0,
    );
    part(
        Cuboid::new(0.3, 0.3, 6.0).mesh().build(),
        Color::srgb(0.1, 0.3, 0.95),
        axes + Vec3::Z * 3.0,
    );

    let first_x = -SPACING * (AircraftModel::ALL.len() as f32 - 1.0) / 2.0;
    for (i, model) in AircraftModel::ALL.into_iter().enumerate() {
        commands.spawn((
            Name::new(model.display_name()),
            model.scene(assets),
            Transform::from_translation(ORIGIN + Vec3::X * (first_x + i as f32 * SPACING)),
        ));
    }
}

fn spawn_help(mut commands: Commands) {
    let names: Vec<_> = AircraftModel::ALL
        .iter()
        .map(|m| m.display_name())
        .collect();
    commands.spawn((
        ShowroomHelp,
        Text::new(format!(
            "SHOWROOM (M to leave)\n\
             Left to right: {}\n\
             Red/white bars: 10 m in 1 m steps\n\
             Axes: X red, Y green, +Z blue. Noses must point away from blue (-Z).\n\
             A/D or Left/Right orbit   W/S or Up/Down tilt   Q/E zoom",
            names.join("  |  ")
        )),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(12),
            left: px(12),
            ..default()
        },
        Visibility::Hidden,
    ));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bevy::{
        asset::AssetMetaCheck,
        input::{
            ButtonState,
            keyboard::{Key, KeyboardInput, NativeKey},
        },
        log::LogPlugin,
        render::{RenderPlugin, settings::WgpuSettings},
        window::ExitCondition,
        winit::WinitPlugin,
    };

    use super::*;
    use crate::GamePlugin;

    /// The full game with all plugins, but no window and no GPU.
    fn headless_game() -> App {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                .disable::<LogPlugin>()
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(AssetPlugin {
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                }),
        )
        .add_plugins(GamePlugin);
        app.finish();
        app.cleanup();
        app
    }

    fn key(app: &mut App, key_code: KeyCode, state: ButtonState) {
        let window = Entity::PLACEHOLDER;
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
    }

    fn tap(app: &mut App, key_code: KeyCode) {
        key(app, key_code, ButtonState::Pressed);
        app.update();
        key(app, key_code, ButtonState::Released);
        app.update();
    }

    /// Number of entities with a mesh below `root`.
    fn meshes_below(world: &mut World, root: Entity) -> usize {
        let mut count = 0;
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if world.get::<Mesh3d>(entity).is_some() {
                count += 1;
            }
            if let Some(children) = world.get::<Children>(entity) {
                stack.extend(children.iter());
            }
        }
        count
    }

    fn camera(app: &mut App) -> Transform {
        let mut query = app.world_mut().query_filtered::<&Transform, With<Camera3d>>();
        *query.single(app.world()).unwrap()
    }

    #[test]
    fn showroom_shows_all_models_and_reacts_to_keys() {
        let mut app = headless_game();
        app.update();
        tap(&mut app, KeyCode::KeyM);
        assert!(app.world().resource::<ShowroomState>().active);

        let roots: Vec<(Entity, String)> = app
            .world_mut()
            .query_filtered::<(Entity, &Name), With<WorldAssetRoot>>()
            .iter(app.world())
            .map(|(e, n)| (e, n.to_string()))
            .collect();
        assert_eq!(roots.len(), AircraftModel::ALL.len(), "model roots spawned");

        // Loading runs on background threads; give it a few seconds.
        let start = Instant::now();
        loop {
            app.update();
            let missing: Vec<_> = roots
                .iter()
                .filter(|(e, _)| meshes_below(app.world_mut(), *e) == 0)
                .map(|(_, n)| n.clone())
                .collect();
            if missing.is_empty() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "no meshes spawned for {missing:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        // Orbit: holding D for a while must move the camera.
        let before = camera(&mut app);
        key(&mut app, KeyCode::KeyD, ButtonState::Pressed);
        for _ in 0..20 {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        key(&mut app, KeyCode::KeyD, ButtonState::Released);
        app.update();
        let after = camera(&mut app);
        assert!(
            before.translation.distance(after.translation) > 0.5,
            "camera did not orbit: {before:?} -> {after:?}"
        );
    }
}
