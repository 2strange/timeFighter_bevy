//! Guns, missile launches, guidance and projectile motion.

use bevy::prelude::*;

use super::{
    CombatKeys, CombatRng, GunSpec, Health, LockOn, MissileSpec, ShotFired, Team, Velocity,
};
use crate::flight::PlayerAircraft;

/// Longest time step integrated at once (s), as in the flight model: a
/// hitch must not teleport projectiles through targets.
const MAX_DT: f32 = 0.1;

pub(super) fn frame_dt(time: &Time) -> f32 {
    time.delta_secs().clamp(0.0, MAX_DT)
}

/// Which weapon a projectile came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weapon {
    Gun,
    Missile,
}

/// What the pilot (player or AI) wants to fire this frame.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trigger {
    /// Held: the gun fires at its rate.
    pub gun: bool,
    /// One launch request; cleared once handled.
    pub missile: bool,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Gun {
    pub spec: GunSpec,
    /// Rounds left; `None` = unlimited (AI gunners).
    pub ammo: Option<u32>,
    /// Seconds until the next round may leave.
    cooldown: f32,
}

impl Gun {
    pub fn new(spec: GunSpec, ammo: Option<u32>) -> Self {
        Self {
            spec,
            ammo,
            cooldown: 0.0,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MissileRack {
    pub spec: MissileSpec,
    pub count: u32,
    pub capacity: u32,
    /// Seconds until the next launch is possible.
    reload: f32,
}

impl MissileRack {
    pub fn new(spec: MissileSpec, count: u32) -> Self {
        Self {
            spec,
            count,
            capacity: count,
            reload: 0.0,
        }
    }
}

/// A round or missile in flight.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Projectile {
    pub shooter: Entity,
    pub team: Team,
    pub weapon: Weapon,
    pub damage: f32,
    /// World velocity (m/s).
    pub velocity: Vec3,
    /// Position at the start of this frame's step; hit detection sweeps
    /// from here to the current position, so fast rounds cannot tunnel.
    pub previous: Vec3,
    pub age: f32,
    pub lifetime: f32,
    /// Extra reach of a proximity fuse (m); 0 for gun rounds.
    pub fuse_radius: f32,
}

/// Guidance state of a missile (on top of its [`Projectile`]).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Missile {
    pub spec: MissileSpec,
    /// Homing target; `None` flies straight on (no lock at launch, or the
    /// seeker lost the target).
    pub target: Option<Entity>,
    pub speed: f32,
}

/// The seeker gives up when the target is further off the missile's nose
/// than this (rad): after a miss, the missile flies on instead of turning
/// around.
const SEEKER_LIMIT: f32 = 100.0 * std::f32::consts::PI / 180.0;

/// Beyond this angle off the nose (rad) the missile turns as hard as it can
/// towards the target instead of using proportional navigation, which is
/// only meant for small corrections.
const PURSUIT_ANGLE: f32 = 50.0 * std::f32::consts::PI / 180.0;

pub(super) fn player_trigger(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    bindings: Res<CombatKeys>,
    mut player: Query<&mut Trigger, With<PlayerAircraft>>,
) {
    if keys.is_none() && mouse.is_none() {
        return;
    }
    let gun = keys
        .as_ref()
        .is_some_and(|k| k.any_pressed(bindings.gun.iter().copied()))
        || mouse
            .as_ref()
            .is_some_and(|m| m.any_pressed(bindings.gun_mouse.iter().copied()));
    let missile = keys
        .as_ref()
        .is_some_and(|k| k.any_just_pressed(bindings.missile.iter().copied()))
        || mouse
            .as_ref()
            .is_some_and(|m| m.any_just_pressed(bindings.missile_mouse.iter().copied()));
    for mut trigger in &mut player {
        trigger.gun = gun;
        trigger.missile |= missile;
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn fire_guns(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<CombatRng>,
    mut shots: MessageWriter<ShotFired>,
    mut shooters: Query<(
        Entity,
        &mut Gun,
        &Trigger,
        &Team,
        &Transform,
        Option<&Velocity>,
    )>,
) {
    let dt = frame_dt(&time);
    for (shooter, mut gun, trigger, team, transform, velocity) in &mut shooters {
        if !trigger.gun {
            gun.cooldown = (gun.cooldown - dt).max(0.0);
            continue;
        }
        gun.cooldown -= dt;
        let forward = transform.forward().as_vec3();
        let base_velocity = velocity.map_or(Vec3::ZERO, |v| v.0);
        // Several rounds per frame at low frame rates; the rate stays the same.
        while gun.cooldown <= 0.0 {
            if let Some(ammo) = gun.ammo.as_mut() {
                if *ammo == 0 {
                    gun.cooldown = 0.0;
                    break;
                }
                *ammo -= 1;
            }
            gun.cooldown += gun.spec.interval();
            let direction = rng.in_cone(forward, gun.spec.spread);
            let position = transform.translation + forward * gun.spec.muzzle_offset;
            let round_velocity = base_velocity + direction * gun.spec.muzzle_speed;
            commands.spawn(projectile_bundle(
                Projectile {
                    shooter,
                    team: *team,
                    weapon: Weapon::Gun,
                    damage: gun.spec.damage,
                    velocity: round_velocity,
                    previous: position,
                    age: 0.0,
                    lifetime: gun.spec.lifetime,
                    fuse_radius: 0.0,
                },
                position,
            ));
            shots.write(ShotFired {
                shooter,
                team: *team,
                weapon: Weapon::Gun,
                position,
            });
        }
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn launch_missiles(
    mut commands: Commands,
    time: Res<Time>,
    mut shots: MessageWriter<ShotFired>,
    mut launchers: Query<(
        Entity,
        &mut MissileRack,
        &mut Trigger,
        &Team,
        &Transform,
        Option<&Velocity>,
        Option<&LockOn>,
    )>,
) {
    let dt = frame_dt(&time);
    for (shooter, mut rack, mut trigger, team, transform, velocity, lock) in &mut launchers {
        rack.reload = (rack.reload - dt).max(0.0);
        if !std::mem::take(&mut trigger.missile) || rack.count == 0 || rack.reload > 0.0 {
            continue;
        }
        rack.count -= 1;
        rack.reload = rack.spec.reload;

        let forward = transform.forward().as_vec3();
        let base_velocity = velocity.map_or(Vec3::ZERO, |v| v.0);
        // Leaves along the nose at the jet's own speed along it plus a kick.
        let speed = base_velocity.dot(forward).max(0.0) + rack.spec.launch_kick;
        let position = transform.transform_point(rack.spec.launch_offset);
        commands.spawn((
            projectile_bundle(
                Projectile {
                    shooter,
                    team: *team,
                    weapon: Weapon::Missile,
                    damage: rack.spec.damage,
                    velocity: forward * speed,
                    previous: position,
                    age: 0.0,
                    lifetime: rack.spec.lifetime,
                    fuse_radius: rack.spec.fuse_radius,
                },
                position,
            ),
            Missile {
                spec: rack.spec,
                target: lock.and_then(LockOn::locked_target),
                speed,
            },
        ));
        shots.write(ShotFired {
            shooter,
            team: *team,
            weapon: Weapon::Missile,
            position,
        });
    }
}

fn projectile_bundle(projectile: Projectile, position: Vec3) -> impl Bundle {
    let transform =
        Transform::from_translation(position).with_rotation(heading(projectile.velocity));
    (projectile, transform)
}

/// Rotation that points −Z along `velocity`.
pub(super) fn heading(velocity: Vec3) -> Quat {
    let Some(dir) = velocity.try_normalize() else {
        return Quat::IDENTITY;
    };
    let up = if dir.y.abs() > 0.999 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Transform::IDENTITY.looking_to(dir, up).rotation
}

/// Proportional navigation light: turn so that the line of sight to the
/// target stops rotating, limited to the missile's lateral acceleration.
#[allow(clippy::type_complexity)]
pub(super) fn guide_missiles(
    time: Res<Time>,
    mut missiles: Query<(&mut Missile, &mut Projectile, &Transform)>,
    targets: Query<(&Transform, Option<&Velocity>, Option<&Health>), Without<Projectile>>,
) {
    let dt = frame_dt(&time);
    for (mut missile, mut projectile, transform) in &mut missiles {
        missile.speed = (missile.speed + missile.spec.boost * dt).min(missile.spec.max_speed);
        let dir = projectile
            .velocity
            .try_normalize()
            .unwrap_or_else(|| transform.forward().as_vec3());

        let mut accel = Vec3::ZERO;
        if let Some(target) = missile.target {
            match targets.get(target) {
                Ok((target_tf, target_vel, health)) if !health.is_some_and(Health::is_dead) => {
                    let to_target = target_tf.translation - transform.translation;
                    let off_nose = dir.angle_between(to_target);
                    if off_nose > SEEKER_LIMIT {
                        missile.target = None;
                    } else {
                        accel = steering(
                            &missile.spec,
                            dir * missile.speed,
                            to_target,
                            target_vel.map_or(Vec3::ZERO, |v| v.0),
                            off_nose,
                        );
                    }
                }
                _ => missile.target = None,
            }
        }
        let new_dir = (dir * missile.speed + accel * dt)
            .try_normalize()
            .unwrap_or(dir);
        projectile.velocity = new_dir * missile.speed;
    }
}

/// Lateral acceleration command for a missile with `velocity` towards a
/// target at `to_target` (relative position) moving with `target_velocity`.
fn steering(
    spec: &MissileSpec,
    velocity: Vec3,
    to_target: Vec3,
    target_velocity: Vec3,
    off_nose: f32,
) -> Vec3 {
    let dir = velocity.normalize_or_zero();
    let los = to_target.normalize_or_zero();
    let accel = if off_nose > PURSUIT_ANGLE {
        // Far off the nose: turn as hard as possible towards it.
        los - dir * los.dot(dir)
    } else {
        // True PN: a = N · Ω × v, with Ω the line-of-sight rotation rate.
        let relative_velocity = target_velocity - velocity;
        let omega = to_target.cross(relative_velocity) / to_target.length_squared().max(1.0);
        spec.nav_gain * omega.cross(velocity)
    };
    // Only sideways: the motor alone sets the speed.
    let lateral = accel - dir * accel.dot(dir);
    if off_nose > PURSUIT_ANGLE {
        lateral.normalize_or_zero() * spec.max_lateral_accel
    } else {
        lateral.clamp_length_max(spec.max_lateral_accel)
    }
}

pub(super) fn move_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform)>,
) {
    let dt = frame_dt(&time);
    for (entity, mut projectile, mut transform) in &mut projectiles {
        projectile.previous = transform.translation;
        transform.translation += projectile.velocity * dt;
        transform.rotation = heading(projectile.velocity);
        projectile.age += dt;
        // Interim: the flat ground stops everything. Buildings come with
        // the physics layer.
        if projectile.age > projectile.lifetime || transform.translation.y <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
