//! Gun turrets (bomber gunners): each mount covers an arc, leads its
//! target and fires short bursts at the nearest aircraft of the other side
//! in range. Weaker than a fighter's guns: slow, scattered, short-ranged.

use bevy::prelude::*;

use super::{
    AircraftVelocity, CombatRng, GunSpec, Health, ShotFired, Team, Weapon,
    weapons::{frame_dt, gun_lead, round_bundle},
};

/// One gunner position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurretMount {
    /// Position in the aircraft's frame (m, nose −Z).
    pub position: Vec3,
    /// Centre of its field of fire in the aircraft's frame (unit vector).
    pub facing: Vec3,
    /// Half-angle of the field of fire (rad).
    pub arc: f32,
}

/// The gunners of an aircraft.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Turrets {
    pub spec: GunSpec,
    /// Gunners open fire within this distance (m).
    pub range: f32,
    /// Seconds of fire, then seconds of pause.
    pub burst: f32,
    pub pause: f32,
    pub mounts: Vec<TurretMount>,
    /// Per mount: seconds to the next round, seconds left in the burst,
    /// seconds left in the pause.
    state: Vec<(f32, f32, f32)>,
}

impl Turrets {
    pub fn new(spec: GunSpec, range: f32, mounts: Vec<TurretMount>) -> Self {
        let state = vec![(0.0, 0.0, 0.0); mounts.len()];
        Self {
            spec,
            range,
            burst: 1.0,
            pause: 1.6,
            mounts,
            state,
        }
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn fire_turrets(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<CombatRng>,
    mut shots: MessageWriter<ShotFired>,
    mut gunners: Query<(
        Entity,
        &mut Turrets,
        &Team,
        &Transform,
        Option<&AircraftVelocity>,
    )>,
    targets: Query<
        (
            &Team,
            &Transform,
            Option<&AircraftVelocity>,
            Option<&Health>,
        ),
        Without<Turrets>,
    >,
) {
    let dt = frame_dt(&time);
    for (shooter, mut turrets, team, transform, velocity) in &mut gunners {
        let base_velocity = velocity.map_or(Vec3::ZERO, |v| v.0);
        // The nearest enemy aircraft in range.
        let target = targets
            .iter()
            .filter(|(t, _, _, health)| *t != team && !health.is_some_and(|h| h.is_dead()))
            .map(|(_, tf, v, _)| (tf.translation, v.map_or(Vec3::ZERO, |v| v.0)))
            .filter(|(p, _)| p.distance(transform.translation) <= turrets.range)
            .min_by(|a, b| {
                a.0.distance_squared(transform.translation)
                    .total_cmp(&b.0.distance_squared(transform.translation))
            });
        let turrets = turrets.as_mut();
        for (mount, (cooldown, burst, pause)) in turrets.mounts.iter().zip(turrets.state.iter_mut())
        {
            *cooldown = (*cooldown - dt).max(-turrets.spec.interval());
            *pause = (*pause - dt).max(0.0);
            let Some((target_pos, target_vel)) = target else {
                *burst = 0.0;
                continue;
            };
            let muzzle = transform.transform_point(mount.position);
            let Some((direction, _)) = gun_lead(
                target_pos - muzzle,
                base_velocity,
                target_vel,
                turrets.spec.muzzle_speed,
            ) else {
                continue;
            };
            let facing = transform.rotation * mount.facing;
            if facing.angle_between(direction) > mount.arc {
                *burst = 0.0;
                continue;
            }
            // Bursts: fire for `burst` seconds, then wait `pause`.
            if *burst <= 0.0 {
                if *pause > 0.0 {
                    continue;
                }
                *burst = turrets.burst;
            }
            *burst -= dt;
            if *burst <= 0.0 {
                *pause = turrets.pause;
            }
            while *cooldown <= 0.0 {
                *cooldown += turrets.spec.interval();
                let aim = rng.in_cone(direction, turrets.spec.spread);
                commands.spawn(round_bundle(
                    shooter,
                    *team,
                    &turrets.spec,
                    muzzle,
                    base_velocity + aim * turrets.spec.muzzle_speed,
                ));
                shots.write(ShotFired {
                    shooter,
                    team: *team,
                    weapon: Weapon::Gun,
                    position: muzzle,
                });
            }
        }
    }
}
