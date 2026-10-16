//! Aircraft showroom: all models side by side with scale references, for
//! checking size and orientation by eye. Runs as its own app mode (browser:
//! `showroom.html`), without world or flight.
//!
//! Mouse: drag to orbit, wheel to zoom. Keys: A/D or Left/Right orbit,
//! W/S or Up/Down tilt, Q/E zoom.

use bevy::{
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    light::CascadeShadowConfigBuilder,
    prelude::*,
    window::CursorMoved,
};

use super::AircraftModel;

/// Distance between neighbouring models along X (m).
const SPACING: f32 = 26.0;
/// Floor top, just below the lowest model (origins sit on the fuselage axis).
const FLOOR_Y: f32 = -2.5;
/// Orbit speed with keys (rad/s) and with the mouse (rad per pixel).
const KEY_ORBIT: f32 = 1.0;
const MOUSE_ORBIT: f32 = 0.005;
/// Zoom: fraction of the distance per second (keys), per wheel line, per pixel.
const KEY_ZOOM: f32 = 1.0;
const LINE_ZOOM: f32 = 0.1;
const PIXEL_ZOOM: f32 = 0.002;

/// Orbit camera around the middle of the row.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ShowroomView {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Default for ShowroomView {
    fn default() -> Self {
        Self {
            yaw: 0.5,
            pitch: 0.35,
            distance: 110.0,
        }
    }
}

impl ShowroomView {
    fn camera_transform(&self) -> Transform {
        let direction = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        );
        Transform::from_translation(direction * self.distance).looking_at(Vec3::ZERO, Vec3::Y)
    }
}

pub struct ShowroomPlugin;

impl Plugin for ShowroomPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowroomView>()
            .insert_resource(ClearColor(Color::srgb(0.62, 0.74, 0.86)))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.75, 0.82, 1.0),
                brightness: 600.0,
                ..default()
            })
            .add_systems(Startup, (spawn_showroom, spawn_help))
            .add_systems(Update, orbit_camera);
    }
}

fn orbit_camera(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut cursor: MessageReader<CursorMoved>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time<Real>>,
    mut view: ResMut<ShowroomView>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let axis = |positive: &[KeyCode], negative: &[KeyCode]| -> f32 {
        f32::from(keys.any_pressed(positive.iter().copied()))
            - f32::from(keys.any_pressed(negative.iter().copied()))
    };

    let mut yaw = axis(
        &[KeyCode::KeyD, KeyCode::ArrowRight],
        &[KeyCode::KeyA, KeyCode::ArrowLeft],
    ) * KEY_ORBIT
        * dt;
    let mut pitch = axis(
        &[KeyCode::KeyW, KeyCode::ArrowUp],
        &[KeyCode::KeyS, KeyCode::ArrowDown],
    ) * KEY_ORBIT
        * dt;
    // Cursor movement (not raw mouse motion): a drag should follow the
    // pointer, and browsers report cursor moves reliably.
    let drag: Vec2 = cursor.read().filter_map(|moved| moved.delta).sum();
    if buttons.pressed(MouseButton::Left) {
        // Dragging moves the scene with the mouse.
        yaw -= drag.x * MOUSE_ORBIT;
        pitch += drag.y * MOUSE_ORBIT;
    }
    let zoom = axis(&[KeyCode::KeyE], &[KeyCode::KeyQ]) * KEY_ZOOM * dt
        - scroll.delta.y
            * match scroll.unit {
                MouseScrollUnit::Line => LINE_ZOOM,
                MouseScrollUnit::Pixel => PIXEL_ZOOM,
            };

    if yaw == 0.0 && pitch == 0.0 && zoom == 0.0 && !view.is_added() {
        return;
    }
    view.yaw += yaw;
    view.pitch = (view.pitch + pitch).clamp(-0.2, 1.5);
    view.distance = (view.distance * (1.0 + zoom.clamp(-0.5, 0.5))).clamp(15.0, 300.0);
    for mut transform in &mut camera {
        *transform = view.camera_transform();
    }
}

fn spawn_showroom(
    mut commands: Commands,
    view: Res<ShowroomView>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("Showroom camera"),
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 45_f32.to_radians(),
            near: 0.5,
            far: 2_000.0,
            ..default()
        }),
        view.camera_transform(),
    ));
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: light_consts::lux::AMBIENT_DAYLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -0.7, -0.6), Vec3::Y),
        // WebGL2 supports a single shadow cascade.
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 400.0,
            ..default()
        }
        .build(),
    ));

    let mut part = |mesh: Mesh, color: Color, at: Vec3| {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(color)),
            Transform::from_translation(at),
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
            model.scene(&assets),
            Transform::from_translation(Vec3::X * (first_x + i as f32 * SPACING)),
        ));
    }
}

