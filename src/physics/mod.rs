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

/// Highest speed of any body (m/s), well above the fastest projectile.
pub const MAX_SPEED: f32 = 2_000.0;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        let mut init = RapierContextInitialization::default();
        if let RapierContextInitialization::InitializeDefaultRapierContext {
            rapier_configuration,
            integration_parameters,
            ..
        } = &mut init
        {
            rapier_configuration.gravity = Vec3::ZERO;
            // Rapier caps speeds at 400 m/s by default; bullets fly faster.
            integration_parameters.normalized_max_linear_velocity = MAX_SPEED;
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

/// An aircraft's bounding box in its local frame (nose −Z, metres), from
/// which its collider is built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AircraftSize {
    pub min: Vec3,
    pub max: Vec3,
}

impl AircraftSize {
    /// A 19 m jet with 13.9 m span (the F-14 and the built-in jet).
    pub const JET: AircraftSize = AircraftSize {
        min: Vec3::new(-6.97, -1.54, -9.55),
        max: Vec3::new(6.97, 2.99, 9.55),
    };

    pub fn length(&self) -> f32 {
        self.max.z - self.min.z
    }

    pub fn span(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Fuselage capsule over the full length, a wing box over the full
    /// span, and a fin box up to the top of the tail. Proportions of the
    /// boxes are generic; the outer dimensions match the model, so hits
    /// at the wing tips, nose and tail count.
    pub fn collider(&self) -> Collider {
        let length = self.length();
        let span = self.span();
        let centre_z = (self.min.z + self.max.z) / 2.0;
        let radius = (0.065 * length).min(self.max.y.max(0.1));
        let mut parts = vec![
            (
                Vec3::new(0.0, 0.0, centre_z),
                Quat::IDENTITY,
                Collider::capsule_z(length / 2.0 - radius, radius),
            ),
            (
                Vec3::new(0.0, -0.2 * radius, centre_z + 0.05 * length),
                Quat::IDENTITY,
                Collider::cuboid(span / 2.0, (0.2 * radius).max(0.1), 0.11 * length),
            ),
        ];
        let fin_height = self.max.y - radius;
        if fin_height > 0.1 {
            parts.push((
                Vec3::new(0.0, radius + fin_height / 2.0, self.max.z - 0.12 * length),
                Quat::IDENTITY,
                Collider::cuboid(0.06 * span, fin_height / 2.0, 0.07 * length),
            ));
        }
        Collider::compound(parts)
    }
}

/// Collider and body for a jet-sized aircraft; see [`aircraft_body_sized`].
pub fn aircraft_body(layer: Layer) -> impl Bundle {
    aircraft_body_sized(layer, AircraftSize::JET)
}

/// Collider and body for an aircraft of `size`.
///
/// Aircraft are kinematic: the flight model moves them, Rapier only checks
/// what they touch, with CCD so a fast jet does not pass through a wall
/// between two frames.
pub fn aircraft_body_sized(layer: Layer, size: AircraftSize) -> impl Bundle {
    (
        RigidBody::KinematicPositionBased,
        size.collider(),
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
