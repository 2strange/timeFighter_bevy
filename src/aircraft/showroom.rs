//! Aircraft showroom: all models side by side with scale references, for
//! checking size and orientation by eye. Runs as its own app mode (browser:
//! `showroom.html`), without world or flight.
//!
//! Pages (keys 1-4 or Tab): the models in the game, a second set, sample
//! formations, and the zeppelin. Mouse: drag to orbit, wheel to zoom. Keys:
//! A/D or Left/Right orbit, W/S or Up/Down tilt, Q/E zoom.

use bevy::{
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    light::CascadeShadowConfigBuilder,
    prelude::*,
    window::CursorMoved,
};

use super::AircraftModel;

/// Orbit speed with keys (rad/s) and with the mouse (rad per pixel).
const KEY_ORBIT: f32 = 1.0;
const MOUSE_ORBIT: f32 = 0.005;
/// Zoom: fraction of the distance per second (keys), per wheel line, per pixel.
const KEY_ZOOM: f32 = 1.0;
const LINE_ZOOM: f32 = 0.1;
const PIXEL_ZOOM: f32 = 0.002;
/// Farthest zoom (m): the zeppelin page needs room.
const MAX_DISTANCE: f32 = 1_000.0;

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
            distance: 230.0,
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
            .init_resource::<ShowroomPage>()
            .add_systems(Startup, (spawn_scene_basics, spawn_help))
            .add_systems(Update, (switch_page, show_page, orbit_camera).chain());
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

    if yaw == 0.0 && pitch == 0.0 && zoom == 0.0 && !view.is_changed() {
        return;
    }
    view.yaw += yaw;
    view.pitch = (view.pitch + pitch).clamp(-0.2, 1.5);
    view.distance = (view.distance * (1.0 + zoom.clamp(-0.5, 0.5))).clamp(15.0, MAX_DISTANCE);
    for mut transform in &mut camera {
        *transform = view.camera_transform();
    }
}

/// Gap between the wing tips of neighbouring models in a row (m).
const GAP: f32 = 6.0;

/// One page of the showroom: models at fixed positions.
#[derive(Clone, Debug)]
pub struct ShowroomScene {
    pub name: &'static str,
    pub items: Vec<(AircraftModel, Vec3)>,
}

impl ShowroomScene {
    /// Models side by side along X, centred, wing tip to wing tip.
    fn row(name: &'static str, models: &[AircraftModel]) -> Self {
        let width =
            models.iter().map(|m| m.size().span()).sum::<f32>() + GAP * (models.len() as f32 - 1.0);
        let mut left = -width / 2.0;
        let items = models
            .iter()
            .map(|&model| {
                let span = model.size().span();
                let centre = left + span / 2.0;
                left += span + GAP;
                (model, Vec3::X * centre)
            })
            .collect();
        Self { name, items }
    }

    /// Corners of the box around all models.
    fn bounds(&self) -> (Vec3, Vec3) {
        self.items.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), (model, at)| {
                let size = model.size();
                (lo.min(*at + size.min), hi.max(*at + size.max))
            },
        )
    }
}

/// All showroom pages; every model appears on at least one.
pub fn scenes() -> Vec<ShowroomScene> {
    use AircraftModel::*;
    vec![
        ShowroomScene::row(
            "Jets, fighters and bombers in the game",
            &[
                F14Tomcat,
                Mig29,
                Tu2,
                F4uCorsair,
                Spitfire,
                SpitfireClassic,
                B17,
                Lancaster,
            ],
        ),
        ShowroomScene::row("Second set", &[Bf109K4, Me309, He177, Me264, Ju390, Ju287]),
        ShowroomScene {
            name: "Formations: fighter pair, bomber flight",
            items: vec![
                // Fighter pair ahead of the bombers.
                (Bf109K4, Vec3::new(-8.0, 0.0, -70.0)),
                (Me309, Vec3::new(8.0, 0.0, -70.0)),
                // Ju 390 leading three He 177 in a V.
                (Ju390, Vec3::new(0.0, 0.0, -10.0)),
                (He177, Vec3::new(-40.0, -2.0, 25.0)),
                (He177, Vec3::new(40.0, -2.0, 25.0)),
                (He177, Vec3::new(0.0, -2.0, 55.0)),
            ],
        },
        ShowroomScene {
            name: "Zeppelin, with an F-14 and an He 177 for scale",
            items: vec![
                (Zeppelin, Vec3::ZERO),
                (F14Tomcat, Vec3::new(45.0, 0.0, -60.0)),
                (He177, Vec3::new(-60.0, 0.0, -40.0)),
            ],
        },
    ]
}