fn spawn_help(mut commands: Commands) {
    let names: Vec<_> = AircraftModel::ALL
        .iter()
        .map(|m| m.display_name())
        .collect();
    commands.spawn((
        Text::new(format!(
            "AIRCRAFT SHOWROOM\n\
             Left to right: {}\n\
             Red/white bars: 10 m in 1 m steps\n\
             Axes: X red, Y green, +Z blue. Noses must point away from blue (-Z).\n\
             Drag to orbit, wheel to zoom   (keys: A/D, W/S, Q/E)",
            names.join("  |  ")
        )),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bevy::input::{
        ButtonState,
        mouse::{MouseButtonInput, MouseWheel},
        touch::TouchPhase,
    };

    use super::*;
    use crate::{
        AppMode, AppModePlugin,
        test_support::{headless_app, key},
    };

    /// Number of entities with a mesh below `root`.
    fn meshes_below(world: &World, root: Entity) -> usize {
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

    fn model_roots(app: &mut App) -> Vec<(Entity, String)> {
        app.world_mut()
            .query_filtered::<(Entity, &Name), With<WorldAssetRoot>>()
            .iter(app.world())
            .map(|(e, n)| (e, n.to_string()))
            .collect()
    }

    fn camera(app: &mut App) -> Transform {
        let mut query = app
            .world_mut()
            .query_filtered::<&Transform, With<Camera3d>>();
        *query.single(app.world()).unwrap()
    }

    fn showroom() -> App {
        let mut app = headless_app(AppModePlugin(AppMode::Showroom));
        app.update();
        app
    }

    /// Updates with a little real time passing, as in a running app.
    fn run_frames(app: &mut App, frames: usize) {
        for _ in 0..frames {
            std::thread::sleep(Duration::from_millis(5));
            app.update();
        }
    }

    #[test]
    fn showroom_spawns_every_model_as_meshes() {
        let mut app = showroom();
        let roots = model_roots(&mut app);
        assert_eq!(roots.len(), AircraftModel::ALL.len());

        // Loading runs on background threads; give it a few seconds.
        let start = Instant::now();
        loop {
            app.update();
            let missing: Vec<_> = roots
                .iter()
                .filter(|(e, _)| meshes_below(app.world(), *e) == 0)
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
    }

    #[test]
    fn keys_orbit_and_zoom_the_camera() {
        let mut app = showroom();
        let start = camera(&mut app);
        assert_eq!(start, ShowroomView::default().camera_transform());

        key(&mut app, KeyCode::KeyD, ButtonState::Pressed);
        run_frames(&mut app, 20);
        key(&mut app, KeyCode::KeyD, ButtonState::Released);
        app.update();
        let orbited = camera(&mut app);
        assert!(start.translation.distance(orbited.translation) > 0.5);

        let before = app.world().resource::<ShowroomView>().distance;
        key(&mut app, KeyCode::KeyQ, ButtonState::Pressed);
        run_frames(&mut app, 20);
        assert!(app.world().resource::<ShowroomView>().distance < before);
    }

    #[test]
    fn mouse_drag_orbits_and_wheel_zooms() {
        let mut app = showroom();
        let window = Entity::PLACEHOLDER;
        let start = *app.world().resource::<ShowroomView>();

        // Moving without a button pressed does nothing.
        let moved = |delta: Vec2| CursorMoved {
            window,
            position: Vec2::ZERO,
            delta: Some(delta),
        };
        app.world_mut().write_message(moved(Vec2::new(100.0, 0.0)));
        app.update();
        assert_eq!(*app.world().resource::<ShowroomView>(), start);

        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
        app.update();
        app.world_mut().write_message(moved(Vec2::new(100.0, 40.0)));
        app.update();
        let dragged = *app.world().resource::<ShowroomView>();
        assert!((dragged.yaw - (start.yaw - 0.5)).abs() < 1e-4);
        assert!((dragged.pitch - (start.pitch + 0.2)).abs() < 1e-4);
        assert_ne!(camera(&mut app), start.camera_transform());

        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: 2.0,
            window,
            phase: TouchPhase::Moved,
        });
        app.update();
        let zoomed = app.world().resource::<ShowroomView>().distance;
        assert!(
            (zoomed - dragged.distance * 0.8).abs() < 1e-3,
            "zoomed to {zoomed}"
        );
    }

    #[test]
    fn game_has_no_showroom() {
        let mut app = headless_app(AppModePlugin(AppMode::Game));
        app.update();
        crate::test_support::tap(&mut app, KeyCode::KeyM);
        assert!(model_roots(&mut app).is_empty());
        assert!(app.world().get_resource::<ShowroomView>().is_none());
    }
}
