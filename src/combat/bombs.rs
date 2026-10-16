//! Bombs: fall under gravity from where they were dropped and damage the
//! city where they land. Simple ballistic bodies, not physics bodies; each
//! step is ray-cast against the world's colliders, so a bomb bursts on a
//! roof, or on the ruin that is left of it.

use bevy::prelude::*;

use super::{Effect, EffectKind, weapons::frame_dt};
use bevy_rapier3d::prelude::{CollisionGroups, QueryFilter, ReadRapierContext};

use crate::{ai::ObstacleMap, physics::Layer, world::damage::DamageArea};

/// Gravity for bombs (m/s²).
pub const GRAVITY: f32 = 9.81;

/// Below this height (m) a bomb bursts even without a collider (the
/// river's water lies at about −25 m).
const LOWEST_GROUND: f32 = -40.0;

/// What one bomb does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BombSpec {
    /// Blast radius (m).
    pub radius: f32,
    /// Damage to each building in the radius (1.0 destroys).
    pub strength: f32,
}

impl BombSpec {
    /// A WWII heavy bomber's bomb in arcade terms: two near hits take
    /// down a building.
    pub const HEAVY: BombSpec = BombSpec {
        radius: 40.0,
        strength: 0.55,
    };
}

/// A falling bomb.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Bomb {
    pub spec: BombSpec,
    pub velocity: Vec3,
}

/// A bomb hit the ground or a roof at `point`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct BombImpact {
    pub point: Vec3,
}

/// Seconds a bomb dropped at `height` above the ground takes to fall.
pub fn fall_time(height: f32) -> f32 {
    (2.0 * height.max(0.0) / GRAVITY).sqrt()
}

/// A bomb leaving an aircraft at `position` with its `velocity`.
pub fn bomb_bundle(spec: BombSpec, position: Vec3, velocity: Vec3) -> impl Bundle {
    (
        Name::new("Bomb"),
        Bomb { spec, velocity },
        Transform::from_translation(position).with_rotation(super::weapons::heading(velocity)),
    )
}

pub(super) fn fall_bombs(
    mut commands: Commands,
    time: Res<Time>,
    rapier: ReadRapierContext,
    obstacles: Option<Res<ObstacleMap>>,
    mut bombs: Query<(Entity, &mut Bomb, &mut Transform)>,
    mut damage: MessageWriter<DamageArea>,
    mut impacts: MessageWriter<BombImpact>,
) {
    let dt = frame_dt(&time);
    if dt <= 0.0 {
        return;
    }
    let context = rapier.single().ok();
    // Rays as a bomb would see the world: everything on the world layer
    // (buildings and ruins as they are now, bridges, land, water).
    let world = QueryFilter::new().groups(CollisionGroups::new(
        Layer::EnemyProjectile.group(),
        Layer::World.group(),
    ));
    for (entity, mut bomb, mut transform) in &mut bombs {
        let from = transform.translation;
        bomb.velocity.y -= GRAVITY * dt;
        let step = bomb.velocity * dt;
        let to = from + step;
        transform.translation = to;
        transform.rotation = super::weapons::heading(bomb.velocity);
        let hit = match &context {
            // The path of this step against the real colliders; below the
            // lowest ground (the river's water) as a safety net.
            Some(context) => {
                let length = step.length().max(1e-3);
                context
                    .cast_ray(from, step / length, length, true, world)
                    .map(|(_, toi)| from + step / length * toi)
                    .or_else(|| (to.y <= LOWEST_GROUND).then_some(to))
            }
            // Without physics: the coarse obstacle heights.
            None => {
                let ground = obstacles.as_ref().map_or(0.0, |o| o.height_at(to.x, to.z));
                (to.y <= ground).then_some(Vec3::new(to.x, ground, to.z))
            }
        };
        let Some(point) = hit else {
            continue;
        };
        damage.write(DamageArea {
            point,
            radius: bomb.spec.radius,
            strength: bomb.spec.strength,
        });
        impacts.write(BombImpact { point });
        commands.spawn((
            Name::new("Effect"),
            Effect {
                kind: EffectKind::Explosion,
                age: 0.0,
            },
            Transform::from_translation(point),
        ));
        commands.entity(entity).despawn();
    }
}
