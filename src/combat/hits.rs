//! Hit detection (interim) and damage.
//!
//! Detection is the only part that changes when the physics layer arrives:
//! it will translate the physics' projectile hits and crash events into
//! [`Hit`] and [`Destroyed`] messages, and [`detect_projectile_hits`] plus
//! [`detect_player_ground_crash`] go away. [`apply_hits`] stays.

use bevy::prelude::*;

use super::{CombatConfig, DestroyCause, Destroyed, Health, Hit, Hitbox, Projectile, Team, Weapon};
use crate::flight::{FlightConfig, PlayerAircraft};

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

/// Interim detection: each projectile's path this frame against the
/// hitbox spheres of the other side. Missiles reach further by their
/// proximity fuse. A projectile hits at most one aircraft.
pub fn detect_projectile_hits(
    projectiles: Query<(Entity, &Projectile, &Transform)>,
    targets: Query<(Entity, &Team, &Hitbox, &Transform, Option<&Health>)>,
    mut hits: MessageWriter<Hit>,
) {
    for (entity, projectile, transform) in &projectiles {
        let from = projectile.previous;
        let to = transform.translation;
        let hit = targets
            .iter()
            .filter(|(_, team, _, _, health)| {
                **team != projectile.team && !health.is_some_and(|h| h.is_dead())
            })
            .filter_map(|(target, _, hitbox, target_tf, _)| {
                let reach = hitbox.radius + projectile.fuse_radius;
                sweep_hits_sphere(from, to, target_tf.translation, reach)
                    .map(|point| (target, point, from.distance_squared(point)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((target, point, _)) = hit {
            hits.write(Hit {
                projectile: entity,
                target,
                point,
                team: projectile.team,
                weapon: projectile.weapon,
                damage: projectile.damage,
            });
        }
    }
}

/// INTERIM crash detection until the physics colliders: the flight model
/// holds the jet at its minimum altitude above the ground plane, so touching
/// it counts as a crash. Ignores buildings; the river channel below street
/// level is fatal.
pub fn detect_player_ground_crash(
    cfg: Res<CombatConfig>,
    flight: Option<Res<FlightConfig>>,
    mut player: Query<(Entity, &Transform, &mut Health), With<PlayerAircraft>>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    let Some(flight) = flight else {
        return;
    };
    if !cfg.ground_crash {
        return;
    }
    for (entity, transform, mut health) in &mut player {
        if !health.is_dead() && transform.translation.y <= flight.min_altitude + 0.01 {
            health.current = 0.0;
            destroyed.write(Destroyed {
                entity,
                team: Team::Player,
                position: transform.translation,
                cause: DestroyCause::Crash,
            });
        }
    }
}

/// Applies damage from [`Hit`]s, removes the projectiles, destroys aircraft
/// at zero health. Destroyed enemies are despawned; the player stays.
pub fn apply_hits(
    mut commands: Commands,
    mut hits: MessageReader<Hit>,
    mut targets: Query<(&mut Health, &Team, &Transform, Has<PlayerAircraft>)>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    for hit in hits.read() {
        if let Ok(mut projectile) = commands.get_entity(hit.projectile) {
            projectile.try_despawn();
        }
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
