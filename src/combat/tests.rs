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
    aircraft::AircraftModel,
    flight::{FlightState, PlayerAircraft},
    physics::{Layer, PhysicsPlugin, ProjectileHit, aircraft_body},
    world::{
        CityLayout, CityPlugin,
        colliders::{CityBuilding, CityCollidersPlugin},
    },
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
            Hitbox {
                radius: 5.5,
                gun_radius: 0.0,
            },
            AircraftVelocity::default(),
            AircraftModel::Me309.body(Layer::Enemy),
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
            Hitbox {
                radius: 5.5,
                gun_radius: 0.0,
            },
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
    assert_eq!(
        crashes[0].cause,
        DestroyCause::Crash(super::CrashInto::Obstacle)
    );
    assert_eq!(crashes[0].team, Team::Player);
}

/// Hits recorded in `Update`, as the combat code sees them.
#[derive(Resource, Default)]
struct WorldHits(Vec<ProjectileHit>);

fn record_hits(mut hits: MessageReader<ProjectileHit>, mut seen: ResMut<WorldHits>) {
    seen.0.extend(hits.read().copied());
}

#[test]
fn a_real_city_building_stops_gun_rounds() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        CombatPlugin,
        CityPlugin,
        CityCollidersPlugin,
    ))
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<WorldHits>()
    .add_systems(Update, record_hits)
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / FPS,
    )));
    app.finish();
    app.update();

    // The tallest building; shoot at it along +X from the street in front
    // of its west face, with an enemy right behind its east face.
    let layout = app.world().resource::<CityLayout>().clone();
    let (lo, hi) = layout
        .buildings
        .iter()
        .map(|b| b.bounds())
        .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .unwrap();
    let height = hi.y * 0.3;
    let z = (lo.z + hi.z) / 2.0;
    // The shooter has no collider: only its rounds matter here.
    app.world_mut().spawn((
        PlayerAircraft,
        FlightState::default(),
        Transform::from_xyz(lo.x - 15.0, height, z).looking_to(Vec3::X, Vec3::Y),
    ));
    let enemy = spawn_target(&mut app, Vec3::new(hi.x + 30.0, height, z));
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    run_for(&mut app, 1.0);

    assert_eq!(app.world().get::<Health>(enemy).unwrap().current, 50.0);
    let hits = app.world().resource::<WorldHits>().0.clone();
    let on_building = hits
        .iter()
        .filter(|h| app.world().get::<CityBuilding>(h.target).is_some())
        .count();
    assert!(
        on_building > 10,
        "{on_building} of {} hits on the building",
        hits.len()
    );
}

#[test]
fn gun_lead_meets_the_target() {
    let cases = [
        // (to target, shooter velocity, target velocity)
        (
            Vec3::new(0.0, 0.0, -500.0),
            Vec3::ZERO,
            Vec3::new(170.0, 0.0, 0.0),
        ),
        (
            Vec3::new(200.0, 50.0, -700.0),
            Vec3::NEG_Z * 300.0,
            Vec3::new(-150.0, 20.0, 60.0),
        ),
        (
            Vec3::new(0.0, 0.0, -300.0),
            Vec3::NEG_Z * 450.0,
            Vec3::NEG_Z * 160.0,
        ),
    ];
    let speed = CombatConfig::default().player_gun.muzzle_speed;
    for (to_target, shooter, target) in cases {
        let (direction, t) = super::gun_lead(to_target, shooter, target, speed).unwrap();
        assert!((direction.length() - 1.0).abs() < 1e-4);
        let round = (shooter + direction * speed) * t;
        let enemy = to_target + target * t;
        assert!(
            round.distance(enemy) < 0.05,
            "miss by {} m",
            round.distance(enemy)
        );
    }
    // A target running away faster than the rounds cannot be caught.
    assert!(
        super::gun_lead(
            Vec3::NEG_Z * 500.0,
            Vec3::ZERO,
            Vec3::NEG_Z * 2_000.0,
            speed
        )
        .is_none()
    );
}

/// A target flying straight at constant speed.
#[derive(Component)]
struct Cruise(Vec3);

