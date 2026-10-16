//! Headless physics tests: no window, no renderer, fixed frame times.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy, transform::TransformPlugin};
use bevy_rapier3d::prelude::*;

use super::*;
use crate::{aircraft::AircraftModel, flight::PlayerAircraft};

/// Hits and crashes as seen by a gameplay system in `Update`, plus whether
/// the projectile entity was still readable there.
#[derive(Resource, Default)]
struct Seen {
    hits: Vec<(ProjectileHit, bool)>,
    crashes: Vec<PlayerCrashed>,
    enemy_crashes: Vec<EnemyCrashed>,
}

fn record(
    mut hits: MessageReader<ProjectileHit>,
    mut crashes: MessageReader<PlayerCrashed>,
    mut enemy_crashes: MessageReader<EnemyCrashed>,
    projectiles: Query<&Projectile>,
    mut seen: ResMut<Seen>,
) {
    for hit in hits.read() {
        seen.hits.push((*hit, projectiles.contains(hit.projectile)));
    }
    seen.crashes.extend(crashes.read().copied());
    seen.enemy_crashes.extend(enemy_crashes.read().copied());
}

/// Moves test aircraft at a constant velocity, like the flight model would.
#[derive(Component)]
struct Cruise(Vec3);

fn cruise(time: Res<Time>, mut aircraft: Query<(&Cruise, &mut Transform)>) {
    for (cruise, mut transform) in &mut aircraft {
        transform.translation += cruise.0 * time.delta_secs();
    }
}

fn app(fps: u32) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, PhysicsPlugin))
        .init_resource::<Seen>()
        .add_systems(Update, (record, cruise))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / f64::from(fps),
        )));
    app.finish();
    app.update();
    app
}

fn run(app: &mut App, seconds: f32) {
    let until = app.world().resource::<Time>().elapsed_secs() + seconds;
    while app.world().resource::<Time>().elapsed_secs() < until {
        app.update();
    }
}

fn spawn_box(app: &mut App, centre: Vec3, half: Vec3, layer: Layer) -> Entity {
    let body = if layer == Layer::World {
        RigidBody::Fixed
    } else {
        RigidBody::KinematicPositionBased
    };
    app.world_mut()
        .spawn((
            body,
            Collider::cuboid(half.x, half.y, half.z),
            layer.groups(),
            Transform::from_translation(centre),
        ))
        .id()
}

fn fire(app: &mut App, from: Vec3, velocity: Vec3, radius: f32, layer: Layer) -> Entity {
    app.world_mut()
        .spawn(projectile(
            Transform::from_translation(from),
            velocity,
            radius,
            layer,
            10.0,
        ))
        .id()
}

/// The ground plane y = 0 (in the game the world builds its own ground).
fn spawn_ground(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            Ground,
            RigidBody::Fixed,
            Collider::halfspace(Vec3::Y).unwrap(),
            Layer::World.groups(),
            Transform::default(),
        ))
        .id()
}

fn hits(app: &App) -> Vec<ProjectileHit> {
    app.world()
        .resource::<Seen>()
        .hits
        .iter()
        .map(|(h, _)| *h)
        .collect()
}

#[test]
fn missile_at_300_m_s_hits_a_house_and_does_not_tunnel() {
    for fps in [30, 60] {
        let mut app = app(fps);
        // A 12 x 20 x 12 m house, 500 m ahead along -Z.
        let house = spawn_box(
            &mut app,
            Vec3::new(0.0, 10.0, -500.0),
            Vec3::new(6.0, 10.0, 6.0),
            Layer::World,
        );
        let missile = fire(
            &mut app,
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, -300.0),
            0.3,
            Layer::PlayerProjectile,
        );

        run(&mut app, 2.5);
        let hits = hits(&app);
        assert_eq!(hits.len(), 1, "{fps} fps: {hits:?}");
        assert_eq!((hits[0].projectile, hits[0].target), (missile, house));
        // Stopped at the front face (z = -494), not inside or behind the house.
        let z = hits[0].point.z;
        assert!((-495.0..=-493.0).contains(&z), "{fps} fps: hit at z = {z}");
    }
}

#[test]
fn thin_wall_stops_a_missile_even_at_30_fps() {
    // 0.5 m wall; the missile moves 10 m per frame.
    let mut app = app(30);
    let wall = spawn_box(
        &mut app,
        Vec3::new(0.0, 0.0, -305.0),
        Vec3::new(10.0, 10.0, 0.25),
        Layer::World,
    );
    fire(
        &mut app,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -300.0),
        0.2,
        Layer::PlayerProjectile,
    );
    run(&mut app, 2.0);
    let hits = hits(&app);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].target, wall);
    assert!(hits[0].point.z > -305.5);
}

