//! Headless tests of city damage: the rules, ruins, health, colliders and
//! the chunk rebuild.

use std::{collections::BTreeMap, time::Duration, time::Instant};

use bevy::{
    ecs::system::RunSystemOnce, prelude::*, time::TimeUpdateStrategy, transform::TransformPlugin,
};
use bevy_rapier3d::prelude::*;

use super::{
    CityConfig, CityLayout, CityPlugin, WorldPlugin,
    colliders::{CityBuilding, CityCollidersPlugin},
    damage::{
        BUILDING_TOUGHNESS, BuildingDestroyed, CityDamage, CityDamagePlugin, CityHealth,
        DamageArea, RepairCity, ruin_part,
    },
    layout::Zone,
    meshes,
    rng::Rng,
};
use crate::physics::PhysicsPlugin;

fn city() -> CityLayout {
    CityLayout::generate(&CityConfig::default())
}

/// Distance from `p` to the nearest collision box of building `i`,
/// computed independently of `in_reach`.
fn nearest_box(layout: &CityLayout, i: usize, p: Vec3) -> f32 {
    layout.buildings[i]
        .collision_boxes()
        .map(|(c, h)| {
            let d = Vec3::new(
                (p.x - c.x).abs() - h.x,
                (p.y - c.y).abs() - h.y,
                (p.z - c.z).abs() - h.z,
            );
            d.max(Vec3::ZERO).length()
        })
        .fold(f32::INFINITY, f32::min)
}

fn downtown_point(layout: &CityLayout) -> Vec3 {
    let b = layout
        .buildings
        .iter()
        .find(|b| b.zone == Zone::Downtown)
        .unwrap();
    let base = b.parts[0];
    Vec3::new(base.center.x, 0.0, base.center.z)
}

#[test]
fn area_damage_destroys_exactly_the_buildings_in_reach() {
    let layout = city();
    let mut damage = CityDamage::new(&layout);
    let area = DamageArea {
        point: downtown_point(&layout) + Vec3::Y * 30.0,
        radius: 120.0,
        strength: BUILDING_TOUGHNESS,
    };
    let destroyed = damage.apply(&layout, &area);
    let expected: Vec<usize> = (0..layout.buildings.len())
        .filter(|&i| nearest_box(&layout, i, area.point) <= area.radius)
        .collect();
    assert!(expected.len() >= 3, "too few buildings in reach");
    assert_eq!(destroyed, expected);
    assert_eq!(damage.destroyed_count(), expected.len());
    // A second identical hit destroys nothing new.
    assert!(damage.apply(&layout, &area).is_empty());
}

#[test]
fn damage_accumulates_and_bad_input_is_ignored() {
    let layout = city();
    let mut damage = CityDamage::new(&layout);
    let half = DamageArea {
        point: downtown_point(&layout),
        radius: 40.0,
        strength: BUILDING_TOUGHNESS / 2.0,
    };
    assert!(damage.apply(&layout, &half).is_empty());
    assert!(!damage.apply(&layout, &half).is_empty());

    let mut untouched = CityDamage::new(&layout);
    for bad in [
        DamageArea {
            radius: f32::NAN,
            ..half
        },
        DamageArea {
            strength: -1.0,
            ..half
        },
        DamageArea {
            point: Vec3::NAN,
            ..half
        },
    ] {
        assert!(untouched.apply(&layout, &bad).is_empty());
    }
    assert_eq!(untouched, CityDamage::new(&layout));
}

/// A deterministic raid: hits spread over downtown and midtown.
fn raid(n: usize) -> Vec<DamageArea> {
    let mut rng = Rng::stream(42, 9, 0, 0);
    (0..n)
        .map(|_| DamageArea {
            point: Vec3::new(
                rng.range(-1_200.0, 1_200.0),
                5.0,
                rng.range(-1_200.0, 1_200.0),
            ),
            radius: rng.range(20.0, 80.0),
            strength: rng.range(0.4, 1.2),
        })
        .collect()
}

#[test]
fn same_raid_same_ruins() {
    let layout = city();
    let run = || {
        let mut damage = CityDamage::new(&layout);
        let order: Vec<usize> = raid(60)
            .iter()
            .flat_map(|a| damage.apply(&layout, a))
            .collect();
        (damage, order)
    };
    assert_eq!(run(), run());
}

