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
    AircraftType, CrashInto, DestroyCause, Destroyed, Health, Hit, Hitbox, Munition, Team, Weapon,
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

/// Proximity fuses: a missile goes off when its path since the last frame
/// passes within fuse range of an aircraft of the other side. It is then
/// spent like a physics hit, so it cannot hit twice.
pub fn missile_fuses(
    mut commands: Commands,
    missiles: Query<(Entity, &Munition, &Transform), Without<ProjectileSpent>>,
    targets: Query<(Entity, &Team, &Hitbox, &Transform, Option<&Health>)>,
    mut hits: MessageWriter<Hit>,
) {
    for (entity, munition, transform) in &missiles {
        if munition.fuse_radius <= 0.0 {
            continue;
        }
        let from = munition.last_position;
        let to = transform.translation;
        let closest = targets
            .iter()
            .filter(|(_, team, _, _, health)| {
                **team != munition.team && !health.is_some_and(|h| h.is_dead())
            })
            .filter_map(|(target, _, hitbox, target_tf, _)| {
                let reach = hitbox.radius + munition.fuse_radius;
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
pub fn apply_hits(
    mut commands: Commands,
    mut hits: MessageReader<Hit>,
    mut targets: Query<(&mut Health, &Team, &Transform, Has<PlayerAircraft>)>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    for hit in hits.read() {
        let Ok((mut health, team, transform, is_player)) = targets.get_mut(hit.target) else {
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
            if !is_player {
                commands.entity(hit.target).despawn();
            }
        }
    }
}