#[test]
fn bullet_at_1000_m_s_hits_a_10_m_enemy() {
    let mut app = app(30);
    // An enemy-sized 10 m box 800 m ahead; the bullet moves 33 m per frame.
    let enemy = spawn_box(
        &mut app,
        Vec3::new(0.0, 0.0, -800.0),
        Vec3::splat(5.0),
        Layer::Enemy,
    );
    let bullet = fire(
        &mut app,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -1_000.0),
        0.05,
        Layer::PlayerProjectile,
    );
    run(&mut app, 1.5);
    let hits = hits(&app);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!((hits[0].projectile, hits[0].target), (bullet, enemy));
}

#[test]
fn fast_projectiles_keep_their_speed() {
    // Rapier caps speeds at 400 m/s unless told otherwise.
    let mut app = app(30);
    let shot = fire(
        &mut app,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -1_000.0),
        0.05,
        Layer::PlayerProjectile,
    );
    run(&mut app, 0.5);
    let z = app.world().get::<Transform>(shot).unwrap().translation.z;
    assert!(
        (z + 500.0).abs() < 40.0,
        "after 0.5 s at 1,000 m/s: z = {z}"
    );
}

#[test]
fn hit_projectile_is_readable_for_one_frame_then_gone() {
    let mut app = app(60);
    spawn_box(
        &mut app,
        Vec3::new(0.0, 0.0, -100.0),
        Vec3::splat(5.0),
        Layer::World,
    );
    let missile = fire(
        &mut app,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -300.0),
        0.3,
        Layer::PlayerProjectile,
    );
    run(&mut app, 1.0);
    let seen = &app.world().resource::<Seen>().hits;
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].1,
        "projectile must still exist when gameplay reads the hit"
    );
    assert!(
        app.world().get_entity(missile).is_err(),
        "spent projectile must be despawned"
    );
}

#[test]
fn projectiles_hit_only_the_other_side() {
    let mut app = app(60);
    // Player at the origin, enemy 200 m ahead, both 10 m boxes.
    let player = spawn_box(&mut app, Vec3::ZERO, Vec3::splat(5.0), Layer::Player);
    let enemy = spawn_box(
        &mut app,
        Vec3::new(0.0, 0.0, -200.0),
        Vec3::splat(5.0),
        Layer::Enemy,
    );

    // Fired from inside the player: passes through it and hits the enemy.
    let own = fire(
        &mut app,
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -500.0),
        0.1,
        Layer::PlayerProjectile,
    );
    // Enemy shot fired from inside the enemy: hits the player, not its shooter.
    let hostile = fire(
        &mut app,
        Vec3::new(0.0, 0.0, -199.0),
        Vec3::new(0.0, 0.0, 500.0),
        0.1,
        Layer::EnemyProjectile,
    );
    run(&mut app, 1.0);

    let mut hits: Vec<_> = hits(&app)
        .iter()
        .map(|h| (h.projectile, h.target))
        .collect();
    hits.sort();
    let mut expected = vec![(own, enemy), (hostile, player)];
    expected.sort();
    assert_eq!(hits, expected);
}

#[test]
fn projectiles_do_not_hit_each_other() {
    let mut app = app(60);
    fire(
        &mut app,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -300.0),
        0.5,
        Layer::PlayerProjectile,
    );
    fire(
        &mut app,
        Vec3::new(0.0, 0.0, -300.0),
        Vec3::new(0.0, 0.0, 300.0),
        0.5,
        Layer::EnemyProjectile,
    );
    fire(
        &mut app,
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -300.0),
        0.5,
        Layer::PlayerProjectile,
    );
    run(&mut app, 1.0);
    assert!(hits(&app).is_empty());
}

#[test]
fn unspent_projectiles_expire() {
    let mut app = app(60);
    let shot = app
        .world_mut()
        .spawn(projectile(
            Transform::default(),
            Vec3::NEG_Z * 100.0,
            0.1,
            Layer::PlayerProjectile,
            0.5,
        ))
        .id();
    run(&mut app, 0.4);
    assert!(app.world().get_entity(shot).is_ok());
    run(&mut app, 0.2);
    assert!(app.world().get_entity(shot).is_err());
    assert!(hits(&app).is_empty());
}

