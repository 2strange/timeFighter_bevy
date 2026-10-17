//! Bombers: fly at the city centre in formation, drop a stick of bombs on
//! their aim point, fly on, turn back for the next run, until shot down.
//! They do not dodge; their turret gunners defend them.

use bevy::prelude::*;

use super::{
    AiConfig, Enemy, EnemyFlight, EnemyKind, EnemyPilot, EnemyTuning, FlightGroup, ObstacleMap,
    avoid_terrain, fly, keep_in_arena, tactics,
};
use crate::{
    combat::{
        AircraftType, AircraftVelocity, BombSpec, FallsWhenDestroyed, GoingDown, Health,
        HitCapsule, Hitbox, Team, TurretMount, Turrets, bomb_bundle, fall_time,
    },
    physics::Layer,
};

/// Bombs per run and the gap between two of them in a stick (s).
pub const BOMBS_PER_RUN: u32 = 6;
pub const DROP_INTERVAL: f32 = 0.35;

/// Bombs a `kind` drops per run and the gap between two (s): the zeppelin
/// lays a long carpet.
pub fn stick(kind: EnemyKind) -> (u32, f32) {
    match kind {
        EnemyKind::Zeppelin => (20, 0.6),
        _ => (BOMBS_PER_RUN, DROP_INTERVAL),
    }
}

/// Radius of the zeppelin's hull in its middle part (m).
const ZEPPELIN_HULL_RADIUS: f32 = 21.8;

/// The zeppelin's hit zone along its length and its burning fall: it goes
/// down slowly where it is shot and wrecks what lies below (r 150 m).
pub fn zeppelin_parts() -> impl Bundle {
    // The hull's middle radius (the model's own collider uses the same);
    // the capsule's round ends reach nose and tail.
    let size = EnemyKind::Zeppelin.model().size();
    let radius = ZEPPELIN_HULL_RADIUS;
    (
        HitCapsule {
            half_length: 0.5 * size.length() - radius,
            radius,
        },
        FallsWhenDestroyed {
            impact_radius: 200.0,
            impact_strength: 1.0,
        },
    )
}

/// Where a gunner named `name` looks (model frame) and how wide, or `None`
/// if this kind does not man that position.
fn mount_facing(kind: EnemyKind, name: &str) -> Option<(Vec3, f32)> {
    let wide = 80_f32.to_radians();
    let cone = 75_f32.to_radians();
    if kind == EnemyKind::Zeppelin {
        // All round on the airship.
        if name.contains("top") {
            return Some((Vec3::Y, wide));
        }
        if name.contains("tail") {
            return Some((Vec3::Z, cone));
        }
        if name.contains("front") {
            return Some((Vec3::NEG_Z, wide));
        }
        if name.contains("rear") {
            return Some((Vec3::Z, wide));
        }
        if name.contains("gondola") {
            return Some((Vec3::NEG_Y, wide));
        }
        return None;
    }
    if name.contains("front") {
        Some((Vec3::NEG_Z, cone))
    } else if name.contains("rear") {
        Some((Vec3::Z, cone))
    } else {
        None
    }
}
/// After a run the flight flies on this long (s) before turning back.
pub const EGRESS_TIME: f32 = 25.0;
/// Aim points lie within this distance of the city centre (m).
pub const TARGET_SPREAD: f32 = 450.0;
/// The stick starts once the bombs would land this close to the aim
/// point (m).
const BOMB_ZONE: f32 = 220.0;
/// Turret gunners open fire within this distance (m).
const TURRET_RANGE: f32 = 650.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BomberPhase {
    /// Flying at the aim point; drops the stick over it.
    Approach,
    /// Flying on after the run, then turning back.
    Egress,
}

/// A bomber's state. Wingmen copy their leader's run and aim point and
/// keep their place in the formation.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BomberPilot {
    pub phase: BomberPhase,
    /// Number of the current run (0 = first).
    pub run: u32,
    /// Aim point on the ground.
    pub target: Vec3,
    pub bombs_left: u32,
    pub drop_cooldown: f32,
    /// Seconds left in the egress.
    pub phase_time: f32,
    /// Altitude the bomber holds (m).
    pub altitude: f32,
}

/// Aim point of `flight`'s run `run`: spread over the centre so that
/// successive runs hit different blocks (deterministic, golden-angle
/// spiral).
pub fn target_point(flight: u32, run: u32) -> Vec3 {
    let n = (flight * 5 + run * 3 + 1) as f32;
    let angle = n * 2.399_963;
    let radius = TARGET_SPREAD * ((n * 0.618_034).fract()).sqrt();
    Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
}

