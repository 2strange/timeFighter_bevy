//! Headless combat tests: lock-on, gun, missiles, hits.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy, transform::TransformPlugin};
use bevy_rapier3d::prelude::{Collider, RigidBody};

use super::{
    AircraftVelocity, CombatConfig, CombatPlugin, DestroyCause, Destroyed, Gun, Health, Hit,
    Hitbox, LockOn, LockState, MissileRack, Munition, ShotFired, Team, Trigger, Weapon,
    hits::sweep_hits_sphere,
};
use crate::{
    ai::{EnemyKind, enemy_body},
    flight::{FlightState, PlayerAircraft},
    physics::{Layer, PhysicsPlugin, aircraft_body},
};

const FPS: f64 = 60.0;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, PhysicsPlugin, CombatPlugin))
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / FPS,
        )));
    app.finish();
    app.update();
    app
}

/// A static world box (building, ground) on the world layer.
fn spawn_world_box(app: &mut App, center: Vec3, half: Vec3) -> Entity {
    app.world_mut()
        .spawn((
            RigidBody::Fixed,
            Collider::cuboid(half.x, half.y, half.z),
            Layer::World.groups(),
            Transform::from_translation(center),
        ))
        .id()
}

/// A player jet at `transform`, armed on the next update.
fn spawn_player(app: &mut App, transform: Transform) -> Entity {
    let jet = app
        .world_mut()
        .spawn((
            PlayerAircraft,
            FlightState::default(),
            aircraft_body(Layer::Player),
            transform,
        ))
        .id();
    app.update();
    jet
}

fn spawn_target(app: &mut App, position: Vec3) -> Entity {
    app.world_mut()
        .spawn((
            Team::Enemy,
            Health::full(50.0),
            Hitbox { radius: 5.5 },
            AircraftVelocity::default(),
            enemy_body(EnemyKind::Spitfire),
            Transform::from_translation(position),
        ))
        .id()
}

fn run_for(app: &mut App, seconds: f32) {
    for _ in 0..(seconds * FPS as f32).round() as u32 {
        app.update();
    }
}

fn lock_state(app: &App, jet: Entity) -> LockState {
    app.world().get::<LockOn>(jet).unwrap().state
}

fn high_up() -> Transform {
    Transform::from_xyz(0.0, 1_000.0, 0.0)
}

#[test]
fn player_is_armed_with_full_loadout() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let cfg = app.world().resource::<CombatConfig>().clone();
    let world = app.world();
    assert_eq!(world.get::<Health>(jet).unwrap().current, cfg.player_health);
    assert_eq!(
        world.get::<Gun>(jet).unwrap().ammo,
        Some(cfg.player_gun_ammo)
    );
    assert_eq!(
        world.get::<MissileRack>(jet).unwrap().count,
        cfg.player_missiles
    );
    assert_eq!(*world.get::<Team>(jet).unwrap(), Team::Player);
}

#[test]
fn lock_needs_the_target_in_cone_and_range_for_the_lock_time() {
    let cfg = CombatConfig::default();
    let ahead = |distance: f32, degrees: f32| {
        let dir = Quat::from_rotation_y(degrees.to_radians()) * Vec3::NEG_Z;
        high_up().translation + dir * distance
    };
    let cases = [
        ("straight ahead", ahead(1_500.0, 0.0), true),
        ("inside the cone", ahead(2_000.0, 15.0), true),
        ("outside the cone", ahead(1_500.0, 30.0), false),
        ("beyond range", ahead(cfg.lock.range + 200.0, 0.0), false),
        ("behind", ahead(800.0, 180.0), false),
    ];
    for (name, position, locks) in cases {
        let mut app = app();
        let jet = spawn_player(&mut app, high_up());
        let target = spawn_target(&mut app, position);

        run_for(&mut app, cfg.lock.time * 0.5);
        let half_way = lock_state(&app, jet);
        run_for(&mut app, cfg.lock.time * 0.6);
        let after = lock_state(&app, jet);
        if locks {
            assert!(
                matches!(half_way, LockState::Tracking { target: t, .. } if t == target),
                "{name}: tracking after half the lock time, got {half_way:?}"
            );
            assert_eq!(after, LockState::Locked { target }, "{name}");
        } else {
            assert_eq!(after, LockState::Searching, "{name}");
        }
    }
}

#[test]
fn lock_is_lost_when_the_target_leaves_the_cone() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let target = spawn_target(&mut app, Vec3::new(0.0, 1_000.0, -1_500.0));
    run_for(&mut app, 1.2);
    assert_eq!(lock_state(&app, jet), LockState::Locked { target });

    // Slightly beyond the cone: the lock holds (hysteresis) ...
    let cfg = app.world().resource::<CombatConfig>().clone();
    let edge = Quat::from_rotation_y(cfg.lock.cone * 1.1) * Vec3::NEG_Z * 1_500.0;
    app.world_mut()
        .get_mut::<Transform>(target)
        .unwrap()
        .translation = Vec3::new(0.0, 1_000.0, 0.0) + edge;
    app.update();
    assert_eq!(lock_state(&app, jet), LockState::Locked { target });

    // ... far off the nose it breaks.
    app.world_mut()
        .get_mut::<Transform>(target)
        .unwrap()
        .translation = Vec3::new(1_500.0, 1_000.0, 0.0);
    app.update();
    assert_eq!(lock_state(&app, jet), LockState::Searching);
}

