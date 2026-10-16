//! Headless tests of the enemy pilots.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy, transform::TransformPlugin};
use bevy_rapier3d::prelude::{Collider, RigidBody};

use super::{
    AiConfig, AiPlugin, EnemyKind, EnemyPilot, EnemyTuning, ObstacleMap, PilotMode, PilotSkill,
    enemy_bundle,
};
use crate::{
    combat::{
        CombatConfig, CombatPlugin, CrashInto, DestroyCause, Destroyed, Health, LockOn, LockState,
        Team, Trigger,
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
        .spawn(enemy_bundle(
            EnemyKind::Spitfire,
            PilotSkill::VETERAN,
            EnemyTuning::default(),
            transform,
        ))
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
    assert_eq!(crash.cause, DestroyCause::Crash(CrashInto::Obstacle));
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

#[test]
fn rookies_take_longer_than_veterans_to_break() {
    let break_after = |skill: PilotSkill| {
        let mut app = app(60.0, false);
        spawn_player(
            &mut app,
            Transform::from_xyz(0.0, 800.0, 0.0),
            200.0,
            FlightInput::default(),
        );
        let enemy = app
            .world_mut()
            .spawn(enemy_bundle(
                EnemyKind::Spitfire,
                skill,
                EnemyTuning::default(),
                Transform::from_xyz(0.0, 800.0, -400.0),
            ))
            .id();
        for frame in 0..120 {
            app.update();
            let pilot = app.world().get::<EnemyPilot>(enemy).unwrap();
            if matches!(pilot.mode, PilotMode::Break { .. }) {
                return Some(frame as f32 / 60.0);
            }
        }
        None
    };
    let veteran = break_after(PilotSkill::VETERAN).expect("veteran breaks");
    let rookie = break_after(PilotSkill::ROOKIE).expect("rookie breaks");
    assert!(veteran < 0.05, "veteran after {veteran} s");
    assert!(
        rookie >= PilotSkill::ROOKIE.reaction - 0.02,
        "rookie after {rookie} s"
    );
}

#[test]
fn collision_course_is_detected_and_escaped() {
    let cfg = AiConfig::default();
    let skill = PilotSkill::VETERAN;
    let me = (Vec3::new(0.0, 800.0, 0.0), Vec3::NEG_Z * 150.0);
    let escape = |other: Vec3, velocity: Vec3| {
        super::collision_escape_for(
            &cfg,
            &skill,
            me.0,
            me.1,
            std::iter::once((other, velocity, true)),
        )
    };
    // Head-on, 1 km apart, closing at 400 m/s: steer away.
    let head_on = escape(Vec3::new(0.0, 800.0, -1_000.0), Vec3::Z * 250.0)
        .expect("escape")
        .0;
    assert!(
        head_on.dot(Vec3::NEG_Z) < 0.95,
        "turns off the line: {head_on:?}"
    );
    // Same course, offset 200 m: no threat.
    assert!(escape(Vec3::new(200.0, 800.0, -300.0), Vec3::NEG_Z * 150.0).is_none());
    // Head-on but far away (closest approach in 10 s): not yet.
    assert!(escape(Vec3::new(0.0, 800.0, -4_000.0), Vec3::Z * 250.0).is_none());
    // Diverging: never.
    assert!(escape(Vec3::new(0.0, 800.0, 100.0), Vec3::Z * 250.0).is_none());
}

/// What a bait run produced.
struct BaitRun {
    /// Collisions between the player and an enemy.
    rammings: usize,
    /// Enemies that flew into the world.
    enemy_crashes: usize,
    /// Closest pass between two enemies (m).
    closest: f32,
}

/// Runs the bait scenario and returns (rammings, closest enemy-enemy pass).
fn bait_scenario(input: FlightInput, enemies: &[Transform], seconds: u32) -> (usize, f32) {
    let run = bait_run(800.0, false, input, enemies, seconds);
    (run.rammings, run.closest)
}

/// A player bait at `altitude` flying with `input`, optionally over a flat
/// ground collider, among `enemies` (alternately veterans and rookies).
fn bait_run(
    altitude: f32,
    ground: bool,
    input: FlightInput,
    enemies: &[Transform],
    seconds: u32,
) -> BaitRun {
    let mut app = app(60.0, false);
    app.world_mut().resource_mut::<CombatConfig>().player_health = f32::MAX;
    if ground {
        app.world_mut().spawn((
            crate::physics::Ground,
            RigidBody::Fixed,
            Collider::cuboid(20_000.0, 50.0, 20_000.0),
            Layer::World.groups(),
            Transform::from_xyz(0.0, -50.0, 0.0),
        ));
    }
    app.update();
    let jet = spawn_player(
        &mut app,
        Transform::from_xyz(0.0, altitude, 0.0),
        250.0,
        input,
    );
    for (i, transform) in enemies.iter().enumerate() {
        let skill = if i % 2 == 0 {
            PilotSkill::VETERAN
        } else {
            PilotSkill::ROOKIE
        };
        app.world_mut().spawn(enemy_bundle(
            EnemyKind::Spitfire,
            skill,
            EnemyTuning::default(),
            *transform,
        ));
    }
    let mut run = BaitRun {
        rammings: 0,
        enemy_crashes: 0,
        closest: f32::INFINITY,
    };
    for _ in 0..(seconds * 60) {
        app.update();
        let world = app.world();
        run.rammings += world
            .resource::<Messages<crate::physics::PlayerCrashed>>()
            .iter_current_update_messages()
            .count();
        for crash in world
            .resource::<Messages<crate::physics::EnemyCrashed>>()
            .iter_current_update_messages()
        {
            if crash.obstacle == jet {
                run.rammings += 1;
            } else {
                run.enemy_crashes += 1;
            }
        }
        let positions: Vec<Vec3> = app
            .world_mut()
            .query_filtered::<&Transform, With<super::Enemy>>()
            .iter(app.world())
            .map(|t| t.translation)
            .collect();
        for (i, a) in positions.iter().enumerate() {
            for b in &positions[i + 1..] {
                run.closest = run.closest.min(a.distance(*b));
            }
        }
    }
    run
}

/// `count` enemies on a ring of `radius` around the origin, facing in.
fn ring(count: usize, radius: f32, altitude: f32) -> Vec<Transform> {
    (0..count)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / count as f32;
            let position = Vec3::new(
                angle.cos() * radius,
                altitude + 15.0 * (i % 8) as f32,
                angle.sin() * radius,
            );
            Transform::from_translation(position)
                .looking_at(Vec3::new(0.0, position.y, 0.0), Vec3::Y)
        })
        .collect()
}

