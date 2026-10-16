//! Headless tests of the city's colliders with Rapier (no window, no renderer).

use std::time::Duration;

use bevy::{
    ecs::system::RunSystemOnce, prelude::*, time::TimeUpdateStrategy, transform::TransformPlugin,
};
use bevy_rapier3d::prelude::*;

use super::{
    CityLayout, CityPlugin,
    colliders::{CityBridge, CityBuilding, CityCollidersPlugin},
    layout::BuildingKind,
    meshes,
};
use crate::{
    flight::PlayerAircraft,
    physics::{
        Ground, Layer, PhysicsPlugin, PlayerCrashed, ProjectileHit, aircraft_body, projectile,
    },
};

#[derive(Resource, Default)]
struct Seen {
    hits: Vec<ProjectileHit>,
    /// Each crash with the jet's position when it was reported.
    crashes: Vec<(PlayerCrashed, Vec3)>,
}

fn record(
    mut hits: MessageReader<ProjectileHit>,
    mut crashes: MessageReader<PlayerCrashed>,
    positions: Query<&Transform>,
    mut seen: ResMut<Seen>,
) {
    seen.hits.extend(hits.read().copied());
    for crash in crashes.read() {
        let at = positions
            .get(crash.player)
            .map_or(Vec3::NAN, |t| t.translation);
        seen.crashes.push((*crash, at));
    }
}

/// Constant velocity, like the flight model would move the jet.
#[derive(Component)]
struct Cruise(Vec3);

fn cruise(time: Res<Time>, mut bodies: Query<(&Cruise, &mut Transform)>) {
    for (cruise, mut transform) in &mut bodies {
        transform.translation += cruise.0 * time.delta_secs();
    }
}

/// The default city with its colliders, physics stepping at 30 fps.
fn city_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        CityPlugin,
        CityCollidersPlugin,
    ))
    .init_resource::<Seen>()
    .add_systems(Update, (record, cruise))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 30.0,
    )));
    app.finish();
    // Startup spawns the colliders, the next update hands them to Rapier.
    app.update();
    app.update();
    app
}

fn run(app: &mut App, seconds: f32) {
    let until = app.world().resource::<Time>().elapsed_secs() + seconds;
    while app.world().resource::<Time>().elapsed_secs() < until {
        app.update();
    }
}

/// First hit along a ray: entity and hit point.
fn ray(app: &mut App, origin: Vec3, dir: Vec3, max: f32) -> Option<(Entity, Vec3)> {
    app.world_mut()
        .run_system_once(move |ctx: ReadRapierContext| {
            ctx.single()
                .unwrap()
                .cast_ray(origin, dir, max, true, QueryFilter::default())
                .map(|(e, toi)| (e, origin + dir * toi))
        })
        .unwrap()
}

fn layout(app: &App) -> CityLayout {
    app.world().resource::<CityLayout>().clone()
}

fn name(app: &App, e: Entity) -> Option<String> {
    app.world().get::<Name>(e).map(|n| n.as_str().to_owned())
}

fn is_ground(app: &App, e: Entity) -> bool {
    app.world().get::<Ground>(e).is_some()
}

#[test]
fn one_static_body_per_building_and_bridge_plus_ground() {
    let mut app = city_app();
    let city = layout(&app);
    let quads = meshes::ground_quads(&city);
    let land = meshes::land_rects(&city, &quads).len();

    let world = app.world_mut();
    let buildings = world.query::<&CityBuilding>().iter(world).count();
    let bridges = world.query::<&CityBridge>().iter(world).count();
    let ground = world.query::<&Ground>().iter(world).count();
    assert_eq!(buildings, city.buildings.len());
    assert_eq!(bridges, city.bridges.len());
    assert_eq!(ground, land + 1, "land boxes + water plane");

    let mut statics = world
        .query_filtered::<(&RigidBody, &CollisionGroups, Option<&ActiveEvents>), With<Collider>>();
    for (body, groups, events) in statics.iter(world) {
        assert_eq!(*body, RigidBody::Fixed);
        assert_eq!(*groups, Layer::World.groups());
        assert!(events.is_none(), "static colliders must not raise events");
    }
}