/// The page on show.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShowroomPage(pub usize);

/// Everything that belongs to the current page.
#[derive(Component)]
struct PageContent;

#[derive(Component)]
struct ShowroomHelp;

/// Keys 1-4 pick a page, Tab goes to the next.
fn switch_page(keys: Res<ButtonInput<KeyCode>>, mut page: ResMut<ShowroomPage>) {
    let count = scenes().len();
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
    ];
    for (i, key) in digits.iter().enumerate().take(count) {
        if keys.just_pressed(*key) {
            page.set_if_neq(ShowroomPage(i));
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        page.0 = (page.0 + 1) % count;
    }
}

/// Rebuilds the page (floor, scale bars, axes, models, gun markers) and
/// moves the camera back far enough to see it.
fn show_page(
    mut commands: Commands,
    page: Res<ShowroomPage>,
    assets: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut view: ResMut<ShowroomView>,
    old: Query<Entity, With<PageContent>>,
    mut help: Query<&mut Text, With<ShowroomHelp>>,
) {
    if !page.is_changed() {
        return;
    }
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let all = scenes();
    let scene = &all[page.0.min(all.len() - 1)];
    let (lo, hi) = scene.bounds();
    let floor_y = lo.y - 0.3;
    let centre = (lo + hi) / 2.0;

    let mut part = |mesh: Mesh, color: Color, at: Vec3| {
        commands.spawn((
            PageContent,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(color)),
            Transform::from_translation(at),
        ));
    };
    let margin = 12.0;
    let floor = (hi - lo).xz() + Vec2::splat(2.0 * margin);
    part(
        Cuboid::new(floor.x, 0.2, floor.y).mesh().build(),
        Color::srgb(0.32, 0.34, 0.36),
        Vec3::new(centre.x, floor_y - 0.1, centre.z),
    );

    // Two 10 m bars in 1 m steps: along X in front, along Z at the left.
    let white = Color::srgb(0.95, 0.95, 0.95);
    let red = Color::srgb(0.85, 0.12, 0.1);
    let front = lo.z - margin / 2.0;
    let left = lo.x - margin / 2.0;
    for i in 0..10 {
        let color = if i % 2 == 0 { white } else { red };
        let step = i as f32 - 4.5;
        part(
            Cuboid::new(1.0, 0.25, 0.25).mesh().build(),
            color,
            Vec3::new(centre.x + step, floor_y + 0.125, front),
        );
        part(
            Cuboid::new(0.25, 0.25, 1.0).mesh().build(),
            color,
            Vec3::new(left, floor_y + 0.125, centre.z + step),
        );
    }

    // Axis cross at the right: +X red, +Y green, +Z blue, 6 m each.
    let axes = Vec3::new(hi.x + margin / 2.0, floor_y, centre.z);
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

    let mount_mesh = meshes.add(Sphere::new(0.45));
    let mount_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.75, 0.2),
        unlit: true,
        ..default()
    });
    for (model, at) in &scene.items {
        commands.spawn((
            PageContent,
            Name::new(model.display_name()),
            model.scene(&assets),
            Transform::from_translation(*at),
        ));
        // Gun positions of the bombers, for checking by eye.
        for mount in model.gun_mounts() {
            commands.spawn((
                PageContent,
                Mesh3d(mount_mesh.clone()),
                MeshMaterial3d(mount_material.clone()),
                Transform::from_translation(*at + mount.position),
            ));
        }
    }
    view.distance = ((hi - lo).xz().length() * 1.1).clamp(40.0, MAX_DISTANCE);

    let names: Vec<_> = scene.items.iter().map(|(m, _)| m.display_name()).collect();
    let mut unique = names.clone();
    unique.dedup();
    let pages: Vec<_> = all
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mark = if i == page.0 { ">" } else { " " };
            format!("{mark} {} {}", i + 1, s.name)
        })
        .collect();
    for mut text in &mut help {
        text.0 = format!(
            "AIRCRAFT SHOWROOM   (1-{} or Tab: page)\n{}\n\nOn this page: {}\n\
             Red/white bars: 10 m in 1 m steps\n\
             Axes: X red, Y green, +Z blue. Noses must point away from blue (-Z).\n\
             Amber dots: gun positions of the bombers.\n\
             Drag to orbit, wheel to zoom   (keys: A/D, W/S, Q/E)",
            all.len(),
            pages.join("\n"),
            unique.join("  |  "),
        );
    }
}