#[test]
fn enemies_never_ram_a_player_flying_straight_into_them() {
    // Spitfires head-on in the jet's path and right behind it.
    let mut enemies = Vec::new();
    for (i, x) in [-30.0, 0.0, 25.0, 60.0].into_iter().enumerate() {
        enemies.push(
            Transform::from_xyz(x, 800.0 + 10.0 * i as f32, -2_500.0 - 400.0 * i as f32)
                .looking_to(Vec3::Z, Vec3::Y),
        );
    }
    for x in [-40.0, 40.0] {
        enemies.push(Transform::from_xyz(x, 800.0, 600.0).looking_to(Vec3::NEG_Z, Vec3::Y));
    }
    let (rams, closest) = bait_scenario(FlightInput::default(), &enemies, 30);
    assert_eq!(rams, 0, "rammings");
    assert!(
        closest > 15.0,
        "enemies came within {closest:.0} m of each other"
    );
}

#[test]
fn enemies_never_ram_a_circling_player_in_a_minute() {
    let enemies: Vec<Transform> = (0..8)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 8.0;
            let position = Vec3::new(
                angle.cos() * 2_000.0,
                700.0 + 40.0 * i as f32,
                angle.sin() * 2_000.0,
            );
            Transform::from_translation(position)
                .looking_at(Vec3::new(0.0, position.y, 0.0), Vec3::Y)
        })
        .collect();
    let input = FlightInput {
        yaw: 0.15,
        ..default()
    };
    let (rams, closest) = bait_scenario(input, &enemies, 60);
    assert_eq!(rams, 0, "rammings");
    assert!(
        closest > 15.0,
        "enemies came within {closest:.0} m of each other"
    );
}