/// Everything a bomber of `kind` in flight `flight` needs, at `transform`.
pub fn bomber_bundle(
    kind: EnemyKind,
    tuning: EnemyTuning,
    transform: Transform,
    flight: u32,
) -> impl Bundle {
    let stats = kind.stats().tuned(tuning);
    // Front and rear gunners (the zeppelin mans more), each covering a cone
    // around its axis.
    let mounts = kind
        .model()
        .gun_mounts()
        .iter()
        .filter_map(|m| {
            let (facing, arc) = mount_facing(kind, m.name)?;
            Some(TurretMount {
                position: m.position,
                facing,
                arc,
            })
        })
        .collect();
    (
        (
            Name::new(format!("Enemy {}", kind.display_name())),
            Enemy { kind },
            BomberPilot {
                phase: BomberPhase::Approach,
                run: 0,
                target: target_point(flight, 0),
                bombs_left: stick(kind).0,
                drop_cooldown: 0.0,
                phase_time: 0.0,
                altitude: transform.translation.y,
            },
            EnemyFlight {
                speed: stats.cruise_speed,
                bank: 0.0,
            },
            Team::Enemy,
            AircraftType(kind.display_name()),
        ),
        Health::full(stats.health),
        Hitbox {
            radius: stats.hitbox_radius,
            gun_radius: kind.gun_radius(),
        },
        AircraftVelocity(transform.forward() * stats.cruise_speed),
        Turrets::new(stats.gun, TURRET_RANGE, mounts),
        tuning,
        kind.model().body(Layer::Enemy),
        transform,
    )
}

#[allow(clippy::type_complexity)]
pub(super) fn fly_bombers(
    mut commands: Commands,
    cfg: Res<AiConfig>,
    time: Res<Time>,
    obstacles: Res<ObstacleMap>,
    mut bombers: Query<
        (
            &Enemy,
            Option<&EnemyTuning>,
            Option<&FlightGroup>,
            &mut BomberPilot,
            &mut EnemyFlight,
            &mut Transform,
            &mut AircraftVelocity,
        ),
        (Without<EnemyPilot>, Without<GoingDown>),
    >,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    if dt <= 0.0 {
        return;
    }
    let leaders = tactics::flight_leaders(
        bombers
            .iter()
            .map(|(_, _, group, _, _, tf, velocity)| (group, false, tf, velocity.0)),
    );
    // The leader's run, so wingmen follow it.
    let mut runs = bevy::platform::collections::HashMap::<u32, (u32, BomberPilot)>::new();
    for (_, _, group, pilot, _, _, _) in &bombers {
        if let Some(group) = group {
            let entry = runs.entry(group.id).or_insert((group.slot, *pilot));
            if group.slot < entry.0 {
                *entry = (group.slot, *pilot);
            }
        }
    }

    for (enemy, tuning, group, mut pilot, mut flight, mut transform, mut velocity) in &mut bombers {
        let stats = enemy
            .kind
            .stats()
            .tuned(tuning.copied().unwrap_or_default());
        let position = transform.translation;
        let forward = transform.forward().as_vec3();
        let leader = group.and_then(|g| leaders.get(&g.id));
        let is_leader = match (group, leader) {
            (Some(g), Some(l)) => l.0 == g.slot,
            _ => true,
        };

        if !is_leader && let Some((_, lead)) = group.and_then(|g| runs.get(&g.id)) {
            if lead.run != pilot.run {
                pilot.run = lead.run;
                pilot.target = lead.target;
                pilot.bombs_left = stick(enemy.kind).0;
            }
            pilot.phase = lead.phase;
        }

        // Where the flight goes.
        let level_to_target = Vec3::new(
            pilot.target.x - position.x,
            0.0,
            pilot.target.z - position.z,
        );
        let hold_altitude = Vec3::Y * ((pilot.altitude - position.y) / 300.0).clamp(-0.2, 0.2);
        let (direction, speed) = if is_leader {
            match pilot.phase {
                BomberPhase::Approach => {
                    // Over the aim point (or past it): fly on.
                    let passed = level_to_target.length() < 150.0
                        || (pilot.bombs_left == 0 && level_to_target.dot(forward) < 0.0);
                    if passed {
                        pilot.phase = BomberPhase::Egress;
                        pilot.phase_time = EGRESS_TIME;
                    }
                    (
                        (level_to_target.normalize_or(forward) + hold_altitude)
                            .normalize_or(forward),
                        stats.cruise_speed,
                    )
                }
                BomberPhase::Egress => {
                    pilot.phase_time -= dt;
                    if pilot.phase_time <= 0.0 {
                        pilot.phase = BomberPhase::Approach;
                        pilot.run += 1;
                        pilot.target = target_point(group.map_or(0, |g| g.id), pilot.run);
                        pilot.bombs_left = stick(enemy.kind).0;
                    }
                    let level = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
                    (
                        (level + hold_altitude).normalize_or(level),
                        stats.cruise_speed,
                    )
                }
            }
        } else {
            tactics::holding_direction(group, leader, position, stats.cruise_speed)
        };

        // Bombs away once they would land near the aim point.
        pilot.drop_cooldown -= dt;
        if pilot.phase == BomberPhase::Approach
            && pilot.bombs_left > 0
            && pilot.drop_cooldown <= 0.0
        {
            let fall = fall_time(position.y);
            let impact = position + velocity.0 * fall;
            let miss = Vec3::new(impact.x - pilot.target.x, 0.0, impact.z - pilot.target.z);
            if miss.length() <= BOMB_ZONE {
                commands.spawn(bomb_bundle(
                    BombSpec::HEAVY,
                    position - Vec3::Y * 2.0,
                    velocity.0,
                ));
                pilot.bombs_left -= 1;
                pilot.drop_cooldown = stick(enemy.kind).1;
            }
        }

        let desired = keep_in_arena(&cfg, position, direction);
        let desired = avoid_terrain(&cfg, &obstacles, position, velocity.0, forward, desired);
        fly(
            &cfg,
            &stats,
            stats.turn_rate,
            &mut flight,
            &mut transform,
            &mut velocity,
            desired,
            speed,
            dt,
        );
    }
}