fn cruise(time: Res<Time>, mut targets: Query<(&Cruise, &mut Transform, &mut AircraftVelocity)>) {
    for (cruise, mut transform, mut velocity) in &mut targets {
        velocity.0 = cruise.0;
        transform.translation += cruise.0 * time.delta_secs();
    }
}

/// Keeps the jet's nose on the gun lead (or straight on the target),
/// like a pilot holding the reticle on the pipper.
#[derive(Resource)]
struct AimWithLead(bool);

fn aim_at_target(
    lead: Res<AimWithLead>,
    targets: Query<(&Transform, &AircraftVelocity), With<Cruise>>,
    mut jet: Query<&mut Transform, (With<PlayerAircraft>, Without<Cruise>)>,
) {
    let (Ok((target, velocity)), Ok(mut jet)) = (targets.single(), jet.single_mut()) else {
        return;
    };
    let to_target = target.translation - jet.translation;
    let speed = CombatConfig::default().player_gun.muzzle_speed;
    let direction = if lead.0 {
        super::gun_lead(to_target, Vec3::ZERO, velocity.0, speed)
            .unwrap()
            .0
    } else {
        to_target.normalize()
    };
    jet.look_to(direction, Vec3::Y);
}

fn crossing_target_destroyed(lead: bool) -> bool {
    let mut app = app();
    app.insert_resource(AimWithLead(lead)).add_systems(
        Update,
        (cruise, aim_at_target)
            .chain()
            .before(super::CombatSystems::Aim),
    );
    spawn_player(&mut app, high_up());
    // A fighter crossing 500 m ahead at 170 m/s.
    let target = spawn_target(&mut app, Vec3::new(-150.0, 1_000.0, -500.0));
    app.world_mut()
        .entity_mut(target)
        .insert(Cruise(Vec3::X * 170.0));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    run_for(&mut app, 1.5);
    app.world().get_entity(target).is_err()
}

#[test]
fn aiming_at_the_lead_point_hits_a_crossing_fighter_and_aiming_at_it_does_not() {
    assert!(crossing_target_destroyed(true), "lead aim missed");
    assert!(!crossing_target_destroyed(false), "direct aim hit anyway");
}

#[test]
fn kills_explode_and_hits_flash_where_they_happen() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let enemy = spawn_target(&mut app, Vec3::new(0.0, 1_000.0, -500.0));
    let point = Vec3::new(1.0, 1_000.0, -495.0);
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: enemy,
        point,
        team: Team::Player,
        weapon: Weapon::Missile,
        damage: 100.0,
    });
    app.update();
    let effects = |app: &mut App| {
        app.world_mut()
            .query::<(&super::Effect, &Transform)>()
            .iter(app.world())
            .map(|(e, t)| (e.kind, t.translation))
            .collect::<Vec<_>>()
    };
    let now = effects(&mut app);
    assert!(
        now.contains(&(super::EffectKind::MissileBurst, point)),
        "{now:?}"
    );
    assert!(
        now.iter()
            .any(|(kind, at)| *kind == super::EffectKind::Explosion
                && at.distance(Vec3::new(0.0, 1_000.0, -500.0)) < 1.0),
        "{now:?}"
    );

    // The player's own crash explodes too, at the impact point.
    let crash_at = Vec3::new(3.0, 999.0, -2.0);
    app.world_mut()
        .write_message(crate::physics::PlayerCrashed {
            player: jet,
            obstacle: Entity::PLACEHOLDER,
            point: crash_at,
        });
    app.update();
    assert!(effects(&mut app).contains(&(super::EffectKind::BigExplosion, crash_at)));

    // All gone once played.
    run_for(&mut app, super::EffectKind::BigExplosion.lifetime() + 0.1);
    assert!(effects(&mut app).is_empty());
}

/// The city with its colliders and the combat plugin, for crash tests.
fn city_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        CombatPlugin,
        CityPlugin,
        CityCollidersPlugin,
    ))
    .init_resource::<ButtonInput<KeyCode>>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / FPS,
    )));
    app.finish();
    app.update();
    app
}

fn crash_cause_at(position: Vec3) -> Option<DestroyCause> {
    let mut app = city_app();
    spawn_player(&mut app, Transform::from_translation(position));
    // Crashes are resolved in the frame they happen, possibly already in
    // the update that armed the jet.
    for _ in 0..5 {
        if let Some(d) = read::<Destroyed>(&app).first() {
            return Some(d.cause);
        }
        app.update();
    }
    None
}

