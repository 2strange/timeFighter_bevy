//! Headless tests of the enemy pilots.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy, transform::TransformPlugin};
use bevy_rapier3d::prelude::{Collider, RigidBody};

use super::{AiConfig, AiPlugin, EnemyKind, EnemyPilot, ObstacleMap, PilotMode, enemy_bundle};
use crate::{
    combat::{
        CombatConfig, CombatPlugin, DestroyCause, Destroyed, Health, LockOn, LockState, Team,
        Trigger,
    },
    flight::{FlightConfig, FlightInput, FlightPlugin, FlightState, PlayerAircraft},
    physics::{Layer, PhysicsPlugin, aircraft_body},
    world::{CityPlugin, colliders::CityCollidersPlugin},
};

fn app(fps: f64, with_city: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        FlightPlugin,
        CombatPlugin,
        AiPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / fps,
    )));
    if with_city {
        app.add_plugins((CityPlugin, CityCollidersPlugin));
    }
    app.finish();
    app.update();
    app
}

/// Player jet at `transform` flying at `speed` with constant `input`.
fn spawn_player(app: &mut App, transform: Transform, speed: f32, input: FlightInput) -> Entity {
    let jet = spawn_ghost_player(app, transform, speed, input);
    app.world_mut()
        .entity_mut(jet)
        .insert(aircraft_body(Layer::Player));
    app.update();
    jet
}

/// Like [`spawn_player`] but without a collider: a bait that flies through
/// buildings without crashing.
fn spawn_ghost_player(
    app: &mut App,
    transform: Transform,
    speed: f32,
    input: FlightInput,
) -> Entity {
    let mut state = FlightState::new(&FlightConfig::default());
    state.speed = speed;
    state.velocity = transform.forward() * speed;
    app.world_mut()
        .spawn((PlayerAircraft, input, state, transform))
        .id()
}

fn spawn_enemy(app: &mut App, transform: Transform) -> Entity {
    app.world_mut()
        .spawn(enemy_bundle(EnemyKind::Spitfire, transform))
        .id()
}

#[test]
fn obstacle_map_holds_the_highest_roof_per_cell() {
    let map = ObstacleMap::from_boxes(
        50.0,
        [
            (Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 120.0, 40.0)),
            (Vec3::new(30.0, 0.0, 30.0), Vec3::new(140.0, 60.0, 90.0)),
        ],
    );
    assert_eq!(map.height_at(10.0, 10.0), 120.0);
    assert_eq!(
        map.height_at(45.0, 45.0),
        120.0,
        "shared cell keeps the max"
    );
    assert_eq!(map.height_at(120.0, 80.0), 60.0);
    assert_eq!(map.height_at(-500.0, 0.0), 0.0, "outside the grid");
    assert_eq!(map.height_at(120.0, 120.0), 0.0, "empty cell");

    // Quay walls and water below street level read as street level.
    let channel = ObstacleMap::from_boxes(
        50.0,
        [(Vec3::new(0.0, -20.0, 0.0), Vec3::new(100.0, 0.0, 100.0))],
    );
    assert_eq!(channel.height_at(50.0, 50.0), 0.0);
}

#[test]
fn river_channel_reads_as_street_level() {
    let layout = crate::world::CityLayout::generate(&crate::world::CityConfig::default());
    assert!(!layout.river.is_empty(), "the default city has a river");
    let map = ObstacleMap::from_layout(&layout);
    let extent = layout.config.grid_half_extent;
    let steps = (2.0 * extent / ObstacleMap::CELL) as i32;
    let lowest = (0..=steps)
        .flat_map(|i| (0..=steps).map(move |j| (i, j)))
        .map(|(i, j)| {
            let x = -extent + i as f32 * ObstacleMap::CELL;
            let z = -extent + j as f32 * ObstacleMap::CELL;
            map.height_at(x, z)
        })
        .fold(f32::INFINITY, f32::min);
    assert!(lowest >= 0.0, "nothing below street level: {lowest}");
}

