//! Enemy pilots: their own simple arcade flight (not the player's flight
//! model) and a small state machine: pursue the player and shoot, break
//! when the player sits on their tail or a missile comes, extend after a
//! head-on pass. Terrain look-ahead keeps them above roofs and ground.
//!
//! [`AiPlugin`] runs headless; [`EnemyVisualsPlugin`] adds the meshes.

mod obstacles;
#[cfg(test)]
mod tests;
mod visuals;

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    ActiveCollisionTypes, ActiveEvents, Ccd, Collider, CollisionEvent, CollisionGroups, RigidBody,
};

pub use obstacles::ObstacleMap;
pub use visuals::EnemyVisualsPlugin;

use crate::{
    combat::{
        AircraftVelocity, CombatRng, CombatSystems, DestroyCause, Destroyed, Gun, GunSpec, Health,
        Hitbox, Missile, Team, Trigger,
    },
    flight::{FlightSystems, PlayerAircraft},
    physics::Layer,
    world::CityLayout,
};

/// Enemy aircraft types. Later modes add bombers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyKind {
    Spitfire,
}

/// Size, strength and handling of an enemy type. Metres, seconds, radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyStats {
    pub length: f32,
    pub span: f32,
    pub health: f32,
    pub hitbox_radius: f32,
    pub min_speed: f32,
    pub cruise_speed: f32,
    pub max_speed: f32,
    /// Speed change (m/s²).
    pub acceleration: f32,
    /// How fast the nose swings towards where the pilot wants to go (rad/s).
    pub turn_rate: f32,
    pub gun: GunSpec,
}

impl EnemyKind {
    pub fn stats(self) -> EnemyStats {
        match self {
            // Real Spitfire: 9.1 m long, 11.2 m span. Arcade speeds: about
            // a third of the jet's top speed, but a tighter turn than the
            // jet at speed, so the player wins with energy, not by turning.
            EnemyKind::Spitfire => EnemyStats {
                length: 9.1,
                span: 11.2,
                health: 50.0,
                hitbox_radius: 5.5,
                min_speed: 120.0,
                cruise_speed: 150.0,
                max_speed: 180.0,
                acceleration: 15.0,
                turn_rate: 0.75,
                gun: GunSpec::WWII_GUNS,
            },
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            EnemyKind::Spitfire => "Spitfire",
        }
    }
}

/// Tunables of enemy behaviour.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AiConfig {
    /// Height kept above the highest obstacle below and ahead (m).
    pub clearance: f32,
    /// How far ahead the terrain is checked along the flight path (s) ...
    pub lookahead: f32,
    /// ... in steps of this (s); short enough not to skip a tower.
    pub lookahead_step: f32,
    /// Steepest climb and dive the AI flies (rad).
    pub max_climb: f32,
    pub max_dive: f32,
    /// Horizontal distance from the map centre after which pilots turn back (m).
    pub arena_radius: f32,
    /// Pilots do not climb above this (m).
    pub ceiling: f32,

    /// Gun range and aiming tolerance (m, rad).
    pub fire_range: f32,
    pub fire_cone: f32,
    /// Length of one burst and the pause range after it (s).
    pub burst: f32,
    pub burst_pause: (f32, f32),

    /// The player counts as "on my tail" within this distance (m) ...
    pub tail_range: f32,
    /// ... inside this cone behind the enemy (rad) ...
    pub tail_cone: f32,
    /// ... with the player's nose pointing at the enemy within this angle (rad).
    pub player_aim_cone: f32,
    /// A missile homing on the enemy is noticed within this distance (m).
    pub missile_warning: f32,
    /// How long a break turn lasts (s, random in range).
    pub break_time: (f32, f32),
    /// Pause after a break before the next one (s).
    pub break_cooldown: f32,
    /// Head-on pass closer than this (m) makes the pilot extend ...
    pub overshoot_range: f32,
    /// ... for this long (s) before turning back in.
    pub extend_time: f32,

    /// Visual bank angle at full turn rate (rad) and how fast it follows (1/s).
    pub max_bank: f32,
    pub bank_response: f32,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            clearance: 100.0,
            lookahead: 4.0,
            lookahead_step: 0.25,
            max_climb: 50_f32.to_radians(),
            max_dive: 40_f32.to_radians(),
            arena_radius: 6_000.0,
            ceiling: 2_500.0,

            fire_range: 650.0,
            fire_cone: 5_f32.to_radians(),
            burst: 0.6,
            burst_pause: (1.2, 2.8),

            tail_range: 900.0,
            tail_cone: 40_f32.to_radians(),
            player_aim_cone: 12_f32.to_radians(),
            missile_warning: 1_800.0,
            break_time: (1.8, 3.2),
            break_cooldown: 2.0,
            overshoot_range: 150.0,
            extend_time: 2.5,

            max_bank: 70_f32.to_radians(),
            bank_response: 3.0,
        }
    }
}