#[test]
fn crashes_name_what_was_hit() {
    let layout = city_app().world().resource::<CityLayout>().clone();
    let (lo, hi) = layout
        .buildings
        .iter()
        .map(|b| b.bounds())
        .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .unwrap();
    let in_building = Vec3::new((lo.x + hi.x) / 2.0, hi.y * 0.5, (lo.z + hi.z) / 2.0);
    assert_eq!(
        crash_cause_at(in_building),
        Some(DestroyCause::Crash(super::CrashInto::Building))
    );

    // Centre of a river cell between two street lines, at the water.
    let lines = &layout.lines;
    let &(gx, gz) = layout
        .river
        .iter()
        .find(|(gx, gz)| *gx > 0 && *gz > 0 && *gx < lines[0].len() && *gz < lines[1].len())
        .expect("an inner river cell");
    let x = (lines[0][gx - 1].max() + lines[0][gx].min()) / 2.0;
    let z = (lines[1][gz - 1].max() + lines[1][gz].min()) / 2.0;
    let depth = layout.config.river.as_ref().unwrap().water_depth;
    assert_eq!(
        crash_cause_at(Vec3::new(x, -depth + 0.5, z)),
        Some(DestroyCause::Crash(super::CrashInto::Water))
    );
}

#[test]
fn next_target_key_cycles_through_the_enemies_in_the_cone() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let ahead = |degrees: f32| {
        let dir = Quat::from_rotation_y(degrees.to_radians()) * Vec3::NEG_Z;
        high_up().translation + dir * 1_200.0
    };
    // Nearest the nose first: centre, then 8° left, then 15° right.
    let centre = spawn_target(&mut app, ahead(0.0));
    let left = spawn_target(&mut app, ahead(8.0));
    let right = spawn_target(&mut app, ahead(-15.0));
    spawn_target(&mut app, ahead(90.0)); // outside the cone
    app.update();
    let target = |app: &App| app.world().get::<LockOn>(jet).unwrap().target();
    assert_eq!(target(&app), Some(centre), "starts on the nose");

    let mut seen = Vec::new();
    for _ in 0..4 {
        press_key(&mut app, KeyCode::Tab);
        seen.push(target(&app).unwrap());
    }
    assert_eq!(seen, vec![left, right, centre, left]);
    // The right Alt/Option key does the same.
    press_key(&mut app, KeyCode::AltRight);
    assert_eq!(target(&app), Some(right));
}

#[test]
fn after_a_missile_launch_the_seeker_moves_to_the_next_enemy() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let first = spawn_target(&mut app, Vec3::new(0.0, 1_000.0, -1_500.0));
    let second = spawn_target(&mut app, Vec3::new(200.0, 1_000.0, -1_600.0));
    run_for(&mut app, 1.2);
    assert_eq!(
        app.world().get::<LockOn>(jet).unwrap().state,
        LockState::Locked { target: first }
    );
    launch_at(&mut app, jet, None);
    app.update();
    app.update();
    assert_eq!(
        app.world().get::<LockOn>(jet).unwrap().target(),
        Some(second),
        "seeker moved on"
    );
    let missile_target = app
        .world_mut()
        .query::<&super::Missile>()
        .single(app.world())
        .unwrap()
        .target;
    assert_eq!(missile_target, Some(first));
}