#[test]
fn gun_spends_ammunition_at_its_rate_and_stops_when_empty() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let cfg = app.world().resource::<CombatConfig>().clone();

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    run_for(&mut app, 1.0);
    let ammo = app.world().get::<Gun>(jet).unwrap().ammo.unwrap();
    let fired = cfg.player_gun_ammo - ammo;
    assert!(
        (fired as f32 - cfg.player_gun.rate).abs() <= 1.0,
        "fired {fired} rounds in 1 s at {} rounds/s",
        cfg.player_gun.rate
    );

    // Releasing stops the gun.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    run_for(&mut app, 0.5);
    assert_eq!(app.world().get::<Gun>(jet).unwrap().ammo, Some(ammo));

    // Empty gun: no more rounds.
    app.world_mut().get_mut::<Gun>(jet).unwrap().ammo = Some(3);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    run_for(&mut app, 0.5);
    assert_eq!(app.world().get::<Gun>(jet).unwrap().ammo, Some(0));
    for _ in 0..30 {
        app.update();
        assert!(read::<ShotFired>(&app).is_empty(), "empty gun fired");
    }
}

#[test]
fn gun_rounds_destroy_a_target_ahead_and_never_hit_their_own_side() {
    let mut app = app();
    spawn_player(&mut app, high_up());
    let enemy = spawn_target(&mut app, Vec3::new(0.0, 1_000.0, -400.0));
    // A friendly aircraft right in the line of fire.
    let wingman = app
        .world_mut()
        .spawn((
            Team::Player,
            Health::full(50.0),
            Hitbox { radius: 5.5 },
            aircraft_body(Layer::Player),
            Transform::from_xyz(0.0, 1_000.0, -200.0),
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);

    let mut destroyed = Vec::new();
    for _ in 0..120 {
        app.update();
        destroyed.extend(read::<Destroyed>(&app));
        if !destroyed.is_empty() {
            break;
        }
    }
    assert_eq!(destroyed.len(), 1, "one kill: {destroyed:?}");
    assert_eq!(destroyed[0].entity, enemy);
    assert_eq!(destroyed[0].cause, DestroyCause::Gun);
    assert!(app.world().get_entity(enemy).is_err(), "enemy despawned");
    assert_eq!(app.world().get::<Health>(wingman).unwrap().current, 50.0);
}

#[test]
fn fast_rounds_cannot_tunnel_through_a_hitbox() {
    // 1 km/s at 30 fps: 33 m per step, both ends outside a 5 m sphere.
    let a = Vec3::new(0.0, 0.0, 20.0);
    let b = Vec3::new(0.0, 0.0, -13.0);
    assert!(sweep_hits_sphere(a, b, Vec3::ZERO, 5.0).is_some());
    assert!(sweep_hits_sphere(a, b, Vec3::new(6.0, 0.0, 0.0), 5.0).is_none());
}

/// Messages of type `M` written during the last update.
fn read<M: Message + Clone>(app: &App) -> Vec<M> {
    let messages = app.world().resource::<Messages<M>>();
    messages.iter_current_update_messages().cloned().collect()
}

/// Moves the target in hard alternating turns with climb and dive, like a
/// pilot jinking to shake a missile.
#[derive(Component)]
struct Jink {
    speed: f32,
    turn_rate: f32,
    period: f32,
    t: f32,
}

fn jink(time: Res<Time>, mut targets: Query<(&mut Jink, &mut Transform, &mut AircraftVelocity)>) {
    let dt = time.delta_secs();
    for (mut jink, mut transform, mut velocity) in &mut targets {
        jink.t += dt;
        let side = if ((jink.t / jink.period) as u32).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        let pitch = (jink.t * 1.3).sin() * 0.3;
        let turn = Quat::from_rotation_y(side * jink.turn_rate * dt)
            * Quat::from_axis_angle(*transform.right(), pitch * dt);
        transform.rotation = (turn * transform.rotation).normalize();
        velocity.0 = transform.forward() * jink.speed;
        transform.translation += velocity.0 * dt;
    }
}

fn launch_at(app: &mut App, jet: Entity, target: Option<Entity>) {
    if let Some(target) = target {
        app.world_mut().get_mut::<LockOn>(jet).unwrap().state = LockState::Locked { target };
    }
    app.world_mut().get_mut::<Trigger>(jet).unwrap().missile = true;
}

/// Seconds until `target` is destroyed by a missile, if within `limit`.
fn time_to_kill(app: &mut App, target: Entity, limit: f32) -> Option<f32> {
    let mut t = 0.0;
    while t < limit {
        app.update();
        t += 1.0 / FPS as f32;
        if read::<Destroyed>(app)
            .iter()
            .any(|d| d.entity == target && d.cause == DestroyCause::Missile)
        {
            return Some(t);
        }
    }
    None
}

#[test]
fn guided_missile_hits_a_jinking_target_within_six_seconds() {
    for (offset, heading) in [
        // Tail chase, target 1.5 km ahead flying away.
        (Vec3::new(0.0, 0.0, -1_500.0), 0.0_f32),
        // Crossing target 15° off the nose.
        (Vec3::new(-400.0, 100.0, -1_400.0), 90.0),
        // Head-on.
        (Vec3::new(150.0, -50.0, -2_000.0), 180.0),
    ] {
        let mut app = app();
        app.add_systems(Update, jink.before(super::CombatSystems::Aim));
        let jet = spawn_player(&mut app, high_up());
        app.world_mut()
            .get_mut::<FlightState>(jet)
            .unwrap()
            .velocity = Vec3::NEG_Z * 250.0;
        let target = spawn_target(&mut app, high_up().translation + offset);
        app.world_mut().entity_mut(target).insert((
            Jink {
                speed: 170.0,
                turn_rate: 0.75,
                period: 1.5,
                t: 0.0,
            },
            Transform::from_translation(high_up().translation + offset)
                .with_rotation(Quat::from_rotation_y(heading.to_radians())),
        ));

        launch_at(&mut app, jet, Some(target));
        let hit = time_to_kill(&mut app, target, 6.0);
        assert!(hit.is_some(), "offset {offset:?}: missile missed");
        assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 5);
    }
}

