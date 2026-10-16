//! Aircraft crashes: into the world, and into each other.
//!
//! Two sources, merged into one report per aircraft:
//! - Rapier contacts (aircraft touching the world or another aircraft),
//! - a swept shape cast per aircraft and frame from its previous to its
//!   current pose against the world. Rapier's CCD only sweeps dynamic
//!   bodies; kinematic aircraft jump up to 50 m per step (500 m/s at the
//!   0.1 s step limit) and would pass through a bridge deck between frames.
//!
//! An aircraft crashes once. Its next crash is reported only after it has
//! been teleported (respawn) or [`CrashSweep::reset`] was called.

use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    platform::collections::HashMap,
    prelude::*,
};
use bevy_rapier3d::prelude::*;

use super::{Layer, MAX_SPEED};
use crate::{MAX_STEP, flight::PlayerAircraft};

/// The player's aircraft touched a world collider (ground, water,
/// building, bridge) or an enemy aircraft.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerCrashed {
    pub player: Entity,
    /// The collider it hit.
    pub obstacle: Entity,
    /// Where it hit (m, world space).
    pub point: Vec3,
}

/// An enemy aircraft touched a world collider or the player's aircraft.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct EnemyCrashed {
    pub enemy: Entity,
    pub obstacle: Entity,
    pub point: Vec3,
}

/// Crash tracking of an aircraft; part of the aircraft body bundle.
#[derive(Component, Debug, Default, Clone)]
#[component(on_insert = remember_pose)]
pub struct CrashSweep {
    /// Pose at the end of the previous frame.
    last: Option<(Vec3, Quat)>,
    /// Already reported a crash.
    crashed: bool,
}

impl CrashSweep {
    /// Forget the previous pose and the crash, e.g. after placing the
    /// aircraft somewhere new.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Like [`CrashSweep::reset`], with the sweep starting at `transform`.
    pub fn reset_at(&mut self, transform: &Transform) {
        *self = Self {
            last: Some((transform.translation, transform.rotation)),
            crashed: false,
        };
    }

    pub fn has_crashed(&self) -> bool {
        self.crashed
    }
}

/// Starts the sweep at the pose the aircraft is spawned with, so even its
/// very first move is checked.
fn remember_pose(mut world: DeferredWorld, context: HookContext) {
    let Some(transform) = world.get::<Transform>(context.entity).copied() else {
        return;
    };
    if let Some(mut sweep) = world.get_mut::<CrashSweep>(context.entity) {
        sweep.last = Some((transform.translation, transform.rotation));
    }
}

/// An aircraft's crash-relevant parts.
type AircraftParts<'a> = (
    Entity,
    &'a Transform,
    &'a Collider,
    &'a CollisionGroups,
    &'a mut CrashSweep,
    Has<PlayerAircraft>,
);

/// Moves longer than this in one frame are teleports (respawn), not flight.
const TELEPORT_DISTANCE: f32 = MAX_SPEED * MAX_STEP;

pub(super) fn detect_crashes(
    rapier: ReadRapierContext,
    mut collisions: MessageReader<CollisionEvent>,
    mut aircraft: Query<AircraftParts>,
    layers: Query<&CollisionGroups>,
    mut player_crashes: MessageWriter<PlayerCrashed>,
    mut enemy_crashes: MessageWriter<EnemyCrashed>,
) {
    // First crash per aircraft this frame: (obstacle, point).
    let mut crashes: HashMap<Entity, (Entity, Vec3)> = HashMap::new();

    // Swept casts against the world.
    let context = rapier.single().ok();
    for (entity, transform, collider, groups, mut sweep, _) in &mut aircraft {
        let now = (transform.translation, transform.rotation);
        let Some((from, rotation)) = sweep.last.replace(now) else {
            continue;
        };
        let motion = now.0 - from;
        if motion.length() > TELEPORT_DISTANCE {
            sweep.crashed = false;
            continue;
        }
        let (Some(context), false) = (&context, sweep.crashed) else {
            continue;
        };
        if motion == Vec3::ZERO {
            continue;
        }
        let filter = QueryFilter::new()
            .groups(CollisionGroups::new(
                groups.memberships,
                Layer::World.group(),
            ))
            .exclude_rigid_body(entity);
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            // Starting inside something we are leaving is not a new crash.
            stop_at_penetration: false,
            ..default()
        };
        if let Some((obstacle, hit)) =
            context.cast_shape(from, rotation, motion, collider, options, filter)
        {
            let point = hit
                .details
                .map_or(from + motion * hit.time_of_impact, |d| d.witness1);
            crashes.entry(entity).or_insert((obstacle, point));
        }
    }

    // Rapier contacts: aircraft with the world or with each other.
    for event in collisions.read() {
        let CollisionEvent::Started(a, b, _) = *event else {
            continue;
        };
        for (crashed, obstacle) in [(a, b), (b, a)] {
            let Ok((_, transform, _, _, _, _)) = aircraft.get(crashed) else {
                continue;
            };
            // Projectiles touching an aircraft are hits, not crashes.
            let solid = aircraft.contains(obstacle)
                || layers
                    .get(obstacle)
                    .is_ok_and(|g| g.memberships.contains(Layer::World.group()));
            if !solid {
                continue;
            }
            crashes
                .entry(crashed)
                .or_insert((obstacle, transform.translation));
        }
    }

    for (entity, (obstacle, point)) in crashes {
        let Ok((_, _, _, _, mut sweep, is_player)) = aircraft.get_mut(entity) else {
            continue;
        };
        if sweep.crashed {
            continue;
        }
        sweep.crashed = true;
        if is_player {
            player_crashes.write(PlayerCrashed {
                player: entity,
                obstacle,
                point,
            });
        } else {
            enemy_crashes.write(EnemyCrashed {
                enemy: entity,
                obstacle,
                point,
            });
        }
    }
}