#[test]
fn rays_land_on_roofs_streets_quays_and_water() {
    let mut app = city_app();
    let city = layout(&app);
    let depth = city.config.river.as_ref().unwrap().water_depth;

    // Roofs: straight down onto the centre of some towers and houses.
    for b in city
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.kind, BuildingKind::Tower | BuildingKind::SetbackTower))
        .step_by(37)
        .take(5)
    {
        let (i, building) = b;
        let (lo, hi) = building.bounds();
        let top = Vec3::new((lo.x + hi.x) / 2.0, 1_000.0, (lo.z + hi.z) / 2.0);
        let (e, p) = ray(&mut app, top, Vec3::NEG_Y, 2_000.0).expect("no hit");
        assert_eq!(app.world().get::<CityBuilding>(e), Some(&CityBuilding(i)));
        assert!(
            (p.y - building.height()).abs() < 0.01,
            "roof at {}, hit {}",
            building.height(),
            p.y
        );
    }

    // Street, quay and water, found through the layout.
    let street = city
        .street_segments()
        .map(|(_, _, r)| r.center())
        .find(|p| !city.on_river(*p) && p.length() < 3_000.0)
        .unwrap();
    // A water cell's centre: open water, no bridge above (bridges only
    // span the street corridors between cells).
    let water = city
        .river
        .iter()
        .map(|&(gx, gz)| {
            let (x0, x1) = city.gap_range(0, gx);
            let (z0, z1) = city.gap_range(1, gz);
            Vec2::new((x0 + x1) / 2.0, (z0 + z1) / 2.0)
        })
        .find(|p| meshes::is_water_at(&city, *p) && p.length() < 5_000.0)
        .unwrap();
    let quay = city
        .river
        .iter()
        .map(|&(gx, gz)| {
            let (z0, z1) = city.gap_range(1, gz);
            Vec2::new(city.gap_range(0, gx).0 + 2.0, (z0 + z1) / 2.0)
        })
        .find(|p| city.on_river(*p) && !meshes::is_water_at(&city, *p))
        .unwrap();
    for (p, want) in [(street, 0.0), (quay, 0.0), (water, -depth)] {
        let (e, hit) = ray(&mut app, Vec3::new(p.x, 500.0, p.y), Vec3::NEG_Y, 1_000.0)
            .unwrap_or_else(|| panic!("nothing below {p}"));
        assert!(is_ground(&app, e), "{p}: hit a non-ground collider");
        assert!(
            (hit.y - want).abs() < 0.01,
            "{p}: ground at {}, want {want}",
            hit.y
        );
    }
}

#[test]
fn the_channel_under_a_bridge_is_open_and_the_deck_is_solid() {
    let mut app = city_app();
    let city = layout(&app);
    let cfg = city.config.river.clone().unwrap();
    for (i, bridge) in city.bridges.iter().enumerate().step_by(9) {
        let deck = bridge.parts[0];
        let line = city.lines[bridge.axis][bridge.line];
        // Along the channel, 10 m below street level, under the bridge.
        let dir = if bridge.axis == 0 { Vec3::X } else { Vec3::Z };
        let mut start = Vec3::new(deck.center.x, -10.0, deck.center.z);
        start[if bridge.axis == 0 { 0 } else { 2 }] = line.center - line.width / 2.0 - 40.0;
        let length = line.width + 80.0;
        assert_eq!(
            ray(&mut app, start, dir, length),
            None,
            "bridge {i}: channel blocked"
        );

        // Up from the water: the deck's underside.
        let below = Vec3::new(deck.center.x, -cfg.water_depth + 1.0, deck.center.z);
        let (e, hit) = ray(&mut app, below, Vec3::Y, 100.0).expect("no deck");
        assert_eq!(app.world().get::<CityBridge>(e), Some(&CityBridge(i)));
        assert!((hit.y - (cfg.deck_height - cfg.deck_thickness)).abs() < 0.01);
    }
}