/// An enemy aircraft.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enemy {
    pub kind: EnemyKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PilotMode {
    /// Chase the player's lead point and shoot when in range.
    #[default]
    Pursue,
    /// Hard turn to one side (`side` ±1) with a climb or dive (`vertical`).
    Break { side: f32, vertical: f32 },
    /// Fly straight on after a head-on pass, then come back.
    Extend { direction: Vec3 },
}

/// Decision state of an enemy pilot.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct EnemyPilot {
    pub mode: PilotMode,
    /// Time left in `Break` or `Extend` (s).
    pub mode_time: f32,
    /// Time until the next break is allowed (s).
    pub break_cooldown: f32,
    /// Time left in the current burst (s).
    pub burst_left: f32,
    /// Time until the next burst may start (s).
    pub burst_pause: f32,
}

/// Motion state of an enemy besides its `Transform` and [`AircraftVelocity`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EnemyFlight {
    pub speed: f32,
    /// Current visual bank angle (rad), positive = left wing down.
    pub bank: f32,
}

/// Everything an enemy of `kind` needs, at `transform` (nose along −Z).
pub fn enemy_bundle(kind: EnemyKind, transform: Transform) -> impl Bundle {
    let stats = kind.stats();
    (
        Name::new(kind.display_name()),
        Enemy { kind },
        EnemyPilot::default(),
        EnemyFlight {
            speed: stats.cruise_speed,
            bank: 0.0,
        },
        Team::Enemy,
        Health::full(stats.health),
        Hitbox {
            radius: stats.hitbox_radius,
        },
        AircraftVelocity(transform.forward() * stats.cruise_speed),
        Gun::new(stats.gun, None),
        Trigger::default(),
        enemy_body(kind),
        transform,
    )
}

/// Kinematic collision body of an enemy, in its local frame (nose −Z), like
/// the physics' `aircraft_body` but in the enemy's own size: the AI moves
/// it, Rapier reports what it touches.
pub fn enemy_body(kind: EnemyKind) -> impl Bundle {
    let shape = match kind {
        // 9.1 m long, 11.2 m span.
        EnemyKind::Spitfire => Collider::compound(vec![
            // Fuselage with spinner.
            (Vec3::ZERO, Quat::IDENTITY, Collider::capsule_z(3.75, 0.8)),
            // Wing.
            (
                Vec3::new(0.0, -0.35, -0.9),
                Quat::IDENTITY,
                Collider::cuboid(5.6, 0.2, 1.1),
            ),
            // Tailplane and fin.
            (
                Vec3::new(0.0, 0.5, 3.7),
                Quat::IDENTITY,
                Collider::cuboid(1.65, 0.8, 0.55),
            ),
        ]),
    };
    (
        RigidBody::KinematicPositionBased,
        shape,
        Ccd::enabled(),
        Layer::Enemy.groups(),
        ActiveEvents::COLLISION_EVENTS,
        // Kinematic bodies ignore static ones by default; crashes into the
        // world must be reported.
        ActiveCollisionTypes::default() | ActiveCollisionTypes::KINEMATIC_STATIC,
    )
}

/// Enemy thinking and flying, after the player moved and before combat
/// aims and fires.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AiSystems;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiConfig>()
            .init_resource::<ObstacleMap>()
            .configure_sets(
                Update,
                AiSystems
                    .after(FlightSystems::Simulate)
                    .after(CombatSystems::Input)
                    .before(CombatSystems::Aim),
            )
            .add_systems(
                Update,
                (
                    obstacles::rebuild_obstacles.run_if(resource_exists_and_changed::<CityLayout>),
                    fly_enemies,
                )
                    .chain()
                    .in_set(AiSystems),
            )
            .add_systems(Update, enemy_crashes.in_set(CombatSystems::DetectHits));
    }
}

/// What the pilot knows about the player this frame.
#[derive(Clone, Copy)]
struct PlayerView {
    position: Vec3,
    velocity: Vec3,
    forward: Vec3,
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn fly_enemies(
    cfg: Res<AiConfig>,
    time: Res<Time>,
    obstacles: Res<ObstacleMap>,
    mut rng: ResMut<CombatRng>,
    player: Query<
        (&Transform, Option<&AircraftVelocity>, Option<&Health>),
        (With<PlayerAircraft>, Without<Enemy>),
    >,
    missiles: Query<(&Missile, &Transform), Without<Enemy>>,
    mut enemies: Query<(
        Entity,
        &Enemy,
        &mut EnemyPilot,
        &mut EnemyFlight,
        &mut Transform,
        &mut AircraftVelocity,
        &mut Trigger,
    )>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.1);
    if dt <= 0.0 {
        return;
    }
    let player = player
        .single()
        .ok()
        .filter(|(_, _, health)| !health.is_some_and(|h| h.is_dead()))
        .map(|(tf, velocity, _)| PlayerView {
            position: tf.translation,
            velocity: velocity.map_or(Vec3::ZERO, |v| v.0),
            forward: tf.forward().as_vec3(),
        });