fn spawn_player(app: &mut App, at: Vec3, velocity: Vec3) -> Entity {
    app.world_mut()
        .spawn((
            PlayerAircraft,
            aircraft_body(Layer::Player),
            Cruise(velocity),
            Transform::from_translation(at),
        ))
        .id()
}

fn crashes(app: &App) -> Vec<(Entity, Entity)> {
    let seen = app.world().resource::<Seen>();
    seen.crashes
        .iter()
        .map(|c| (c.player, c.obstacle))
        .chain(seen.enemy_crashes.iter().map(|c| (c.enemy, c.obstacle)))
        .collect()
}

#[test]
fn player_crashes_into_buildings() {
    let mut app = app(30);
    let building = spawn_box(
        &mut app,
        Vec3::new(0.0, 50.0, -600.0),
        Vec3::new(10.0, 50.0, 10.0),
        Layer::World,
    );
    // Full speed, 16.7 m per frame, at a 20 m deep building.
    let player = spawn_player(
        &mut app,
        Vec3::new(0.0, 50.0, 0.0),
        Vec3::new(0.0, 0.0, -500.0),
    );
    run(&mut app, 1.5);
    assert_eq!(crashes(&app), vec![(player, building)]);
    // Reported where the jet met the facade (z = -590), not somewhere inside.
    let point = app.world().resource::<Seen>().crashes[0].point;
    assert!((point.z + 590.0).abs() < 1.0, "crash at {point}");
}

#[test]
fn player_crashes_into_the_ground() {
    let mut app = app(30);
    let ground = spawn_ground(&mut app);
    let player = spawn_player(
        &mut app,
        Vec3::new(0.0, 100.0, 0.0),
        Vec3::new(0.0, -100.0, -200.0),
    );
    run(&mut app, 1.5);
    assert_eq!(crashes(&app), vec![(player, ground)]);
}

#[test]
fn thin_deck_and_wall_stop_a_jet_at_500_m_s_and_10_fps() {
    // The jet moves 50 m per frame: one frame before a 1.5 m bridge deck
    // (or a 0.5 m wall), the next frame well past it. Only the swept cast
    // between the two poses can see the impact.
    let cases = [
        // Diving straight down through a deck.
        (
            Vec3::new(0.0, 27.0, 0.0),
            Vec3::new(0.0, -500.0, 0.0),
            Vec3::new(40.0, 0.75, 40.0),
        ),
        // Flying level through a wall.
        (
            Vec3::new(0.0, 0.0, 27.0),
            Vec3::new(0.0, 0.0, -500.0),
            Vec3::new(40.0, 40.0, 0.25),
        ),
    ];
    for (start, velocity, half) in cases {
        let mut app = app(10);
        let obstacle = spawn_box(&mut app, Vec3::ZERO, half, Layer::World);
        let player = spawn_player(&mut app, start, velocity);
        run(&mut app, 0.5);
        assert_eq!(crashes(&app), vec![(player, obstacle)], "{velocity}");
    }
}

#[test]
fn player_and_spitfire_collide() {
    let mut app = app(30);
    let enemy = app
        .world_mut()
        .spawn((
            AircraftModel::Spitfire.body(Layer::Enemy),
            Cruise(Vec3::new(0.0, 0.0, 150.0)),
            Transform::from_xyz(0.0, 300.0, -400.0),
        ))
        .id();
    let player = spawn_player(
        &mut app,
        Vec3::new(0.0, 300.0, 0.0),
        Vec3::new(0.0, 0.0, -150.0),
    );
    run(&mut app, 2.0);
    let mut got = crashes(&app);
    got.sort();
    let mut expected = vec![(player, enemy), (enemy, player)];
    expected.sort();
    assert_eq!(got, expected);
}

#[test]
fn enemy_crashing_into_the_world_is_an_enemy_crash() {
    let mut app = app(30);
    let building = spawn_box(
        &mut app,
        Vec3::new(0.0, 50.0, -300.0),
        Vec3::splat(10.0),
        Layer::World,
    );
    let enemy = app
        .world_mut()
        .spawn((
            AircraftModel::Spitfire.body(Layer::Enemy),
            Cruise(Vec3::new(0.0, 0.0, -300.0)),
            Transform::from_xyz(0.0, 50.0, 0.0),
        ))
        .id();
    run(&mut app, 1.5);
    assert!(app.world().resource::<Seen>().crashes.is_empty());
    assert_eq!(crashes(&app), vec![(enemy, building)]);
}