/// The player circles low over downtown, eight enemies chase it for a
/// minute. None may come closer to roofs or ground than half
/// its clearance, and none may crash.
#[test]
fn enemies_chasing_a_low_player_never_hit_the_real_city() {
    let mut app = app(30.0, true);
    app.world_mut().resource_mut::<CombatConfig>().player_health = f32::MAX;
    app.update();
    let obstacles = app.world().resource::<ObstacleMap>().clone();
    assert!(obstacles.max_height() > 100.0, "city loaded");

    // Low enough to fly between the towers; without a collider of its own
    // it does not crash, so pursuing it leads right into them. The city
    // has its colliders: an enemy touching anything would crash.
    let jet = spawn_ghost_player(
        &mut app,
        Transform::from_xyz(0.0, 60.0, 600.0),
        180.0,
        FlightInput {
            yaw: 0.3,
            ..default()
        },
    );
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::TAU / 8.0;
        let height = obstacles.max_height() + 150.0 + 40.0 * i as f32;
        let position = Vec3::new(angle.cos() * 1_500.0, height, angle.sin() * 1_500.0);
        let towards_centre = Transform::from_translation(position)
            .looking_at(Vec3::new(0.0, position.y, 0.0), Vec3::Y);
        spawn_enemy(&mut app, towards_centre);
    }

    let clearance = app.world().resource::<AiConfig>().clearance;
    let mut lowest = f32::INFINITY;
    let mut closest = f32::INFINITY;
    for _ in 0..(60 * 30) {
        app.update();
        let crashed = app
            .world()
            .resource::<Messages<Destroyed>>()
            .iter_current_update_messages()
            .any(|d| d.team == Team::Enemy);
        assert!(!crashed, "an enemy crashed");
        let mut enemies = app
            .world_mut()
            .query_filtered::<&Transform, With<super::Enemy>>();
        let player = app.world().get::<Transform>(jet).unwrap().translation;
        for tf in enemies.iter(app.world()) {
            let above = tf.translation.y - obstacles.height_at(tf.translation.x, tf.translation.z);
            lowest = lowest.min(above);
            closest = closest.min(tf.translation.distance(player));
        }
    }
    eprintln!("lowest {lowest:.0} m above obstacles, closest {closest:.0} m to the player");
    assert!(
        closest < 400.0,
        "enemies never engaged: closest {closest:.0} m"
    );
    assert!(
        lowest > clearance * 0.5,
        "lowest enemy {lowest:.0} m above the obstacles (clearance {clearance} m)"
    );
}

#[test]
fn enemy_behind_the_player_closes_in_and_shoots() {
    let mut app = app(60.0, false);
    let jet = spawn_player(
        &mut app,
        Transform::from_xyz(0.0, 800.0, 0.0),
        140.0,
        FlightInput::default(),
    );
    spawn_enemy(&mut app, Transform::from_xyz(30.0, 810.0, 700.0));

    let mut fired = false;
    for _ in 0..(10 * 60) {
        app.update();
        let mut triggers = app.world_mut().query::<(&Trigger, &Team)>();
        fired |= triggers
            .iter(app.world())
            .any(|(t, team)| *team == Team::Enemy && t.gun);
    }
    assert!(fired, "enemy never fired");
    let health = app.world().get::<Health>(jet).unwrap();
    assert!(health.current < health.max, "player never hit");
}

#[test]
fn enemy_breaks_when_the_player_sits_on_its_tail() {
    let mut app = app(60.0, false);
    // Enemy 400 m ahead of the player, both flying along −Z.
    spawn_player(
        &mut app,
        Transform::from_xyz(0.0, 800.0, 0.0),
        200.0,
        FlightInput::default(),
    );
    let enemy = spawn_enemy(&mut app, Transform::from_xyz(0.0, 800.0, -400.0));
    app.update();
    let pilot = app.world().get::<EnemyPilot>(enemy).unwrap();
    assert!(
        matches!(pilot.mode, PilotMode::Break { .. }),
        "mode {:?}",
        pilot.mode
    );

    // While breaking it turns away from the straight line.
    let start = app.world().get::<Transform>(enemy).unwrap().forward();
    for _ in 0..60 {
        app.update();
    }
    let now = app.world().get::<Transform>(enemy).unwrap().forward();
    assert!(
        start.angle_between(*now) > 0.5,
        "turned only {} rad",
        start.angle_between(*now)
    );
}