#[test]
fn city_health_only_falls_and_tracks_the_centre() {
    let layout = city();
    let mut damage = CityDamage::new(&layout);
    let mut health = CityHealth::measure(&layout, &damage);
    assert_eq!(health, CityHealth::default());
    for area in raid(80) {
        damage.apply(&layout, &area);
        let now = CityHealth::measure(&layout, &damage);
        assert!(now.center_intact_ratio <= health.center_intact_ratio);
        assert!(now.intact_ratio <= health.intact_ratio);
        health = now;
    }
    assert!(health.center_intact_ratio < 1.0, "raid missed downtown");
    assert!(health.intact_ratio > 0.5);

    // Flatten downtown: the centre is gone, the rest of the city stands.
    for (i, b) in layout.buildings.iter().enumerate() {
        if b.zone == Zone::Downtown {
            damage.destroyed[i] = true;
        }
    }
    let flat = CityHealth::measure(&layout, &damage);
    assert_eq!(flat.center_intact_ratio, 0.0);
    assert!(flat.intact_ratio > 0.3);
}

#[test]
fn ruins_are_low_stumps_on_the_old_footprint() {
    let layout = city();
    for b in layout.buildings.iter().step_by(97) {
        let ruin = ruin_part(b);
        let base = b.parts[0];
        assert_eq!(ruin.min().y, 0.0);
        assert!(ruin.max().y <= b.height().min(18.0) + 1e-3);
        assert_eq!(ruin.center.xz(), base.center.xz());
        assert_eq!(ruin.half_size.xz(), base.half_size.xz());
    }
}

#[test]
fn rebuilding_a_chunk_is_fast_and_shows_the_ruins() {
    let layout = city();
    // The chunk with the most buildings.
    let mut per_chunk: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    for (i, b) in layout.buildings.iter().enumerate() {
        per_chunk
            .entry(meshes::chunk_of(&layout, &b.parts))
            .or_default()
            .push(i);
    }
    let (&chunk, members) = per_chunk.iter().max_by_key(|(_, v)| v.len()).unwrap();
    let mut destroyed = vec![false; layout.buildings.len()];
    let intact = meshes::chunk_meshes(&layout, &destroyed, chunk);
    for &i in members.iter().step_by(2) {
        destroyed[i] = true;
    }

    let runs = 5;
    let started = Instant::now();
    let mut ruined = Default::default();
    for _ in 0..runs {
        ruined = meshes::chunk_meshes(&layout, &destroyed, chunk);
    }
    let ms = started.elapsed().as_secs_f64() * 1e3 / f64::from(runs);
    println!(
        "chunk {chunk:?}: {} buildings, rebuild {ms:.2} ms",
        members.len()
    );
    assert!(ms < 50.0, "chunk rebuild took {ms:.2} ms");

    let tris = |m: &[meshes::MeshData; 3]| m.iter().map(|d| d.triangle_count()).sum::<usize>();
    assert!(tris(&ruined) < tris(&intact));
    // Matches the whole-city build with the same ruins.
    let whole = meshes::building_meshes_with(&layout, &destroyed);
    for (s, surface) in meshes::SURFACES.into_iter().enumerate() {
        let n = whole
            .get(&(chunk, surface))
            .map_or(0, |d| d.triangle_count());
        assert_eq!(ruined[s].triangle_count(), n, "{surface:?}");
    }
}

#[test]
fn intact_meshes_are_unchanged_by_the_damage_path() {
    let layout = city();
    let plain = meshes::building_meshes(&layout);
    let with = meshes::building_meshes_with(&layout, &vec![false; layout.buildings.len()]);
    assert_eq!(plain.len(), with.len());
    for (k, d) in &plain {
        assert_eq!(d.positions, with[k].positions);
    }
}

// --- ECS: messages, colliders, repair -------------------------------------

#[derive(Resource, Default)]
struct Seen(Vec<BuildingDestroyed>);

fn record(mut destroyed: MessageReader<BuildingDestroyed>, mut seen: ResMut<Seen>) {
    seen.0.extend(destroyed.read().copied());
}

fn damage_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        CityPlugin,
        CityCollidersPlugin,
        CityDamagePlugin,
    ))
    .init_resource::<Seen>()
    .add_systems(Update, record)
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 30.0,
    )));
    app.finish();
    app.update();
    app.update();
    app
}