/// How a burning wreck goes down: it sinks faster and faster, drifts to
/// one side, slows down, wobbles. About 20 s from 700 m (it sinks with
/// a constant 3 m/s² more).
const WRECK_SINK_ACCEL: f32 = 3.0;
const WRECK_DRIFT: f32 = 12.0;
const WRECK_SLOWDOWN: f32 = 0.06;
/// Seconds between two fire bursts on the hull.
const WRECK_FIRE_INTERVAL: f32 = 0.4;

/// Flies destroyed aircraft that go down slowly ([`GoingDown`]) until
/// they hit something; then they wreck the city below. They no longer
/// fire, bomb or take hits.
#[allow(clippy::type_complexity)]
pub(super) fn fly_wrecks(
    mut commands: Commands,
    time: Res<Time>,
    obstacles: Res<ObstacleMap>,
    mut crashes: MessageReader<crate::physics::EnemyCrashed>,
    mut wrecks: Query<(
        Entity,
        &mut GoingDown,
        &FallsWhenDestroyed,
        &mut Transform,
        &mut AircraftVelocity,
        Option<&HitCapsule>,
        Has<Turrets>,
    )>,
    mut damage: MessageWriter<crate::world::damage::DamageArea>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    let crashed: Vec<Entity> = crashes.read().map(|c| c.enemy).collect();
    for (entity, mut wreck, falls, mut transform, mut velocity, capsule, armed) in &mut wrecks {
        if armed {
            // Gunners gone; only the world still collides with it.
            commands.entity(entity).remove::<Turrets>().insert(
                bevy_rapier3d::prelude::CollisionGroups::new(
                    crate::physics::Layer::Enemy.group(),
                    crate::physics::Layer::World.group(),
                ),
            );
        }
        let t0 = wreck.time;
        wreck.time += dt;
        let t = wreck.time;
        let forward = transform.forward().as_vec3();
        let side = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let level = Vec3::new(velocity.0.x, 0.0, velocity.0.z) * (1.0 - WRECK_SLOWDOWN * dt);
        let drift = side * WRECK_DRIFT * (t * 0.35).sin();
        let sink = WRECK_SINK_ACCEL * t * t * 0.5 - WRECK_SINK_ACCEL * t0 * t0 * 0.5;
        velocity.0 = level;
        transform.translation += (level + drift) * dt - Vec3::Y * sink;
        // Unsteady: rolls and pitches nose down a little more over time.
        let wobble =
            Quat::from_rotation_z(0.25 * (t * 0.9).sin() * dt) * Quat::from_rotation_x(-0.02 * dt);
        transform.rotation = (transform.rotation * wobble).normalize();

        // Fire along the hull.
        if (t / WRECK_FIRE_INTERVAL).floor() != (t0 / WRECK_FIRE_INTERVAL).floor() {
            let along = capsule.map_or(0.0, |c| c.half_length) * (t * 1.7).sin();
            let at = transform.translation + transform.rotation * Vec3::new(0.0, 0.0, along);
            commands.spawn((
                Name::new("Effect"),
                crate::combat::Effect {
                    kind: crate::combat::EffectKind::Explosion,
                    age: 0.0,
                },
                Transform::from_translation(at),
            ));
        }

        // The lowest point of the hull meets the ground or a roof.
        let half_height = capsule.map_or(0.0, |c| c.radius);
        let p = transform.translation;
        let ground = obstacles.height_at(p.x, p.z);
        if crashed.contains(&entity) || p.y - half_height <= ground {
            // The whole hull comes down: blasts along its length, at street
            // level, so towers and low houses below it all count.
            let axis = transform.rotation * Vec3::Z;
            let axis = Vec3::new(axis.x, 0.0, axis.z).normalize_or(Vec3::Z);
            let half_length = capsule.map_or(0.0, |c| c.half_length);
            for offset in [-1.0, 0.0, 1.0] {
                let point = Vec3::new(p.x, 0.0, p.z) + axis * half_length * offset;
                damage.write(crate::world::damage::DamageArea {
                    point,
                    radius: falls.impact_radius,
                    strength: falls.impact_strength,
                });
                commands.spawn((
                    Name::new("Effect"),
                    crate::combat::Effect {
                        kind: crate::combat::EffectKind::BigExplosion,
                        age: 0.0,
                    },
                    Transform::from_translation(point + Vec3::Y * ground.max(0.0)),
                ));
            }
            commands.entity(entity).despawn();
        }
    }
}