#[test]
fn being_shot_is_not_a_crash() {
    let mut app = app(60);
    let player = spawn_player(&mut app, Vec3::ZERO, Vec3::ZERO);
    let bullet = fire(
        &mut app,
        Vec3::new(0.0, 0.0, -200.0),
        Vec3::new(0.0, 0.0, 800.0),
        0.1,
        Layer::EnemyProjectile,
    );
    run(&mut app, 0.5);
    assert!(crashes(&app).is_empty());
    let hits = hits(&app);
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].projectile, hits[0].target), (bullet, player));
}

#[test]
fn a_jet_crashes_once_until_it_is_moved_somewhere_new() {
    let mut app = app(30);
    spawn_ground(&mut app);
    let player = spawn_player(
        &mut app,
        Vec3::new(0.0, 30.0, 0.0),
        Vec3::new(0.0, -100.0, -200.0),
    );
    // Keeps going into and through the ground plane: still one crash.
    run(&mut app, 2.0);
    assert_eq!(crashes(&app).len(), 1);

    // Respawn far away; flying into the ground again is a new crash.
    app.world_mut()
        .entity_mut(player)
        .insert(Transform::from_xyz(0.0, 30.0, 5_000.0));
    run(&mut app, 1.0);
    assert_eq!(crashes(&app).len(), 2);
}

#[test]
fn teleport_in_a_frame_without_physics_step_sticks() {
    // dt = 0 (paused game time): Rapier skips the step but still writes its
    // old body pose back. The aircraft must stay where gameplay put it.
    let mut app = app(30);
    let player = spawn_player(&mut app, Vec3::new(0.0, 500.0, 0.0), Vec3::ZERO);
    run(&mut app, 0.2);

    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    let target = Vec3::new(1_000.0, 800.0, 2_000.0);
    app.world_mut()
        .entity_mut(player)
        .insert(Transform::from_translation(target));
    for _ in 0..3 {
        app.update();
        let at = app.world().get::<Transform>(player).unwrap().translation;
        assert_eq!(at, target, "teleport undone while paused");
    }

    app.world_mut().resource_mut::<Time<Virtual>>().unpause();
    for _ in 0..3 {
        app.update();
    }
    let at = app.world().get::<Transform>(player).unwrap().translation;
    assert_eq!(at, target, "teleport undone after resuming");
    assert!(crashes(&app).is_empty(), "a teleport is not a crash");
}

#[test]
fn bullet_at_2000_m_s_and_10_fps_still_hits() {
    // 200 m per step against a 10 m box.
    let mut app = app(10);
    let enemy = spawn_box(
        &mut app,
        Vec3::new(0.0, 0.0, -1_500.0),
        Vec3::splat(5.0),
        Layer::Enemy,
    );
    fire(
        &mut app,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, -2_000.0),
        0.05,
        Layer::PlayerProjectile,
    );
    run(&mut app, 1.5);
    let hits = hits(&app);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].target, enemy);
}

#[test]
fn hit_in_the_last_frame_of_life_still_counts() {
    // At 300 m/s and 30 fps the missile reaches the wall (z = -95) in its
    // tenth frame, exactly when its lifetime of 10 frames runs out.
    let mut app = app(30);
    spawn_box(
        &mut app,
        Vec3::new(0.0, 0.0, -100.0),
        Vec3::splat(5.0),
        Layer::World,
    );
    app.world_mut().spawn(projectile(
        Transform::default(),
        Vec3::new(0.0, 0.0, -300.0),
        0.3,
        Layer::PlayerProjectile,
        10.0 / 30.0,
    ));
    run(&mut app, 1.0);
    let seen = &app.world().resource::<Seen>().hits;
    assert_eq!(seen.len(), 1, "hit lost to expiry");
    assert!(
        seen[0].1,
        "projectile despawned before gameplay could read it"
    );
}

#[test]
fn layer_matrix_is_symmetric_and_as_documented() {
    use Layer::*;
    let expected = [
        (Player, Enemy),
        (Player, EnemyProjectile),
        (Player, World),
        (Enemy, PlayerProjectile),
        (Enemy, World),
        (PlayerProjectile, World),
        (EnemyProjectile, World),
    ];
    for a in Layer::ALL {
        for b in Layer::ALL {
            let listed = expected.contains(&(a, b)) || expected.contains(&(b, a));
            assert_eq!(a.collides_with(b), listed, "{a:?} vs {b:?}");
            // Each side's filter alone already agrees (no one-sided pairs).
            assert_eq!(
                a.filter().contains(b.group()),
                listed,
                "{a:?} filter vs {b:?}"
            );
        }
    }
}