fn roof_height(app: &mut App, xz: Vec2) -> Option<(Entity, f32)> {
    let origin = Vec3::new(xz.x, 1_000.0, xz.y);
    app.world_mut()
        .run_system_once(move |ctx: ReadRapierContext| {
            ctx.single()
                .unwrap()
                .cast_ray(origin, Vec3::NEG_Y, 2_000.0, true, QueryFilter::default())
                .map(|(e, toi)| (e, origin.y - toi))
        })
        .unwrap()
}

#[test]
fn destroyed_buildings_become_stump_colliders_and_repair_restores_them() {
    let mut app = damage_app();
    let layout = app.world().resource::<CityLayout>().clone();
    let (i, tower) = layout
        .buildings
        .iter()
        .enumerate()
        .find(|(_, b)| b.zone == Zone::Downtown && b.height() > 100.0)
        .unwrap();
    let xz = tower.parts[0].center.xz();
    let (_, before) = roof_height(&mut app, xz).unwrap();
    assert!((before - tower.height()).abs() < 0.01);

    app.world_mut().write_message(DamageArea {
        point: Vec3::new(xz.x, 50.0, xz.y),
        radius: 1.0,
        strength: 1.0,
    });
    app.update();
    app.update();

    let seen = app.world().resource::<Seen>().0.clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].index, i);
    assert_eq!(seen[0].zone, Zone::Downtown);
    let health = *app.world().resource::<CityHealth>();
    assert!(health.center_intact_ratio < 1.0);

    let (e, after) = roof_height(&mut app, xz).unwrap();
    assert_eq!(app.world().get::<CityBuilding>(e), Some(&CityBuilding(i)));
    assert!(
        (after - ruin_part(tower).max().y).abs() < 0.01,
        "stump at {after}"
    );

    app.world_mut().write_message(RepairCity);
    app.update();
    app.update();
    let (_, repaired) = roof_height(&mut app, xz).unwrap();
    assert!((repaired - tower.height()).abs() < 0.01);
    assert_eq!(*app.world().resource::<CityHealth>(), CityHealth::default());
    assert_eq!(app.world().resource::<CityDamage>().destroyed_count(), 0);
}

#[test]
fn smoke_and_craters_appear_and_go_with_repair() {
    let mut app = crate::test_support::headless_app(WorldPlugin);
    app.update();
    let layout = app.world().resource::<CityLayout>().clone();
    let target = downtown_point(&layout);
    // A street: some paved segment away from the river.
    let street = layout
        .street_segments()
        .map(|(_, _, r)| r.center())
        .find(|p| !layout.on_river(*p) && p.length() < 2_000.0)
        .unwrap();
    app.world_mut().write_message(DamageArea {
        point: target + Vec3::Y * 10.0,
        radius: 60.0,
        strength: 1.0,
    });
    app.world_mut().write_message(DamageArea {
        point: Vec3::new(street.x, 0.0, street.y),
        radius: 10.0,
        strength: 1.0,
    });
    app.update();
    app.update();

    let count = |app: &mut App, name: &str| {
        let world = app.world_mut();
        world
            .query::<&Name>()
            .iter(world)
            .filter(|n| n.as_str() == name)
            .count()
    };
    let destroyed = app.world().resource::<CityDamage>().destroyed_count();
    assert!(destroyed > 0);
    assert_eq!(count(&mut app, "Smoke column"), destroyed.min(48));
    assert_eq!(count(&mut app, "Crater"), 2);

    app.world_mut().write_message(RepairCity);
    app.update();
    app.update();
    assert_eq!(count(&mut app, "Smoke column"), 0);
    assert_eq!(count(&mut app, "Crater"), 0);
}

#[test]
fn downtown_lies_around_the_centre_within_its_radius() {
    let layout = city();
    let r = layout.config.downtown_radius_m();
    assert!((800.0..1_000.0).contains(&r), "{r}");
    let slack = layout.config.zone_noise * layout.config.city_radius;
    for b in layout.buildings.iter().filter(|b| b.zone == Zone::Downtown) {
        assert!(b.parts[0].center.xz().length() < r + slack + 200.0);
    }
}
