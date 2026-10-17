//! Smoke trails behind damaged enemies.
//!
//! Every enemy below half health trails smoke puffs, more of them the less
//! health it has left. The puffs come from a fixed pool created once, so a
//! trail allocates nothing per frame: each new puff reuses the oldest one.
//! Puffs live in world space, drift up and grow, then vanish; they stay
//! behind when the aircraft goes down. Headless; [`super::CombatVisualsPlugin`]
//! gives the puffs their mesh.

use bevy::prelude::*;

use super::{Health, Team};

/// Health fraction below which an enemy smokes.
pub const SMOKE_BELOW: f32 = 0.5;
/// Puffs per second just below [`SMOKE_BELOW`] and at zero health.
const RATE_MIN: f32 = 6.0;
const RATE_MAX: f32 = 22.0;
/// Puffs in the pool: enough for about a dozen heavily damaged aircraft.
pub const POOL_SIZE: usize = 256;
/// Seconds a puff lives, and its diameter at birth and at the end (m).
const LIFE: f32 = 1.6;
const SIZE_START: f32 = 2.5;
const SIZE_END: f32 = 9.0;
/// Upward drift (m/s).
const RISE: f32 = 3.0;

/// One smoke puff of the pool; idle while `age >= LIFE`.
#[derive(Component, Debug, Clone, Copy)]
pub struct SmokePuff {
    pub age: f32,
}

impl SmokePuff {
    pub fn is_active(&self) -> bool {
        self.age < LIFE
    }

    /// Diameter (m) at the puff's age: grows, then shrinks away at the end.
    fn size(&self) -> f32 {
        let t = (self.age / LIFE).clamp(0.0, 1.0);
        let grow = SIZE_START + (SIZE_END - SIZE_START) * t.sqrt();
        let fade = ((1.0 - t) / 0.25).min(1.0);
        grow * fade
    }
}

/// Emission state of a smoking aircraft: fractional puffs carried over.
#[derive(Component, Debug, Default)]
pub struct SmokeTrail {
    carry: f32,
}

/// The pool, in spawn order; `next` is the oldest puff.
#[derive(Resource, Debug, Default)]
pub struct SmokePool {
    puffs: Vec<Entity>,
    next: usize,
}

/// Puffs per second for an aircraft at `health` (0 above [`SMOKE_BELOW`]).
pub fn smoke_rate(health: &Health) -> f32 {
    let f = health.fraction();
    if f >= SMOKE_BELOW {
        return 0.0;
    }
    let damage = 1.0 - f / SMOKE_BELOW;
    RATE_MIN + (RATE_MAX - RATE_MIN) * damage
}

pub(super) fn spawn_pool(mut commands: Commands) {
    let puffs = (0..POOL_SIZE)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Smoke puff"),
                    SmokePuff { age: LIFE },
                    Transform::from_scale(Vec3::ZERO),
                    Visibility::Hidden,
                ))
                .id()
        })
        .collect();
    commands.insert_resource(SmokePool { puffs, next: 0 });
}

/// Damaged enemies emit puffs at their tail.
#[allow(clippy::type_complexity)]
pub(super) fn emit_smoke(
    mut commands: Commands,
    time: Res<Time>,
    pool: Option<ResMut<SmokePool>>,
    mut aircraft: Query<(Entity, &Team, &Health, &Transform, Option<&mut SmokeTrail>)>,
    mut puffs: Query<(&mut SmokePuff, &mut Transform, &mut Visibility), Without<Team>>,
) {
    let Some(mut pool) = pool else {
        return;
    };
    let dt = time.delta_secs();
    if pool.puffs.is_empty() || dt <= 0.0 {
        return;
    }
    for (entity, team, health, transform, trail) in &mut aircraft {
        let rate = smoke_rate(health);
        if *team != Team::Enemy || rate == 0.0 {
            continue;
        }
        let Some(mut trail) = trail else {
            commands.entity(entity).insert(SmokeTrail::default());
            continue;
        };
        trail.carry += rate * dt;
        // Spread this frame's puffs along the path just flown, from the tail.
        let count = trail.carry.floor() as u32;
        trail.carry -= count as f32;
        let tail = transform.translation + transform.back() * 6.0;
        for k in 0..count {
            let i = pool.next;
            pool.next = (pool.next + 1) % pool.puffs.len();
            if let Ok((mut puff, mut at, mut visibility)) = puffs.get_mut(pool.puffs[i]) {
                // Older puffs of this frame start a little further back.
                let back = transform.back() * (k as f32 * 2.0);
                *puff = SmokePuff { age: 0.0 };
                at.translation = tail + back;
                at.scale = Vec3::splat(SIZE_START);
                *visibility = Visibility::Inherited;
            }
        }
    }
}

/// Puffs rise, grow and vanish.
pub(super) fn age_smoke(
    time: Res<Time>,
    mut puffs: Query<(&mut SmokePuff, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    for (mut puff, mut transform, mut visibility) in &mut puffs {
        if !puff.is_active() {
            continue;
        }
        puff.age += dt;
        if puff.is_active() {
            transform.translation.y += RISE * dt;
            transform.scale = Vec3::splat(puff.size());
        } else {
            transform.scale = Vec3::ZERO;
            *visibility = Visibility::Hidden;
        }
    }
}
