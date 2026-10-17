//! Short-lived effects at hits and kills: what happens, where, how long.
//! Headless; [`super::CombatVisualsPlugin`] draws them.

use bevy::prelude::*;

use super::{Destroyed, Health, Hit, Munition, Team, Weapon};
use crate::physics::ProjectileHit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    /// An enemy goes down: fireball, smoke, debris.
    Explosion,
    /// The player's jet goes down: the same, bigger.
    BigExplosion,
    /// A gun round strikes: a brief flash.
    Spark,
    /// A missile detonates (hit, proximity fuse or impact on the world):
    /// a small fireball, so every missile visibly does something even when
    /// the target survives.
    MissileBurst,
}

impl EffectKind {
    /// Fireball diameter (m).
    pub fn size(self) -> f32 {
        match self {
            EffectKind::Explosion => 18.0,
            EffectKind::BigExplosion => 28.0,
            EffectKind::Spark => 2.5,
            EffectKind::MissileBurst => 9.0,
        }
    }

    /// Seconds until the effect is gone.
    pub fn lifetime(self) -> f32 {
        match self {
            EffectKind::Explosion => 2.0,
            EffectKind::BigExplosion => 2.5,
            EffectKind::Spark => 0.15,
            EffectKind::MissileBurst => 1.0,
        }
    }
}

/// An effect playing at its `Transform`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    pub kind: EffectKind,
    /// Seconds since it started.
    pub age: f32,
}

impl Effect {
    /// Progress from 0 (start) to 1 (gone).
    pub fn progress(&self) -> f32 {
        (self.age / self.kind.lifetime()).clamp(0.0, 1.0)
    }
}

pub(super) fn spawn(commands: &mut Commands, kind: EffectKind, at: Vec3) {
    commands.spawn((
        Name::new("Effect"),
        Effect { kind, age: 0.0 },
        Transform::from_translation(at),
    ));
}

/// Every enemy kill and crash explodes where it happened; every gun hit
/// flashes; every missile detonation bursts, on an aircraft (contact or
/// proximity fuse) as on the ground or a building. The player's own
/// explosion starts with the wreck, in the frame of the kill (see
/// `wreck.rs`).
pub(super) fn spawn_effects(
    mut commands: Commands,
    mut destroyed: MessageReader<Destroyed>,
    mut hits: MessageReader<Hit>,
    mut contacts: MessageReader<ProjectileHit>,
    munitions: Query<&Munition>,
    aircraft: Query<(), (With<Team>, With<Health>)>,
) {
    for event in destroyed.read().filter(|d| d.team == Team::Enemy) {
        spawn(&mut commands, EffectKind::Explosion, event.position);
    }
    for hit in hits.read() {
        let kind = match hit.weapon {
            Weapon::Missile => EffectKind::MissileBurst,
            Weapon::Gun => EffectKind::Spark,
        };
        spawn(&mut commands, kind, hit.point);
    }
    // Missiles striking the world: aircraft contacts already came as hits.
    for contact in contacts.read() {
        let missile = munitions
            .get(contact.projectile)
            .is_ok_and(|m| m.weapon == Weapon::Missile);
        if missile && !aircraft.contains(contact.target) {
            spawn(&mut commands, EffectKind::MissileBurst, contact.point);
        }
    }
}

pub(super) fn age_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &mut Effect)>,
) {
    for (entity, mut effect) in &mut effects {
        effect.age += time.delta_secs();
        if effect.age >= effect.kind.lifetime() {
            commands.entity(entity).despawn();
        }
    }
}