fn spawn_scene_basics(mut commands: Commands, view: Res<ShowroomView>) {
    commands.spawn((
        Name::new("Showroom camera"),
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 45_f32.to_radians(),
            near: 0.5,
            far: 3_000.0,
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
            maximum_distance: 600.0,
            ..default()
        }
        .build(),
    ));
}

fn spawn_help(mut commands: Commands) {
    commands.spawn((
        ShowroomHelp,
        Text::new(""),
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
    fn every_page_spawns_its_models_as_meshes() {
        let mut app = showroom();
        for (index, scene) in scenes().iter().enumerate() {
            app.world_mut().resource_mut::<ShowroomPage>().0 = index;
            app.update();
            let roots = model_roots(&mut app);
            assert_eq!(roots.len(), scene.items.len(), "page {index}");

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
                    "page {index}: no meshes spawned for {missing:?}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn every_model_is_on_a_page_and_keys_switch_pages() {
        for model in AircraftModel::ALL {
            assert!(
                scenes()
                    .iter()
                    .any(|s| s.items.iter().any(|(m, _)| *m == model)),
                "{model:?} on no page"
            );
        }
        let mut app = showroom();
        crate::test_support::tap(&mut app, KeyCode::Digit2);
        assert_eq!(app.world().resource::<ShowroomPage>().0, 1);
        crate::test_support::tap(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<ShowroomPage>().0, 2);
        let names: Vec<_> = model_roots(&mut app).into_iter().map(|(_, n)| n).collect();
        assert_eq!(
            names.iter().filter(|n| *n == "He 177").count(),
            3,
            "{names:?}"
        );
    }

    #[test]
    fn formations_do_not_overlap() {
        for scene in scenes() {
            for (i, (a, pa)) in scene.items.iter().enumerate() {
                for (b, pb) in &scene.items[i + 1..] {
                    let (sa, sb) = (a.size(), b.size());
                    let apart = (pa.x + sa.max.x < pb.x + sb.min.x)
                        || (pb.x + sb.max.x < pa.x + sa.min.x)
                        || (pa.z + sa.max.z < pb.z + sb.min.z)
                        || (pb.z + sb.max.z < pa.z + sa.min.z);
                    assert!(apart, "{}: {a:?} and {b:?} overlap", scene.name);
                }
            }
        }
    }

    #[test]
    fn keys_orbit_and_zoom_the_camera() {
        let mut app = showroom();
        let start = camera(&mut app);
        assert_eq!(
            start,
            app.world().resource::<ShowroomView>().camera_transform()
        );

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
        // Only the player's own jet carries a model in the game.
        let showroom_models: Vec<_> = model_roots(&mut app)
            .into_iter()
            .filter(|(_, name)| AircraftModel::ALL.iter().any(|m| m.display_name() == name))
            .collect();
        assert!(showroom_models.is_empty(), "{showroom_models:?}");
        assert!(app.world().get_resource::<ShowroomView>().is_none());
    }
}
