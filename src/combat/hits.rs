//! From physics contacts to hits, crashes and damage.
//!
//! The physics layer reports what projectiles touch ([`ProjectileHit`]) and
//! when aircraft fly into the world or into each other ([`PlayerCrashed`],
//! [`EnemyCrashed`]). This module
//! turns that into [`Hit`] and [`Destroyed`] messages, adds the missiles'
//! proximity fuses, and applies damage in [`apply_hits`].

use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, ColliderDisabled, RigidBodyDisabled};

use super::{
    AircraftType, CrashInto, DestroyCause, Destroyed, FallsWhenDestroyed, GoingDown, Health, Hit,
    HitCapsule, Hitbox, Munition, Team, Weapon,
};
use crate::{
    flight::PlayerAircraft,
    physics::{EnemyCrashed, Ground, PlayerCrashed, ProjectileHit, ProjectileSpent},
    world::colliders::{CityBridge, CityBuilding},
};

/// What a crash obstacle is, from its components.
pub type ObstacleParts<'a> = (
    Has<CityBuilding>,
    Has<CityBridge>,
    Has<Ground>,
    Option<&'a Collider>,
    Option<&'a AircraftType>,
);

/// Classifies the collider an aircraft crashed into. The river's water is
/// the one ground collider that is a half-space.
pub fn crash_into(obstacles: &Query<ObstacleParts>, obstacle: Entity) -> CrashInto {
    let Ok((building, bridge, ground, collider, aircraft)) = obstacles.get(obstacle) else {
        return CrashInto::Obstacle;
    };
    if let Some(aircraft) = aircraft {
        CrashInto::Aircraft(aircraft.0)
    } else if building {
        CrashInto::Building
    } else if bridge {
        CrashInto::Bridge
    } else if ground && collider.is_some_and(|c| c.as_halfspace().is_some()) {
        CrashInto::Water
    } else if ground {
        CrashInto::Ground
    } else {
        CrashInto::Obstacle
    }
}

/// Point of the segment `a..b` closest to `p`.
pub fn closest_point_on_segment(a: Vec3, b: Vec3, p: Vec3) -> Vec3 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= f32::EPSILON {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0)
}

