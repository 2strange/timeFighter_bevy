//! Fast projectiles (bullets, missiles) with continuous collision detection.

use bevy::{platform::collections::HashSet, prelude::*};
use bevy_rapier3d::prelude::*;

use super::Layer;

/// A projectile in flight. Spawn with [`projectile`].
#[derive(Component, Clone, Copy, Debug)]
pub struct Projectile {
    /// `PlayerProjectile` or `EnemyProjectile`.
    pub layer: Layer,
    /// Seconds left before it disappears without hitting anything.
    pub lifetime: f32,
}

/// The projectile has hit something and is frozen. It is despawned in the
/// next frame, so systems reading [`ProjectileHit`] in `Update` can still
/// look at its components (damage, kind, shooter).
#[derive(Component, Debug)]
pub struct ProjectileSpent;

/// A projectile touched a collider of a layer it collides with.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ProjectileHit {
    pub projectile: Entity,
    /// The collider entity that was hit (aircraft, building, ground).
    pub target: Entity,
    /// Where the projectile stopped: the point of impact, up to its radius.
    pub point: Vec3,
}

/// Everything a projectile entity needs: flying at `velocity` (m/s) from
/// `transform`, a sphere of `radius` (m), on `layer`, gone after `lifetime`
/// seconds. Combat adds its own components (damage, kind) next to it.
///
/// Projectiles fly straight; guided missiles steer by changing their
/// `Velocity` each frame.
pub fn projectile(
    transform: Transform,
    velocity: Vec3,
    radius: f32,
    layer: Layer,
    lifetime: f32,
) -> impl Bundle {
    debug_assert!(
        matches!(layer, Layer::PlayerProjectile | Layer::EnemyProjectile),
        "projectiles belong on a projectile layer, got {layer:?}"
    );
    (
        Projectile { layer, lifetime },
        transform,
        RigidBody::Dynamic,
        Collider::ball(radius),
        Velocity::linear(velocity),
        // Sweeps the path between two steps: a 1,000 m/s bullet moves 33 m
        // per frame at 30 fps and would otherwise skip thin targets.
        Ccd::enabled(),
        GravityScale(0.0),
        Sleeping::disabled(),
        layer.groups(),
        ActiveEvents::COLLISION_EVENTS,
    )
}

pub(super) fn report_hits(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionEvent>,
    projectiles: Query<&Transform, (With<Projectile>, Without<ProjectileSpent>)>,
    mut hits: MessageWriter<ProjectileHit>,
) {
    // A projectile can touch several colliders in one step; only the first counts.
    let mut spent = HashSet::new();
    for event in collisions.read() {
        let CollisionEvent::Started(a, b, _) = *event else {
            continue;
        };
        for (projectile, target) in [(a, b), (b, a)] {
            let Ok(transform) = projectiles.get(projectile) else {
                continue;
            };
            if !spent.insert(projectile) {
                continue;
            }
            hits.write(ProjectileHit {
                projectile,
                target,
                point: transform.translation,
            });
            commands.entity(projectile).insert((
                ProjectileSpent,
                RigidBodyDisabled,
                ColliderDisabled,
            ));
        }
    }
}

/// Removes projectiles marked spent in an earlier frame.
pub(super) fn despawn_spent(mut commands: Commands, spent: Query<Entity, With<ProjectileSpent>>) {
    for entity in &spent {
        commands.entity(entity).despawn();
    }
}

/// Removes projectiles whose lifetime ran out.
pub(super) fn expire(
    mut commands: Commands,
    time: Res<Time>,
    mut projectiles: Query<(Entity, &mut Projectile), Without<ProjectileSpent>>,
) {
    for (entity, mut projectile) in &mut projectiles {
        projectile.lifetime -= time.delta_secs();
        if projectile.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