#[test]
fn missile_at_300_m_s_hits_a_roof_without_tunnelling() {
    let mut app = city_app();
    let city = layout(&app);
    let (i, house) = city
        .buildings
        .iter()
        .enumerate()
        .find(|(_, b)| b.kind == BuildingKind::MidRise && b.parts.len() == 1)
        .unwrap();
    let (lo, hi) = house.bounds();
    let above = Vec3::new((lo.x + hi.x) / 2.0, hi.y + 400.0, (lo.z + hi.z) / 2.0);
    let missile = app
        .world_mut()
        .spawn(projectile(
            Transform::from_translation(above),
            Vec3::new(0.0, -300.0, 0.0),
            0.3,
            Layer::PlayerProjectile,
            10.0,
        ))
        .id();
    run(&mut app, 2.0);
    let hits = app.world().resource::<Seen>().hits.clone();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].projectile, missile);
    assert_eq!(
        app.world().get::<CityBuilding>(hits[0].target),
        Some(&CityBuilding(i))
    );
    assert!(
        (hits[0].point.y - hi.y).abs() < 1.0,
        "stopped at {}",
        hits[0].point.y
    );
}

/// A jet at 500 m/s from `from` along `dir`; returns the first crash and
/// where the jet was then.
fn crash(app: &mut App, from: Vec3, dir: Vec3) -> (PlayerCrashed, Vec3) {
    app.world_mut().spawn((
        PlayerAircraft,
        aircraft_body(Layer::Player),
        Cruise(dir.normalize() * 500.0),
        Transform::from_translation(from).looking_to(dir, Vec3::Y),
    ));
    run(app, 1.5);
    *app.world()
        .resource::<Seen>()
        .crashes
        .first()
        .expect("no crash")
}

#[test]
fn jet_at_500_m_s_crashes_into_the_canal() {
    let mut app = city_app();
    let city = layout(&app);
    let river = city.config.river.clone().unwrap();
    // The stem's southern stretch: a straight channel along Z.
    let &(gx, gz) = city.river.iter().max_by_key(|c| c.1).unwrap();
    let (x0, _) = city.gap_range(0, gx);
    let (x0w, x1w) = (
        x0 + river.quay_width,
        city.gap_range(0, gx).1 - river.quay_width,
    );
    let z = city.gap_range(1, gz).0 + 600.0;
    let centre = Vec2::new((x0w + x1w) / 2.0, z);
    assert!(meshes::is_water_at(&city, centre));

    // Diving steeply into the water: the crash comes at the water level,
    // not at street level.
    let (hit, at) = crash(
        &mut app,
        Vec3::new(centre.x, 150.0, centre.y),
        Vec3::new(0.0, -1.0, -0.3),
    );
    assert!(is_ground(&app, hit.obstacle));
    assert_eq!(name(&app, hit.obstacle).as_deref(), Some("Water collider"));
    // Recorded up to two 16 m frames after the touch: the event comes one
    // physics step late and gameplay reads it in the following frame.
    let water = -river.water_depth;
    let nose = 9.5;
    assert!(
        (water - nose - 2.0 * 17.0..water).contains(&at.y),
        "crashed at {at}, water at {water}"
    );

    // Inside the channel, 10 m below the street, straight at the quay wall.
    let mut app = city_app();
    let (hit, at) = crash(&mut app, Vec3::new(centre.x, -10.0, centre.y), Vec3::NEG_X);
    assert!(is_ground(&app, hit.obstacle), "hit a non-ground collider");
    assert_eq!(name(&app, hit.obstacle).as_deref(), Some("Land collider"));
    // The wingtip (6.5 m out) touches with the centre at x0w + 6.5; the
    // crash is recorded up to two 16.7 m frames later.
    assert!(
        (x0w + 6.5 - 2.0 * 17.0..x0w + 6.5).contains(&at.x),
        "crashed at {at}, wall at x = {x0w}"
    );
}