/// Closest points of the segments `p1..q1` and `p2..q2` (Ericson, Real-Time
/// Collision Detection, 5.1.9).
pub fn closest_points_on_segments(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);
    let eps = 1e-9;
    let (s, t) = if a <= eps && e <= eps {
        (0.0, 0.0)
    } else if a <= eps {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= eps {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s = if denom > eps {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// Swept test against a capsule from `c0` to `c1` of `radius`: returns the
/// point of the path closest to the capsule's core when within reach.
pub fn sweep_hits_capsule(a: Vec3, b: Vec3, c0: Vec3, c1: Vec3, radius: f32) -> Option<Vec3> {
    let (on_path, on_core) = closest_points_on_segments(a, b, c0, c1);
    (on_path.distance_squared(on_core) <= radius * radius).then_some(on_path)
}

/// Swept test: does a point moving from `a` to `b` pass within `radius`
/// of `center`? Returns the closest point on the path.
pub fn sweep_hits_sphere(a: Vec3, b: Vec3, center: Vec3, radius: f32) -> Option<Vec3> {
    let closest = closest_point_on_segment(a, b, center);
    (closest.distance_squared(center) <= radius * radius).then_some(closest)
}

/// Direct hits reported by the physics. Hits on the world (ground,
/// buildings) only end the projectile; hits on aircraft become [`Hit`]s.
pub fn projectile_hits(
    mut contacts: MessageReader<ProjectileHit>,
    munitions: Query<&Munition>,
    aircraft: Query<(), (With<Team>, With<Health>)>,
    mut hits: MessageWriter<Hit>,
) {
    for contact in contacts.read() {
        let Ok(munition) = munitions.get(contact.projectile) else {
            continue;
        };
        if !aircraft.contains(contact.target) {
            continue;
        }
        hits.write(Hit {
            projectile: contact.projectile,
            target: contact.target,
            point: contact.point,
            team: munition.team,
            weapon: munition.weapon,
            damage: munition.damage,
        });
    }
}

/// Hits by nearness: a missile's proximity fuse goes off when its path
/// since the last frame passes within fuse range of an aircraft of the
/// other side; a gun round hits when its path crosses the aircraft's gun
/// sphere (its silhouette). The projectile is then spent like a physics
/// hit, so it cannot hit twice.
#[allow(clippy::type_complexity)]
pub fn proximity_hits(
    mut commands: Commands,
    projectiles: Query<(Entity, &Munition, &Transform), Without<ProjectileSpent>>,
    targets: Query<(
        Entity,
        &Team,
        &Hitbox,
        &Transform,
        Option<&Health>,
        Option<&HitCapsule>,
    )>,
    mut hits: MessageWriter<Hit>,
) {
    for (entity, munition, transform) in &projectiles {
        let from = munition.last_position;
        let to = transform.translation;
        let closest = targets
            .iter()
            .filter(|(_, team, _, _, health, _)| {
                **team != munition.team && !health.is_some_and(|h| h.is_dead())
            })
            .filter_map(|(target, _, hitbox, target_tf, _, capsule)| {
                if let Some(capsule) = capsule {
                    // Long hit zone: along the aircraft's axis.
                    let axis = target_tf.rotation * Vec3::Z * capsule.half_length;
                    let reach = capsule.radius + munition.fuse_radius;
                    let c = target_tf.translation;
                    return sweep_hits_capsule(from, to, c - axis, c + axis, reach)
                        .map(|point| (target, point, from.distance_squared(point)));
                }
                let reach = if munition.fuse_radius > 0.0 {
                    hitbox.radius + munition.fuse_radius
                } else {
                    hitbox.gun_radius
                };
                if reach <= 0.0 {
                    return None;
                }
                sweep_hits_sphere(from, to, target_tf.translation, reach)
                    .map(|point| (target, point, from.distance_squared(point)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((target, point, _)) = closest {
            hits.write(Hit {
                projectile: entity,
                target,
                point,
                team: munition.team,
                weapon: munition.weapon,
                damage: munition.damage,
            });
            commands
                .entity(entity)
                .insert((ProjectileSpent, RigidBodyDisabled, ColliderDisabled));
        }
    }
}

/// The player flew into the ground, water, a building, a bridge or an
/// enemy (ramming).
pub fn player_crashes(
    mut crashes: MessageReader<PlayerCrashed>,
    obstacles: Query<ObstacleParts>,
    mut player: Query<&mut Health, With<PlayerAircraft>>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    for crash in crashes.read() {
        let Ok(mut health) = player.get_mut(crash.player) else {
            continue;
        };
        if health.is_dead() {
            continue;
        }
        health.current = 0.0;
        destroyed.write(Destroyed {
            entity: crash.player,
            team: Team::Player,
            position: crash.point,
            cause: DestroyCause::Crash(crash_into(&obstacles, crash.obstacle)),
        });
    }
}

/// An enemy flew into the world or rammed the player. It is destroyed and
/// counts as a kill.
pub fn enemy_crashes(
    mut commands: Commands,
    mut crashes: MessageReader<EnemyCrashed>,
    obstacles: Query<ObstacleParts>,
    mut enemies: Query<(&Team, &mut Health), Without<PlayerAircraft>>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    for crash in crashes.read() {
        let Ok((team, mut health)) = enemies.get_mut(crash.enemy) else {
            continue;
        };
        if health.is_dead() {
            continue;
        }
        health.current = 0.0;
        destroyed.write(Destroyed {
            entity: crash.enemy,
            team: *team,
            position: crash.point,
            cause: DestroyCause::Crash(crash_into(&obstacles, crash.obstacle)),
        });
        commands.entity(crash.enemy).despawn();
    }
}

/// Start of the next frame's fuse sweep.
pub fn remember_positions(mut munitions: Query<(&mut Munition, &Transform)>) {
    for (mut munition, transform) in &mut munitions {
        munition.last_position = transform.translation;
    }
}

/// Applies damage from [`Hit`]s and destroys aircraft at zero health.
/// Destroyed enemies are despawned; the player stays. Spent projectiles are
/// removed by the physics.
#[allow(clippy::type_complexity)]
pub fn apply_hits(
    mut commands: Commands,
    mut hits: MessageReader<Hit>,
    mut targets: Query<(
        &mut Health,
        &Team,
        &Transform,
        Has<PlayerAircraft>,
        Has<FallsWhenDestroyed>,
    )>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    for hit in hits.read() {
        let Ok((mut health, team, transform, is_player, falls)) = targets.get_mut(hit.target)
        else {
            continue;
        };
        if health.is_dead() {
            continue;
        }
        health.current = (health.current - hit.damage).max(0.0);
        if health.is_dead() {
            destroyed.write(Destroyed {
                entity: hit.target,
                team: *team,
                position: transform.translation,
                cause: match hit.weapon {
                    Weapon::Gun => DestroyCause::Gun,
                    Weapon::Missile => DestroyCause::Missile,
                },
            });
            if falls {
                // Shot down, but it goes down slowly and burning; it only
                // stops counting once it hits something (see the AI's wreck
                // flight).
                commands.entity(hit.target).insert(GoingDown::default());
            } else if !is_player {
                commands.entity(hit.target).despawn();
            }
        }
    }
}