#[test]
fn slower_difficulty_slows_the_enemies() {
    let mut app = app(60.0, false);
    let tuned = |app: &mut App, speed: f32| {
        app.world_mut()
            .spawn(enemy_bundle(
                EnemyKind::Spitfire,
                PilotSkill::VETERAN,
                EnemyTuning {
                    speed,
                    accuracy: 1.0,
                },
                Transform::from_xyz(0.0, 900.0, 0.0),
            ))
            .id()
    };
    let slow = tuned(&mut app, 0.8);
    for _ in 0..(20 * 60) {
        app.update();
    }
    let speed = app.world().get::<super::EnemyFlight>(slow).unwrap().speed;
    let max = EnemyKind::Spitfire.stats().max_speed;
    assert!(speed <= max * 0.8 + 0.01, "speed {speed} m/s");
}

#[test]
fn a_running_avoidance_is_held_against_a_competing_contact() {
    let left = Vec3::NEG_X;
    let right = Vec3::X;
    // Two contacts alternate as "most urgent" from frame to frame.
    let mut state = None;
    let mut directions = Vec::new();
    for frame in 0..60 {
        let found = if frame % 2 == 0 {
            (left, 2.0)
        } else {
            (right, 1.8)
        };
        state = super::update_avoidance(state, Some(found), 1.2, 1.0 / 60.0);
        directions.push(state.unwrap().direction);
    }
    let switches = directions.windows(2).filter(|w| w[0] != w[1]).count();
    assert_eq!(switches, 0, "flipped {switches} times");

    // A much more urgent contact does take over at once.
    let urgent = super::update_avoidance(state, Some((Vec3::Y, 0.3)), 1.2, 1.0 / 60.0).unwrap();
    assert_eq!(urgent.direction, Vec3::Y);
    // Without contacts the manoeuvre ends after its hold time.
    let mut state = Some(urgent);
    for _ in 0..80 {
        state = super::update_avoidance(state, None, 1.2, 1.0 / 60.0);
    }
    assert!(state.is_none());
}

#[test]
fn non_finite_difficulty_factors_fall_back_to_normal() {
    let tuning = EnemyTuning {
        speed: f32::NAN,
        accuracy: f32::INFINITY,
    };
    let stats = EnemyKind::Spitfire.stats().tuned(tuning);
    assert_eq!(stats, EnemyKind::Spitfire.stats());

    // And such an enemy flies without panicking.
    let mut app = app(60.0, false);
    app.world_mut().spawn(enemy_bundle(
        EnemyKind::Spitfire,
        PilotSkill::VETERAN,
        tuning,
        Transform::from_xyz(0.0, 800.0, 0.0),
    ));
    for _ in 0..60 {
        app.update();
    }
}

#[test]
fn low_level_traffic_over_the_ground_neither_rams_nor_crashes() {
    let input = FlightInput {
        yaw: 0.15,
        ..default()
    };
    let run = bait_run(150.0, true, input, &ring(8, 1_800.0, 200.0), 40);
    assert_eq!(run.rammings, 0, "rammings");
    assert_eq!(run.enemy_crashes, 0, "enemies flew into the ground");
    assert!(run.closest > 15.0, "closest pass {:.0} m", run.closest);
}

#[test]
fn forty_eight_enemies_at_once_stay_stable() {
    // The custom maximum, all in the air together (normally flights come
    // one after another): a stability and cost smoke test.
    let input = FlightInput {
        yaw: 0.15,
        ..default()
    };
    let started = std::time::Instant::now();
    let run = bait_run(600.0, true, input, &ring(48, 2_500.0, 500.0), 20);
    eprintln!(
        "48 enemies, 20 s at 60 fps: {:.1} s wall clock, closest pass {:.0} m",
        started.elapsed().as_secs_f32(),
        run.closest
    );
    assert_eq!(run.rammings, 0, "rammings");
    assert_eq!(run.enemy_crashes, 0, "crashes");
    assert!(run.closest > 10.0, "closest pass {:.0} m", run.closest);
}