/// Presses and releases `key` over one update.
fn press_key(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

#[test]
fn bombs_burst_on_the_ruin_not_on_the_old_roof() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        CombatPlugin,
        crate::ai::AiPlugin,
        CityPlugin,
        CityCollidersPlugin,
        crate::world::damage::CityDamagePlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / FPS,
    )));
    app.finish();
    app.update();

    let layout = app.world().resource::<CityLayout>().clone();
    let (index, tallest) = layout
        .buildings
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.height().total_cmp(&b.1.height()))
        .unwrap();
    let centre = tallest.parts[0].center;
    let roof = tallest.height();
    let map_height = |app: &App| {
        app.world()
            .resource::<crate::ai::ObstacleMap>()
            .height_at(centre.x, centre.z)
    };
    assert!(map_height(&app) >= roof - 0.1);

    // Destroy it, let colliders and the AI's map catch up.
    app.world_mut()
        .write_message(crate::world::damage::DamageArea {
            point: Vec3::new(centre.x, roof * 0.5, centre.z),
            radius: 1.0,
            strength: 1.0,
        });
    for _ in 0..5 {
        app.update();
    }
    assert!(
        app.world()
            .resource::<crate::world::damage::CityDamage>()
            .destroyed[index]
    );
    assert!(
        map_height(&app) < roof - 1.0,
        "AI map still has the old roof"
    );

    // A bomb dropped right above it bursts on the stump (at most 18 m).
    app.world_mut().spawn(super::bomb_bundle(
        super::BombSpec::HEAVY,
        Vec3::new(centre.x, roof + 50.0, centre.z),
        Vec3::ZERO,
    ));
    let mut impact = None;
    for _ in 0..(10 * 60) {
        app.update();
        if let Some(i) = read::<super::BombImpact>(&app).first() {
            impact = Some(i.point);
            break;
        }
    }
    let impact = impact.expect("bomb never landed");
    assert!(
        impact.y < 20.0,
        "burst at {:.0} m (roof was {roof:.0} m)",
        impact.y
    );
}

/// Fires one player round from 300 m behind `kind`, passing `offset` from
/// its centre (in its frame), and tells whether it hit.
fn round_hits(kind: crate::ai::EnemyKind, gun_radius: f32, offset: Vec3) -> bool {
    let mut app = app();
    let target = app
        .world_mut()
        .spawn((
            Team::Enemy,
            Health::full(1_000.0),
            Hitbox {
                radius: 5.0,
                gun_radius,
            },
            AircraftVelocity::default(),
            kind.model().body(Layer::Enemy),
            Transform::from_xyz(0.0, 1_000.0, -300.0),
        ))
        .id();
    app.update();
    let start = Vec3::new(0.0, 1_000.0, 0.0) + offset;
    let shooter = app.world_mut().spawn_empty().id();
    app.world_mut().spawn(super::weapons::round_bundle(
        shooter,
        Team::Player,
        &super::GunSpec::JET_CANNON,
        start,
        Vec3::NEG_Z * super::GunSpec::JET_CANNON.muzzle_speed,
    ));
    for _ in 0..30 {
        app.update();
    }
    app.world().get::<Health>(target).unwrap().current < 1_000.0
}

#[test]
fn gun_rounds_count_on_the_whole_silhouette_wings_included() {
    use crate::ai::EnemyKind;
    let fighter = EnemyKind::Me309.gun_radius();
    let bomber = EnemyKind::He177.gun_radius();
    // Out on the wing, inside the model's silhouette (its box reaches
    // 1.75 m up) but above the collider's thin wing plate.
    let wing = Vec3::new(4.0, 1.2, 0.0);
    assert!(
        !round_hits(EnemyKind::Me309, 0.0, wing),
        "the collider alone takes it"
    );
    assert!(
        round_hits(EnemyKind::Me309, fighter, wing),
        "wing hit not counted"
    );
    assert!(
        round_hits(EnemyKind::Me309, fighter, Vec3::ZERO),
        "centre hit"
    );
    // Clearly outside the span: no hit.
    assert!(!round_hits(
        EnemyKind::Me309,
        fighter,
        Vec3::new(fighter + 1.5, 0.0, 0.0)
    ));
    // A bomber's outer wing.
    assert!(round_hits(
        EnemyKind::He177,
        bomber,
        Vec3::new(12.0, 0.8, 0.0)
    ));
    assert!(
        (fighter - 5.33).abs() < 0.01 && (bomber - 15.59).abs() < 0.05,
        "{fighter} {bomber}"
    );
}

#[test]
fn enemy_rounds_still_need_the_jets_collider() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let enemy = app.world_mut().spawn_empty().id();
    // Passes 5 m above the jet: inside a 6 m sphere, outside its body.
    let start = high_up().translation + Vec3::new(0.0, 5.0, -300.0);
    app.world_mut().spawn(super::weapons::round_bundle(
        enemy,
        Team::Enemy,
        &super::GunSpec::WWII_GUNS,
        start,
        Vec3::Z * super::GunSpec::WWII_GUNS.muzzle_speed,
    ));
    for _ in 0..40 {
        app.update();
    }
    let health = app.world().get::<Health>(jet).unwrap();
    assert_eq!(health.current, health.max);
}