#[test]
fn unguided_missile_flies_straight_on() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let off_axis = spawn_target(&mut app, Vec3::new(300.0, 1_000.0, -1_500.0));
    launch_at(&mut app, jet, None);
    assert!(time_to_kill(&mut app, off_axis, 5.0).is_none());

    let mut missiles = app.world_mut().query::<(&Munition, &Transform)>();
    let (projectile, transform) = missiles.single(app.world()).unwrap();
    assert_eq!(projectile.weapon, Weapon::Missile);
    assert!(
        transform.translation.x.abs() < 1.0,
        "drifted: {transform:?}"
    );
}

#[test]
fn missile_racks_hold_a_limited_number() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let cfg = app.world().resource::<CombatConfig>().clone();
    for _ in 0..cfg.player_missiles + 3 {
        launch_at(&mut app, jet, None);
        run_for(&mut app, cfg.missile.reload + 0.1);
    }
    assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 0);
    let launched = app
        .world_mut()
        .query::<&super::Missile>()
        .iter(app.world())
        .count();
    assert_eq!(launched as u32, cfg.player_missiles);
}

#[test]
fn player_destroyed_by_hits_stays_in_the_world() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let cfg = app.world().resource::<CombatConfig>().clone();
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: jet,
        point: Vec3::ZERO,
        team: Team::Enemy,
        weapon: Weapon::Gun,
        damage: cfg.player_health + 1.0,
    });
    app.update();
    let destroyed = read::<Destroyed>(&app);
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0].team, Team::Player);
    assert!(app.world().get::<Health>(jet).unwrap().is_dead());
}

#[test]
fn buildings_stop_rounds_and_missiles() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let enemy = spawn_target(&mut app, Vec3::new(0.0, 1_000.0, -600.0));
    // A tower between the jet and the enemy.
    spawn_world_box(
        &mut app,
        Vec3::new(0.0, 1_000.0, -300.0),
        Vec3::new(40.0, 60.0, 20.0),
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    launch_at(&mut app, jet, Some(enemy));
    run_for(&mut app, 2.0);
    assert_eq!(app.world().get::<Health>(enemy).unwrap().current, 50.0);
}

#[test]
fn flying_into_the_world_destroys_the_player() {
    let mut app = app();
    let jet = spawn_player(&mut app, Transform::from_xyz(0.0, 30.0, 0.0));
    spawn_world_box(
        &mut app,
        Vec3::new(0.0, -50.0, 0.0),
        Vec3::new(500.0, 50.0, 500.0),
    );
    run_for(&mut app, 0.2);
    assert!(!app.world().get::<Health>(jet).unwrap().is_dead());

    // Down into the ground, as the flight model would move it.
    app.world_mut()
        .get_mut::<Transform>(jet)
        .unwrap()
        .translation
        .y = 0.5;
    let mut crashes = Vec::new();
    for _ in 0..5 {
        app.update();
        crashes.extend(read::<Destroyed>(&app));
    }
    assert_eq!(crashes.len(), 1, "{crashes:?}");
    assert_eq!(crashes[0].cause, DestroyCause::Crash);
    assert_eq!(crashes[0].team, Team::Player);
}