#[test]
fn enemies_without_a_player_cruise_level_and_stay_in_the_arena() {
    let mut app = app(30.0, false);
    let cfg = app.world().resource::<AiConfig>().clone();
    let enemy = spawn_enemy(
        &mut app,
        Transform::from_xyz(cfg.arena_radius - 200.0, 600.0, 0.0).looking_to(Vec3::X, Vec3::Y),
    );
    for _ in 0..(60 * 30) {
        app.update();
    }
    let tf = app.world().get::<Transform>(enemy).unwrap();
    assert!(
        tf.translation.xz().length() < cfg.arena_radius + 1_000.0,
        "flew off to {:?}",
        tf.translation
    );
    assert!(
        tf.translation.y > cfg.clearance,
        "altitude {}",
        tf.translation.y
    );
}

#[test]
fn breaking_spitfire_cannot_shake_a_missile_fired_from_its_six() {
    let mut app = app(60.0, false);
    let jet = spawn_player(
        &mut app,
        Transform::from_xyz(0.0, 800.0, 0.0),
        250.0,
        FlightInput::default(),
    );
    let enemy = spawn_enemy(&mut app, Transform::from_xyz(80.0, 850.0, -1_500.0));
    app.world_mut().get_mut::<LockOn>(jet).unwrap().state = LockState::Locked { target: enemy };
    app.world_mut().get_mut::<Trigger>(jet).unwrap().missile = true;

    let mut broke = false;
    let mut killed_after = None;
    for frame in 0..(6 * 60) {
        app.update();
        if let Some(pilot) = app.world().get::<EnemyPilot>(enemy) {
            broke |= matches!(pilot.mode, PilotMode::Break { .. });
        }
        let killed = app
            .world()
            .resource::<Messages<Destroyed>>()
            .iter_current_update_messages()
            .any(|d| d.entity == enemy);
        if killed {
            killed_after = Some(frame as f32 / 60.0);
            break;
        }
    }
    assert!(broke, "the enemy noticed the missile and broke");
    assert!(killed_after.is_some(), "missile did not hit within 6 s");
}

#[test]
fn enemy_flying_into_a_building_crashes() {
    let mut app = app(60.0, false);
    // A wall ahead, too close to climb over.
    app.world_mut().spawn((
        RigidBody::Fixed,
        Collider::cuboid(300.0, 400.0, 10.0),
        Layer::World.groups(),
        Transform::from_xyz(0.0, 400.0, -150.0),
    ));
    let enemy = spawn_enemy(&mut app, Transform::from_xyz(0.0, 500.0, 0.0));
    let mut crash = None;
    for _ in 0..120 {
        app.update();
        crash = crash.or(app
            .world()
            .resource::<Messages<Destroyed>>()
            .iter_current_update_messages()
            .find(|d| d.entity == enemy)
            .copied());
    }
    let crash = crash.expect("enemy did not crash");
    assert_eq!(crash.cause, DestroyCause::Crash);
    assert!(app.world().get_entity(enemy).is_err(), "wreck removed");
}

#[test]
fn a_zero_lookahead_step_is_clamped_and_still_avoids_the_ground() {
    let mut app = app(60.0, false);
    app.world_mut().resource_mut::<AiConfig>().lookahead_step = 0.0;
    // Diving towards flat ground from 120 m.
    let enemy = spawn_enemy(
        &mut app,
        Transform::from_xyz(0.0, 120.0, 0.0).looking_to(Vec3::new(0.0, -0.5, -1.0), Vec3::Y),
    );
    let mut lowest = f32::INFINITY;
    for _ in 0..(5 * 60) {
        app.update();
        lowest = lowest.min(app.world().get::<Transform>(enemy).unwrap().translation.y);
    }
    assert!(lowest > 20.0, "dipped to {lowest:.0} m");
}