#[test]
fn segment_and_capsule_sweeps_find_the_closest_approach() {
    use super::hits::{closest_points_on_segments, sweep_hits_capsule};
    // Crossing segments.
    let (a, b) = closest_points_on_segments(
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 2.0, -1.0),
        Vec3::new(0.0, 2.0, 1.0),
    );
    assert!(a.distance(Vec3::ZERO) < 1e-5 && b.distance(Vec3::new(0.0, 2.0, 0.0)) < 1e-5);
    // A round flying across a long capsule near its end hits, past the end misses.
    let (c0, c1) = (Vec3::new(0.0, 0.0, -100.0), Vec3::new(0.0, 0.0, 100.0));
    let across = |z: f32| {
        sweep_hits_capsule(
            Vec3::new(-50.0, 0.0, z),
            Vec3::new(50.0, 0.0, z),
            c0,
            c1,
            20.0,
        )
    };
    assert!(across(110.0).is_some(), "rounded end");
    assert!(across(125.0).is_none(), "beyond the end");
}

#[test]
fn the_zeppelin_is_hit_along_its_whole_length() {
    use crate::ai::{EnemyKind, EnemyTuning, bomber_bundle, zeppelin_parts};
    let hit_at = |along: f32| {
        let mut app = app();
        // The airship lies across the round's path, nose −Z.
        let target = app
            .world_mut()
            .spawn((
                bomber_bundle(
                    EnemyKind::Zeppelin,
                    EnemyTuning::default(),
                    Transform::from_xyz(0.0, 1_000.0, -600.0),
                    0,
                ),
                zeppelin_parts(),
            ))
            .id();
        app.update();
        let shooter = app.world_mut().spawn_empty().id();
        // From the side (−X), crossing the hull `along` metres from its centre.
        app.world_mut().spawn(super::weapons::round_bundle(
            shooter,
            Team::Player,
            &super::GunSpec::JET_CANNON,
            Vec3::new(-300.0, 1_000.0, -600.0 + along),
            Vec3::X * super::GunSpec::JET_CANNON.muzzle_speed,
        ));
        for _ in 0..30 {
            app.update();
        }
        let health = app.world().get::<Health>(target).unwrap();
        health.current < health.max
    };
    assert!(hit_at(0.0), "middle");
    assert!(hit_at(-110.0), "bow");
    assert!(hit_at(110.0), "stern");
    assert!(!hit_at(160.0), "past the stern");
}

// --- Missile bursts and smoke trails ---------------------------------------

fn effects_of(app: &mut App, kind: super::EffectKind) -> Vec<Vec3> {
    app.world_mut()
        .query::<(&super::Effect, &Transform)>()
        .iter(app.world())
        .filter(|(e, _)| e.kind == kind)
        .map(|(_, t)| t.translation)
        .collect()
}

fn hit(app: &mut App, target: Entity, point: Vec3, weapon: Weapon, damage: f32) {
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target,
        point,
        team: Team::Player,
        weapon,
        damage,
    });
    app.update();
}

#[test]
fn every_missile_hit_bursts_even_when_the_target_survives() {
    let mut app = app();
    spawn_player(&mut app, high_up());
    // A tough target: one missile is not enough.
    let bomber = spawn_target(&mut app, Vec3::new(0.0, 1_000.0, -500.0));
    app.world_mut().get_mut::<Health>(bomber).unwrap().current = 50.0;
    let point = Vec3::new(2.0, 1_000.0, -497.0);
    hit(&mut app, bomber, point, Weapon::Missile, 30.0);
    assert!(!app.world().get::<Health>(bomber).unwrap().is_dead());
    assert_eq!(
        effects_of(&mut app, super::EffectKind::MissileBurst),
        vec![point]
    );
    assert!(effects_of(&mut app, super::EffectKind::Explosion).is_empty());

    // A gun round only flashes.
    let gun_at = Vec3::new(-1.0, 1_000.0, -498.0);
    hit(&mut app, bomber, gun_at, Weapon::Gun, 1.0);
    assert!(effects_of(&mut app, super::EffectKind::Spark).contains(&gun_at));
    assert_eq!(
        effects_of(&mut app, super::EffectKind::MissileBurst).len(),
        1
    );

    // The killing missile: its burst plus the big explosion.
    hit(&mut app, bomber, point, Weapon::Missile, 30.0);
    assert_eq!(
        effects_of(&mut app, super::EffectKind::MissileBurst).len(),
        2
    );
    assert_eq!(effects_of(&mut app, super::EffectKind::Explosion).len(), 1);
    assert!(super::EffectKind::MissileBurst.size() < super::EffectKind::Explosion.size());
}

