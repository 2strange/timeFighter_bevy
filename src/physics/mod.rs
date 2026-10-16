//! Collision detection with Rapier: aircraft, projectiles and the world.
//!
//! Rapier is used for collision and hit detection only. There is no gravity
//! and no physical response that matters for gameplay: aircraft are
//! kinematic bodies moved by the flight model, projectiles are fast dynamic
//! bodies with continuous collision detection (CCD), the world is static.
//!
//! Gameplay reacts to two messages: [`ProjectileHit`] and [`PlayerCrashed`].
//! Damage, health and what a hit means belong to the combat code.

mod groups;
mod projectile;
#[cfg(test)]
mod tests;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

pub use groups::Layer;
pub use projectile::{Projectile, ProjectileHit, ProjectileSpent, projectile};

use crate::flight::PlayerAircraft;

/// Longest physics step (s), as in the flight model: below 10 fps the game
/// runs in slow motion instead of letting things jump.
const MAX_STEP: f32 = 0.1;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        let mut init = RapierContextInitialization::default();
        if let RapierContextInitialization::InitializeDefaultRapierContext {
            rapier_configuration,
            ..
        } = &mut init
        {
            rapier_configuration.gravity = Vec3::ZERO;
        }

        #[cfg(feature = "physics-debug")]
        app.add_plugins(RapierDebugRenderPlugin::default());

        app.add_plugins(
            RapierPhysicsPlugin::<NoUserData>::default().with_custom_initialization(init),
        )
        // Rapier's default caps a step at 1/60 s, which would slow the
        // whole simulation down below 60 fps.
        .insert_resource(TimestepMode::Variable {
            max_dt: MAX_STEP,
            time_scale: 1.0,
            substeps: 1,
        })
        .add_message::<ProjectileHit>()
        .add_message::<PlayerCrashed>()
        .add_systems(
            PostUpdate,
            (
                projectile::despawn_spent.before(PhysicsSet::SyncBackend),
                (projectile::report_hits, report_crashes).after(PhysicsSet::Writeback),
                projectile::expire.after(PhysicsSet::Writeback),
            ),
        );
    }
}

/// The player's aircraft touched a world collider (ground, water,
/// building, bridge). What follows (explosion, game over) is up to gameplay.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerCrashed {
    pub player: Entity,
    /// The collider it hit.
    pub obstacle: Entity,
}

/// Marks ground colliders (land, water). The world builds them from its
/// layout on [`Layer::World`]; this module only defines the marker.
#[derive(Component, Debug)]
pub struct Ground;

/// Collider and body for an aircraft, in its local frame (nose −Z, metres).
///
/// A compound of fuselage capsule, wing box and fin box sized for a 19 m
/// jet with 13 m span, close to both the built-in jet and the F-14 model.
/// Aircraft are kinematic: the flight model moves them, Rapier only checks
/// what they touch, with CCD so a fast jet does not pass through a wall
/// between two frames.
pub fn aircraft_body(layer: Layer) -> impl Bundle {
    (
        RigidBody::KinematicPositionBased,
        Collider::compound(vec![
            // Fuselage: 19 m long including the rounded ends.
            (
                Vec3::new(0.0, 0.0, 0.0),
                Quat::IDENTITY,
                Collider::capsule_z(8.3, 1.2),
            ),
            // Wings: 13 m span, 4.5 m chord.
            (
                Vec3::new(0.0, -0.2, 1.8),
                Quat::IDENTITY,
                Collider::cuboid(6.5, 0.25, 2.25),
            ),
            // Fins.
            (
                Vec3::new(0.0, 2.2, 6.8),
                Quat::IDENTITY,
                Collider::cuboid(1.45, 1.6, 1.4),
            ),
        ]),
        Ccd::enabled(),
        layer.groups(),
        ActiveEvents::COLLISION_EVENTS,
        // Kinematic bodies ignore static ones by default; we want to know
        // when the jet flies into the ground or a building.
        ActiveCollisionTypes::default() | ActiveCollisionTypes::KINEMATIC_STATIC,
    )
}

fn report_crashes(
    mut collisions: MessageReader<CollisionEvent>,
    players: Query<(), With<PlayerAircraft>>,
    layers: Query<&CollisionGroups>,
    mut crashes: MessageWriter<PlayerCrashed>,
) {
    for event in collisions.read() {
        let CollisionEvent::Started(a, b, _) = *event else {
            continue;
        };
        for (player, obstacle) in [(a, b), (b, a)] {
            let is_world = layers
                .get(obstacle)
                .is_ok_and(|g| g.memberships.contains(Layer::World.group()));
            if players.contains(player) && is_world {
                crashes.write(PlayerCrashed { player, obstacle });
            }
        }
    }
}
