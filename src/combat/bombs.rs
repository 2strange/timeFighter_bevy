//! Bombs: fall under gravity from where they were dropped and damage the
//! city where they land. Simple ballistic bodies, no physics engine: the
//! city's coarse obstacle heights stop them, so a bomb bursts on a roof.

use bevy::prelude::*;

use super::{Effect, EffectKind, weapons::frame_dt};
use crate::{ai::ObstacleMap, world::damage::DamageArea};

/// Gravity for bombs (m/s²).
pub const GRAVITY: f32 = 9.81;

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
    obstacles: Option<Res<ObstacleMap>>,
    mut bombs: Query<(Entity, &mut Bomb, &mut Transform)>,
    mut damage: MessageWriter<DamageArea>,
    mut impacts: MessageWriter<BombImpact>,
) {
    let dt = frame_dt(&time);
    if dt <= 0.0 {
        return;
    }
    for (entity, mut bomb, mut transform) in &mut bombs {
        bomb.velocity.y -= GRAVITY * dt;
        transform.translation += bomb.velocity * dt;
        transform.rotation = super::weapons::heading(bomb.velocity);
        let p = transform.translation;
        let ground = obstacles.as_ref().map_or(0.0, |o| o.height_at(p.x, p.z));
        if p.y > ground {
            continue;
        }
        let point = Vec3::new(p.x, ground, p.z);
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