#[test]
fn a_missile_striking_a_building_bursts_a_gun_round_does_not() {
    let mut app = app();
    let jet = spawn_player(&mut app, high_up());
    let wall = spawn_world_box(&mut app, Vec3::new(0.0, 50.0, -300.0), Vec3::splat(50.0));
    let munition = |weapon| Munition {
        shooter: jet,
        team: Team::Player,
        weapon,
        damage: 50.0,
        fuse_radius: 0.0,
        last_position: Vec3::ZERO,
    };
    let missile = app.world_mut().spawn(munition(Weapon::Missile)).id();
    let round = app.world_mut().spawn(munition(Weapon::Gun)).id();
    let at = Vec3::new(0.0, 60.0, -250.0);
    for projectile in [missile, round] {
        app.world_mut().write_message(ProjectileHit {
            projectile,
            target: wall,
            point: at,
        });
    }
    app.update();
    assert_eq!(
        effects_of(&mut app, super::EffectKind::MissileBurst),
        vec![at]
    );
    assert!(effects_of(&mut app, super::EffectKind::Spark).is_empty());
}

fn active_puffs(app: &mut App) -> Vec<Vec3> {
    app.world_mut()
        .query::<(&super::smoke::SmokePuff, &Transform)>()
        .iter(app.world())
        .filter(|(p, _)| p.is_active())
        .map(|(_, t)| t.translation)
        .collect()
}

/// Puffs after `seconds` behind one enemy at `fraction` of its health.
fn smoke_after(fraction: f32, seconds: f32) -> (App, Entity, Vec<Vec3>) {
    let mut app = app();
    let at = Vec3::new(0.0, 1_000.0, -500.0);
    let enemy = spawn_target(&mut app, at);
    {
        let mut health = app.world_mut().get_mut::<Health>(enemy).unwrap();
        health.current = health.max * fraction;
    }
    run_for(&mut app, seconds);
    let puffs = active_puffs(&mut app);
    (app, enemy, puffs)
}

#[test]
fn enemies_below_half_health_trail_smoke_more_the_worse_they_are() {
    let (_, _, healthy) = smoke_after(0.8, 1.0);
    assert!(healthy.is_empty(), "smoke above half health");
    let (_, _, hurt) = smoke_after(0.4, 1.0);
    let (_, _, wrecked) = smoke_after(0.05, 1.0);
    assert!(!hurt.is_empty(), "no smoke at 40 %");
    assert!(
        wrecked.len() > hurt.len() * 2,
        "{} vs {}",
        wrecked.len(),
        hurt.len()
    );
    // At the aircraft's tail, rising a little.
    let at = Vec3::new(0.0, 1_000.0, -500.0);
    assert!(hurt.iter().all(|p| p.distance(at) < 20.0), "{hurt:?}");
}

#[test]
fn smoke_comes_from_a_fixed_pool_and_clears_after_the_aircraft_is_gone() {
    let (mut app, enemy, _) = smoke_after(0.0, 0.1);
    // Nearly dead for a long time: never more puffs than the pool.
    run_for(&mut app, 5.0);
    let pool = app
        .world_mut()
        .query::<&super::smoke::SmokePuff>()
        .iter(app.world())
        .count();
    assert_eq!(pool, super::smoke::POOL_SIZE);
    assert!(!active_puffs(&mut app).is_empty());

    app.world_mut().despawn(enemy);
    run_for(&mut app, 2.0);
    assert!(
        active_puffs(&mut app).is_empty(),
        "smoke outlived its puffs"
    );
}
