//! Missile lock-on: keep an enemy inside the cone ahead of the nose for a
//! moment and the seeker locks it.

use bevy::{platform::collections::HashSet, prelude::*};

use crate::flight::PlayerAircraft;

use super::{CombatConfig, CombatKeys, Health, LockSpec, Missile, Team, weapons::frame_dt};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LockState {
    /// No target in the cone.
    #[default]
    Searching,
    /// `target` is in the cone; `progress` runs from 0 to 1 over the lock time.
    Tracking { target: Entity, progress: f32 },
    /// Missiles launched now home on `target`.
    Locked { target: Entity },
}

/// Seeker of an aircraft that carries missiles.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct LockOn {
    pub state: LockState,
}

impl LockOn {
    pub fn locked_target(&self) -> Option<Entity> {
        match self.state {
            LockState::Locked { target } => Some(target),
            _ => None,
        }
    }

    /// The entity being tracked or locked.
    pub fn target(&self) -> Option<Entity> {
        match self.state {
            LockState::Searching => None,
            LockState::Tracking { target, .. } | LockState::Locked { target } => Some(target),
        }
    }
}

/// Whether a target at `to_target` (relative to the seeker) lies within
/// `cone` around `forward` and within `range`.
pub fn in_seeker_cone(forward: Vec3, to_target: Vec3, cone: f32, range: f32) -> bool {
    let distance = to_target.length();
    distance <= range && distance > 0.0 && forward.angle_between(to_target) <= cone
}

/// The best candidate in the cone: smallest angle off the nose, distance
/// as tie breaker.
fn best_candidate<'a>(
    spec: &LockSpec,
    position: Vec3,
    forward: Vec3,
    candidates: impl Iterator<Item = (Entity, &'a Transform)>,
) -> Option<Entity> {
    candidates
        .filter_map(|(entity, transform)| {
            let to_target = transform.translation - position;
            in_seeker_cone(forward, to_target, spec.cone, spec.range).then(|| {
                let score =
                    forward.angle_between(to_target) + to_target.length() / spec.range * 0.05;
                (entity, score)
            })
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(entity, _)| entity)
}

pub(super) fn update_lock(
    cfg: Res<CombatConfig>,
    time: Res<Time>,
    mut seekers: Query<(&mut LockOn, &Team, &Transform)>,
    targets: Query<(Entity, &Team, &Transform, Option<&Health>)>,
    missiles: Query<&Missile>,
) {
    let spec = cfg.lock;
    let dt = frame_dt(&time);
    // Targets a missile is already on its way to.
    let engaged: HashSet<Entity> = missiles.iter().filter_map(|m| m.target).collect();
    for (mut lock, team, transform) in &mut seekers {
        let position = transform.translation;
        let forward = transform.forward().as_vec3();
        let alive_enemy = |entity: Entity| {
            targets
                .get(entity)
                .ok()
                .filter(|(_, t, _, health)| *t != team && !health.is_some_and(|h| h.is_dead()))
                .map(|(_, _, tf, _)| tf.translation - position)
        };
        let holds = |entity: Entity, factor: f32| {
            alive_enemy(entity).is_some_and(|to_target| {
                in_seeker_cone(forward, to_target, spec.cone * factor, spec.range * factor)
            })
        };

        let next = match lock.state {
            LockState::Locked { target } if holds(target, spec.hold_factor) => lock.state,
            LockState::Tracking { target, progress } if holds(target, 1.0) => {
                let progress = progress + dt / spec.time.max(f32::EPSILON);
                if progress >= 1.0 {
                    LockState::Locked { target }
                } else {
                    LockState::Tracking { target, progress }
                }
            }
            _ => {
                let candidates = || {
                    targets.iter().filter_map(|(entity, t, tf, health)| {
                        (t != team && !health.is_some_and(|h| h.is_dead())).then_some((entity, tf))
                    })
                };
                // Prefer targets no missile is chasing yet.
                let fresh = candidates().filter(|(entity, _)| !engaged.contains(entity));
                match best_candidate(&spec, position, forward, fresh)
                    .or_else(|| best_candidate(&spec, position, forward, candidates()))
                {
                    Some(target) => LockState::Tracking {
                        target,
                        progress: 0.0,
                    },
                    None => LockState::Searching,
                }
            }
        };
        lock.set_if_neq(LockOn { state: next });
    }
}

/// The next-target key: switches the seeker to the next enemy in the lock
/// cone, ordered by angle off the nose, and starts tracking it.
pub(super) fn next_target(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    bindings: Res<CombatKeys>,
    cfg: Res<CombatConfig>,
    mut seekers: Query<(&mut LockOn, &Team, &Transform), With<PlayerAircraft>>,
    targets: Query<(Entity, &Team, &Transform, Option<&Health>)>,
) {
    if !keys.is_some_and(|k| k.any_just_pressed(bindings.next_target.iter().copied())) {
        return;
    }
    let spec = cfg.lock;
    for (mut lock, team, transform) in &mut seekers {
        let forward = transform.forward().as_vec3();
        let mut in_cone: Vec<(Entity, f32)> = targets
            .iter()
            .filter(|(_, t, _, health)| *t != team && !health.is_some_and(|h| h.is_dead()))
            .filter_map(|(entity, _, tf, _)| {
                let to_target = tf.translation - transform.translation;
                in_seeker_cone(forward, to_target, spec.cone, spec.range)
                    .then(|| (entity, forward.angle_between(to_target)))
            })
            .collect();
        in_cone.sort_by(|a, b| a.1.total_cmp(&b.1));
        let next = match lock
            .target()
            .and_then(|current| in_cone.iter().position(|(e, _)| *e == current))
        {
            Some(i) => in_cone.get((i + 1) % in_cone.len()),
            None => in_cone.first(),
        };
        if let Some(&(target, _)) = next
            && Some(target) != lock.target()
        {
            lock.state = LockState::Tracking {
                target,
                progress: 0.0,
            };
        }
    }
}