    for (entity, enemy, mut pilot, mut flight, mut transform, mut velocity, mut trigger) in
        &mut enemies
    {
        let stats = enemy.kind.stats();
        let position = transform.translation;
        let forward = transform.forward().as_vec3();
        let incoming = missiles
            .iter()
            .filter(|(missile, _)| missile.target == Some(entity))
            .map(|(_, tf)| tf.translation)
            .find(|m| m.distance(position) < cfg.missile_warning);

        let decision = think(
            &cfg, &stats, &mut pilot, &mut rng, position, forward, player, incoming, dt,
        );
        trigger.gun = decision.fire;

        let desired = keep_in_arena(&cfg, position, decision.direction);
        let desired = avoid_terrain(&cfg, &obstacles, position, velocity.0, forward, desired);

        // Turn the nose towards the wish, at most `turn_rate`.
        let new_forward = forward.rotate_towards(desired, stats.turn_rate * dt);
        let yaw_rate = heading_change(forward, new_forward) / dt;
        let bank_target = (yaw_rate / stats.turn_rate).clamp(-1.0, 1.0) * cfg.max_bank;
        flight.bank += (bank_target - flight.bank) * (1.0 - (-cfg.bank_response * dt).exp());

        let speed_change = stats.acceleration * dt;
        flight.speed += (decision.speed.clamp(stats.min_speed, stats.max_speed) - flight.speed)
            .clamp(-speed_change, speed_change);

        velocity.0 = new_forward * flight.speed;
        transform.translation += velocity.0 * dt;
        transform.rotation = Transform::IDENTITY
            .looking_to(new_forward, Vec3::Y)
            .rotation
            * Quat::from_rotation_z(flight.bank);
    }
}

/// An enemy touched the world (ground, water, building, bridge): crashed.
/// Counts as a kill for the player.
fn enemy_crashes(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionEvent>,
    enemies: Query<&Transform, With<Enemy>>,
    groups: Query<&CollisionGroups>,
    mut destroyed: MessageWriter<Destroyed>,
) {
    let mut crashed = Vec::new();
    for event in collisions.read() {
        let CollisionEvent::Started(a, b, _) = *event else {
            continue;
        };
        for (enemy, obstacle) in [(a, b), (b, a)] {
            let is_world = groups
                .get(obstacle)
                .is_ok_and(|g| g.memberships.contains(Layer::World.group()));
            if !is_world || crashed.contains(&enemy) {
                continue;
            }
            let Ok(transform) = enemies.get(enemy) else {
                continue;
            };
            crashed.push(enemy);
            destroyed.write(Destroyed {
                entity: enemy,
                team: Team::Enemy,
                position: transform.translation,
                cause: DestroyCause::Crash,
            });
            commands.entity(enemy).despawn();
        }
    }
}

struct Decision {
    direction: Vec3,
    speed: f32,
    fire: bool,
}

/// One frame of the pilot's state machine.
#[allow(clippy::too_many_arguments)]
fn think(
    cfg: &AiConfig,
    stats: &EnemyStats,
    pilot: &mut EnemyPilot,
    rng: &mut CombatRng,
    position: Vec3,
    forward: Vec3,
    player: Option<PlayerView>,
    incoming: Option<Vec3>,
    dt: f32,
) -> Decision {
    pilot.mode_time -= dt;
    pilot.break_cooldown -= dt;
    pilot.burst_pause -= dt;
    pilot.burst_left -= dt;

    let Some(player) = player else {
        // Nobody to fight: cruise level.
        pilot.mode = PilotMode::Pursue;
        return Decision {
            direction: Vec3::new(forward.x, 0.0, forward.z),
            speed: stats.cruise_speed,
            fire: false,
        };
    };
    let to_player = player.position - position;
    let distance = to_player.length();

    match pilot.mode {
        PilotMode::Pursue => {
            let on_my_tail = distance < cfg.tail_range
                && (-forward).angle_between(to_player) < cfg.tail_cone
                && player.forward.angle_between(-to_player) < cfg.player_aim_cone;
            if (on_my_tail || incoming.is_some()) && pilot.break_cooldown <= 0.0 {
                pilot.mode = PilotMode::Break {
                    side: if rng.coin() { 1.0 } else { -1.0 },
                    vertical: rng.range(-0.3, 0.45),
                };
                pilot.mode_time = rng.range(cfg.break_time.0, cfg.break_time.1);
            } else if distance < cfg.overshoot_range && forward.dot(player.forward) < -0.3 {
                let climb = Vec3::new(forward.x, forward.y.max(0.0) + 0.15, forward.z);
                pilot.mode = PilotMode::Extend {
                    direction: climb.normalize_or(forward),
                };
                pilot.mode_time = cfg.extend_time;
            }
        }
        PilotMode::Break { .. } | PilotMode::Extend { .. } => {
            if pilot.mode_time <= 0.0 {
                pilot.mode = PilotMode::Pursue;
                pilot.break_cooldown = cfg.break_cooldown;
            }
        }
    }

    match pilot.mode {
        PilotMode::Pursue => {
            // Aim where the player will be when the rounds arrive.
            let flight_time = (distance / stats.gun.muzzle_speed).min(2.0);
            let aim = player.position + player.velocity * flight_time - position;
            let in_solution =
                distance < cfg.fire_range && forward.angle_between(aim) < cfg.fire_cone;
            let fire = if pilot.burst_left > 0.0 {
                in_solution
            } else if in_solution && pilot.burst_pause <= 0.0 {
                pilot.burst_left = cfg.burst;
                pilot.burst_pause = cfg.burst + rng.range(cfg.burst_pause.0, cfg.burst_pause.1);
                true
            } else {
                false
            };
            // Out of gun range: close in at full speed; in range: match the
            // player's speed to stay on the tail.
            let speed = if distance > cfg.fire_range * 0.7 {
                stats.max_speed
            } else {
                player.velocity.length()
            };
            Decision {
                direction: aim.normalize_or(forward),
                speed,
                fire,
            }
        }
        PilotMode::Break { side, vertical } => {
            // Keep turning to one side: the wish stays 90° off the nose.
            let level = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
            let right = level.cross(Vec3::Y);
            Decision {
                direction: (right * side + level * 0.2 + Vec3::Y * vertical).normalize(),
                speed: stats.max_speed,
                fire: false,
            }
        }
        PilotMode::Extend { direction } => Decision {
            direction,
            speed: stats.max_speed,
            fire: false,
        },
    }
}

/// Turns back towards the centre outside the arena; no climbing above the
/// ceiling.
fn keep_in_arena(cfg: &AiConfig, position: Vec3, desired: Vec3) -> Vec3 {
    let mut desired = desired.normalize_or(Vec3::NEG_Z);
    let out = position.xz().length() - cfg.arena_radius;
    if out > 0.0 {
        let home = Vec3::new(-position.x, 0.0, -position.z).normalize_or_zero();
        desired = (desired + home * (out / 500.0).min(3.0)).normalize_or(home);
    }
    if position.y > cfg.ceiling {
        desired.y = desired.y.min(-0.1);
    }
    desired.normalize_or(Vec3::NEG_Z)
}

/// Limits the wished direction so the path stays `clearance` above the
/// obstacles below and ahead: climbs harder the deeper the path would cut
/// into that margin, and never dives steeper than allowed.
fn avoid_terrain(
    cfg: &AiConfig,
    obstacles: &ObstacleMap,
    position: Vec3,
    velocity: Vec3,
    forward: Vec3,
    desired: Vec3,
) -> Vec3 {
    let steps = (cfg.lookahead / cfg.lookahead_step).ceil() as u32;
    let deficit = (0..=steps)
        .map(|i| {
            let p = position + velocity * (i as f32 * cfg.lookahead_step);
            obstacles.height_at(p.x, p.z) + cfg.clearance - p.y
        })
        .fold(f32::NEG_INFINITY, f32::max);

    let mut min_y = (-cfg.max_dive).sin();
    let max_y = cfg.max_climb.sin();
    if deficit > 0.0 {
        // Up to the steepest climb when a full clearance is missing.
        let urgency = (deficit / cfg.clearance).clamp(0.2, 1.0);
        min_y = (urgency * cfg.max_climb).sin();
    } else if deficit > -cfg.clearance {
        // Within two clearances of the floor: dive no steeper than gently.
        min_y = min_y.max(-0.15);
    }
    let y = desired.y.clamp(min_y, max_y);
    let level = Vec3::new(desired.x, 0.0, desired.z)
        .try_normalize()
        .or_else(|| Vec3::new(forward.x, 0.0, forward.z).try_normalize())
        .unwrap_or(Vec3::NEG_Z);
    level * (1.0 - y * y).max(0.0).sqrt() + Vec3::Y * y
}

/// Signed change of heading around +Y from `from` to `to` (rad), positive
/// = turning left (the flight model's yaw sign).
fn heading_change(from: Vec3, to: Vec3) -> f32 {
    let a = Vec2::new(from.x, from.z);
    let b = Vec2::new(to.x, to.z);
    if a.length_squared() < 1e-6 || b.length_squared() < 1e-6 {
        return 0.0;
    }
    // x right, z back: a left turn rotates the heading towards −x.
    -a.normalize().perp_dot(b.normalize()).asin().clamp(-PI, PI)
}
