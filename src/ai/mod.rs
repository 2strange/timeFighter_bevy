//! Enemy pilots: their own simple arcade flight (not the player's flight
//! model) and a small state machine: pursue the player and shoot, break
//! when the player sits on their tail or a missile comes, extend after a
//! head-on pass. Only a few attack at once ([`AttackSlots`]); the rest
//! circle with their flights. Pilots only see ahead. Terrain look-ahead
//! keeps them above roofs and ground.
//!
//! [`AiPlugin`] runs headless; [`EnemyVisualsPlugin`] adds the meshes.

mod bomber;
mod obstacles;
mod tactics;
#[cfg(test)]
mod tests;
mod visuals;

use std::f32::consts::PI;

use bevy::prelude::*;

pub use bomber::{BomberPhase, BomberPilot, bomber_bundle};
pub use obstacles::ObstacleMap;
pub use tactics::{AttackSlots, FlightGroup, HOLDING_RADIUS, sees};
pub use visuals::EnemyVisualsPlugin;

use crate::{
    aircraft::AircraftModel,
    combat::{
        AircraftType, AircraftVelocity, CombatRng, CombatSystems, Gun, GunSpec, Health, Hit,
        Hitbox, Missile, Team, Trigger,
    },
    flight::{FlightSystems, PlayerAircraft},
    physics::Layer,
    world::damage::{BuildingDestroyed, RepairCity},
};

/// Enemy aircraft types. Later modes add bombers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyKind {
    Spitfire,
    Lancaster,
    B17,
    // Second set, registered for trying out; provisional Spitfire/Lancaster
    // stats with the hitbox from the model's span.
    Bf109K4,
    Me309,
    He177,
    Me264,
    Ju390,
    Ju287,
}

/// Strength and handling of an enemy type. Metres, seconds, radians.
/// Dimensions and collider come from its [`AircraftModel`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyStats {
    pub health: f32,
    /// Sphere for missile proximity fuses (m), about half the span.
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

impl EnemyStats {
    /// With the difficulty's factors applied.
    pub fn tuned(self, tuning: EnemyTuning) -> Self {
        let tuning = tuning.sanitized();
        Self {
            min_speed: self.min_speed * tuning.speed,
            cruise_speed: self.cruise_speed * tuning.speed,
            max_speed: self.max_speed * tuning.speed,
            gun: GunSpec {
                spread: self.gun.spread / tuning.accuracy.max(0.01),
                ..self.gun
            },
            ..self
        }
    }
}

impl EnemyKind {
    pub fn stats(self) -> EnemyStats {
        match self {
            // Arcade speeds: about
            // a third of the jet's top speed, but a tighter turn than the
            // jet at speed, so the player wins with energy, not by turning.
            EnemyKind::Spitfire => EnemyStats {
                health: 50.0,
                hitbox_radius: 5.5,
                min_speed: 120.0,
                cruise_speed: 150.0,
                max_speed: 180.0,
                acceleration: 15.0,
                turn_rate: 0.75,
                gun: GunSpec::WWII_GUNS,
            },
            // A heavy bomber: slow, sluggish, tough (about 12 cannon hits
            // or 2 missiles); its gun is the turrets'.
            EnemyKind::Bf109K4 | EnemyKind::Me309 => EnemyStats {
                hitbox_radius: self.model().size().span() * 0.5,
                ..EnemyKind::Spitfire.stats()
            },
            EnemyKind::He177 | EnemyKind::Me264 | EnemyKind::Ju390 | EnemyKind::Ju287 => {
                EnemyStats {
                    hitbox_radius: self.model().size().span() * 0.43,
                    ..EnemyKind::Lancaster.stats()
                }
            }
            EnemyKind::Lancaster | EnemyKind::B17 => EnemyStats {
                health: 200.0,
                hitbox_radius: 13.0,
                min_speed: 95.0,
                cruise_speed: 110.0,
                max_speed: 125.0,
                acceleration: 5.0,
                turn_rate: 0.25,
                gun: GunSpec::TURRET,
            },
        }
    }

    /// Bombers fly bomb runs at the city; fighters fight.
    pub fn is_bomber(self) -> bool {
        matches!(
            self,
            EnemyKind::Lancaster
                | EnemyKind::B17
                | EnemyKind::He177
                | EnemyKind::Me264
                | EnemyKind::Ju390
                | EnemyKind::Ju287
        )
    }

    pub fn display_name(self) -> &'static str {
        self.model().display_name()
    }

    /// Normalised model; it also gives the collider, so both share one
    /// source of dimensions.
    pub fn model(self) -> AircraftModel {
        match self {
            EnemyKind::Spitfire => AircraftModel::Spitfire,
            EnemyKind::Lancaster => AircraftModel::Lancaster,
            EnemyKind::B17 => AircraftModel::B17,
            EnemyKind::Bf109K4 => AircraftModel::Bf109K4,
            EnemyKind::Me309 => AircraftModel::Me309,
            EnemyKind::He177 => AircraftModel::He177,
            EnemyKind::Me264 => AircraftModel::Me264,
            EnemyKind::Ju390 => AircraftModel::Ju390,
            EnemyKind::Ju287 => AircraftModel::Ju287,
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
    /// ... in steps of this (s); short enough not to skip a tower. Clamped
    /// to at least 0.05 s and 100 steps.
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
    /// Pilots see the player within this half-angle around the nose (rad)
    /// and distance (m); they cannot look back.
    pub sight_half_angle: f32,
    pub sight_range: f32,
    /// How long a pilot keeps the player in mind after seeing it (s).
    pub awareness: f32,
    /// How long being hit gives priority for an attack slot (s).
    pub provoked_time: f32,
    /// An attacker that lost the player flies to where it last saw it and
    /// gives up its slot after this long (s) ...
    pub search_time: f32,
    /// ... or once it gets this close to that point without finding it (m),
    /// and may not attack again for this long (s).
    pub search_arrival: f32,
    pub slot_rest: f32,
    /// Collision avoidance looks this far ahead (s).
    pub collision_horizon: f32,
    /// Closest pass (m) enemies keep between each other.
    pub formation_min_pass: f32,
    /// How long one avoidance manoeuvre is held (s).
    pub avoid_time: f32,
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
            sight_half_angle: 70_f32.to_radians(),
            sight_range: 2_500.0,
            awareness: 5.0,
            provoked_time: 8.0,
            search_time: 8.0,
            search_arrival: 300.0,
            slot_rest: 10.0,
            collision_horizon: 3.0,
            formation_min_pass: 30.0,
            avoid_time: 1.2,
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

/// How well a pilot fights. Missions put rookies in early waves and
/// veterans later, so the fight gets harder as it goes on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PilotSkill {
    /// How long a threat (player on the tail, missile coming) must last
    /// before the pilot breaks (s). Gives the player a moment to shoot.
    pub reaction: f32,
    /// Pause after a break before the next one (s).
    pub break_cooldown: f32,
    /// How long a break turn lasts (s, random in range).
    pub break_time: (f32, f32),
    /// Share of the aircraft type's turn rate the pilot uses.
    pub turn_rate: f32,
    /// Collision avoidance: the pilot steers clear when another aircraft
    /// would pass closer than this (m) ...
    pub min_pass: f32,
    /// ... and is already within this distance (m) or less than
    /// [`AiConfig::collision_horizon`] away from the closest approach.
    pub avoid_distance: f32,
}

impl PilotSkill {
    /// Slow to notice a pursuer, breaks less often and less hard.
    pub const ROOKIE: PilotSkill = PilotSkill {
        reaction: 0.8,
        break_cooldown: 4.0,
        break_time: (1.2, 2.2),
        turn_rate: 0.85,
        min_pass: 80.0,
        avoid_distance: 300.0,
    };

    /// Breaks at once and often: the original behaviour.
    pub const VETERAN: PilotSkill = PilotSkill {
        reaction: 0.0,
        break_cooldown: 2.0,
        break_time: (1.8, 3.2),
        turn_rate: 1.0,
        // Passes closer, but still never on purpose into anyone.
        min_pass: 50.0,
        avoid_distance: 250.0,
    };
}

impl Default for PilotSkill {
    fn default() -> Self {
        Self::VETERAN
    }
}

/// Decision state of an enemy pilot.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct EnemyPilot {
    pub skill: PilotSkill,
    /// How long the current threat has lasted (s).
    pub threat_time: f32,
    pub mode: PilotMode,
    /// Time left in `Break` or `Extend` (s).
    pub mode_time: f32,
    /// Time until the next break is allowed (s).
    pub break_cooldown: f32,
    /// Time left in the current burst (s).
    pub burst_left: f32,
    /// Time until the next burst may start (s).
    pub burst_pause: f32,
    /// Collision avoidance under way.
    pub avoiding: Option<Avoidance>,
    /// Holds one of the attack slots: goes for the player. Otherwise it
    /// waits with its flight.
    pub attacking: bool,
    /// Seconds the pilot still knows where the player is (saw it ahead,
    /// was hit, saw a missile). Only an aware pilot breaks away.
    pub aware: f32,
    /// Seconds since being hit count down from here; a provoked pilot gets
    /// the next free attack slot first.
    pub provoked: f32,
    /// Where the pilot last saw the player.
    pub last_seen: Option<Vec3>,
    /// How long an attacker has been looking for the player it lost (s).
    pub searching: f32,
    /// Seconds before a pilot that gave up its slot may attack again.
    pub slot_rest: f32,
}

/// One collision avoidance manoeuvre, held until it is done.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avoidance {
    pub direction: Vec3,
    /// Seconds the manoeuvre is still held.
    pub time_left: f32,
    /// Seconds until the closest approach it avoids.
    pub closest_in: f32,
}

/// Only a contact this much more urgent (closest approach in less than
/// this share of the current one's time) replaces a running avoidance.
/// Otherwise two contacts would make the pilot flip back and forth.
const URGENT_SHARE: f32 = 0.5;

/// Next avoidance state from the current one and this frame's most urgent
/// collision course (`(direction, seconds to closest approach)`), if any.
pub fn update_avoidance(
    current: Option<Avoidance>,
    found: Option<(Vec3, f32)>,
    hold: f32,
    dt: f32,
) -> Option<Avoidance> {
    let running = current
        .map(|a| Avoidance {
            time_left: a.time_left - dt,
            closest_in: (a.closest_in - dt).max(0.0),
            ..a
        })
        .filter(|a| a.time_left > 0.0);
    match (running, found) {
        (Some(a), Some((_, t))) if t >= a.closest_in * URGENT_SHARE => Some(a),
        (_, Some((direction, t))) => Some(Avoidance {
            direction,
            time_left: hold,
            closest_in: t,
        }),
        (running, None) => running,
    }
}

/// Per-round factors on an enemy, from the difficulty.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EnemyTuning {
    /// Factor on all speeds.
    pub speed: f32,
    /// Factor on gun accuracy: the gun's scatter is divided by it.
    pub accuracy: f32,
}

impl EnemyTuning {
    /// Factors that are not positive finite numbers count as 1.
    pub fn sanitized(self) -> Self {
        let sane = |f: f32| if f.is_finite() && f > 0.0 { f } else { 1.0 };
        Self {
            speed: sane(self.speed),
            accuracy: sane(self.accuracy),
        }
    }
}

impl Default for EnemyTuning {
    fn default() -> Self {
        Self {
            speed: 1.0,
            accuracy: 1.0,
        }
    }
}

/// Motion state of an enemy besides its `Transform` and [`AircraftVelocity`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EnemyFlight {
    pub speed: f32,
    /// Current visual bank angle (rad), positive = left wing down.
    pub bank: f32,
}

/// Everything an enemy of `kind` flown with `skill` needs, at `transform`
/// (nose along −Z).
pub fn enemy_bundle(
    kind: EnemyKind,
    skill: PilotSkill,
    tuning: EnemyTuning,
    transform: Transform,
) -> impl Bundle {
    let stats = kind.stats().tuned(tuning);
    (
        Name::new(format!("Enemy {}", kind.display_name())),
        Enemy { kind },
        EnemyPilot { skill, ..default() },
        EnemyFlight {
            speed: stats.cruise_speed,
            bank: 0.0,
        },
        Team::Enemy,
        AircraftType(kind.display_name()),
        Health::full(stats.health),
        Hitbox {
            radius: stats.hitbox_radius,
        },
        AircraftVelocity(transform.forward() * stats.cruise_speed),
        Gun::new(stats.gun, None),
        Trigger::default(),
        tuning,
        kind.model().body(Layer::Enemy),
        transform,
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
            .init_resource::<AttackSlots>()
            // The city's damage plugin registers them too.
            .add_message::<BuildingDestroyed>()
            .add_message::<RepairCity>()
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
                    obstacles::rebuild_obstacles,
                    tactics::assign_attack_slots,
                    (fly_enemies, bomber::fly_bombers),
                )
                    .chain()
                    .in_set(AiSystems),
            );
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
    mut hits: MessageReader<Hit>,
    mut enemies: Query<(
        Entity,
        &Enemy,
        Option<&EnemyTuning>,
        Option<&FlightGroup>,
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

    // Everyone's position and velocity at the start of the frame, for
    // collision avoidance.
    let traffic: Vec<Traffic> = player
        .map(|p| Traffic {
            entity: None,
            position: p.position,
            velocity: p.velocity,
        })
        .into_iter()
        .chain(
            enemies
                .iter()
                .map(|(entity, _, _, _, _, _, tf, velocity, _)| Traffic {
                    entity: Some(entity),
                    position: tf.translation,
                    velocity: velocity.0,
                }),
        )
        .collect();

    let leaders = tactics::flight_leaders(enemies.iter().map(
        |(_, _, _, group, pilot, _, tf, velocity, _)| (group, pilot.attacking, tf, velocity.0),
    ));
    let hit: Vec<Entity> = hits.read().map(|h| h.target).collect();

    for (
        entity,
        enemy,
        tuning,
        group,
        mut pilot,
        mut flight,
        mut transform,
        mut velocity,
        mut trigger,
    ) in &mut enemies
    {
        let stats = enemy
            .kind
            .stats()
            .tuned(tuning.copied().unwrap_or_default());
        let position = transform.translation;
        let forward = transform.forward().as_vec3();
        let incoming = missiles
            .iter()
            .filter(|(missile, _)| missile.target == Some(entity))
            .map(|(_, tf)| tf.translation)
            .find(|m| m.distance(position) < cfg.missile_warning);

        // What the pilot knows: the player ahead, a hit, a missile.
        pilot.aware -= dt;
        pilot.provoked -= dt;
        let seen = player.is_some_and(|p| {
            sees(
                forward,
                p.position - position,
                cfg.sight_half_angle,
                cfg.sight_range,
            )
        });
        if hit.contains(&entity) {
            pilot.provoked = cfg.provoked_time;
        }
        if seen || incoming.is_some() || hit.contains(&entity) {
            pilot.aware = cfg.awareness;
        }
        if seen {
            pilot.last_seen = player.map(|p| p.position);
        }
        pilot.slot_rest -= dt;

        // An attacker only knows where the player is while aware; after
        // that it flies to where it saw the player last, and gives up its
        // slot when the search fails.
        let known = if pilot.aware > 0.0 || !pilot.attacking {
            pilot.searching = 0.0;
            player
        } else {
            pilot.searching += dt;
            let arrived = pilot
                .last_seen
                .is_none_or(|p| p.distance(position) < cfg.search_arrival);
            if pilot.searching > cfg.search_time || arrived {
                pilot.attacking = false;
                pilot.slot_rest = cfg.slot_rest;
                pilot.searching = 0.0;
                pilot.last_seen = None;
                player
            } else {
                pilot.last_seen.map(|p| PlayerView {
                    position: p,
                    velocity: Vec3::ZERO,
                    forward: Vec3::ZERO,
                })
            }
        };

        let mut decision = think(
            &cfg, &stats, &mut pilot, &mut rng, position, forward, known, incoming, dt,
        );
        // Nobody shoots at what it cannot see.
        decision.fire &= pilot.aware > 0.0;
        // Without an attack slot: wait with the flight (still breaking
        // when threatened, see `think`).
        if !pilot.attacking && pilot.mode == PilotMode::Pursue {
            let (direction, speed) = tactics::holding_direction(
                group,
                group.and_then(|g| leaders.get(&g.id)),
                position,
                stats.cruise_speed,
            );
            decision = Decision {
                direction,
                speed,
                fire: false,
            };
        }
        // Nobody rams on purpose: a collision course beats everything but
        // the ground.
        let found = collision_escape(&cfg, &pilot.skill, entity, position, velocity.0, &traffic);
        pilot.avoiding = update_avoidance(pilot.avoiding, found, cfg.avoid_time, dt);
        if let Some(avoidance) = pilot.avoiding {
            decision.direction = avoidance.direction;
            decision.fire = false;
        }
        trigger.gun = decision.fire;

        let desired = keep_in_arena(&cfg, position, decision.direction);
        let desired = avoid_terrain(&cfg, &obstacles, position, velocity.0, forward, desired);
        fly(
            &cfg,
            &stats,
            stats.turn_rate * pilot.skill.turn_rate,
            &mut flight,
            &mut transform,
            &mut velocity,
            desired,
            decision.speed,
            dt,
        );
    }
}

/// Moves an enemy one step: turns the nose towards `desired` at most at
/// `turn_rate`, banks with the turn, changes speed towards `speed`.
#[allow(clippy::too_many_arguments)]
fn fly(
    cfg: &AiConfig,
    stats: &EnemyStats,
    turn_rate: f32,
    flight: &mut EnemyFlight,
    transform: &mut Transform,
    velocity: &mut AircraftVelocity,
    desired: Vec3,
    speed: f32,
    dt: f32,
) {
    let forward = transform.forward().as_vec3();
    let new_forward = forward.rotate_towards(desired, turn_rate * dt);
    let yaw_rate = heading_change(forward, new_forward) / dt;
    let bank_target = (yaw_rate / turn_rate).clamp(-1.0, 1.0) * cfg.max_bank;
    flight.bank += (bank_target - flight.bank) * (1.0 - (-cfg.bank_response * dt).exp());

    let speed_change = stats.acceleration * dt;
    flight.speed += (speed.clamp(stats.min_speed, stats.max_speed) - flight.speed)
        .clamp(-speed_change, speed_change);

    velocity.0 = new_forward * flight.speed;
    transform.translation += velocity.0 * dt;
    transform.rotation = Transform::IDENTITY
        .looking_to(new_forward, Vec3::Y)
        .rotation
        * Quat::from_rotation_z(flight.bank);
}

/// Another aircraft as seen for collision avoidance.
#[derive(Clone, Copy)]
struct Traffic {
    /// `None` for the player.
    entity: Option<Entity>,
    position: Vec3,
    velocity: Vec3,
}

/// If this aircraft is on a collision course, the direction to steer away:
/// for the most urgent other aircraft whose closest approach (assuming
/// both fly straight) comes nearer than the pilot's minimum pass, within
/// the look-ahead time or the avoidance distance.
pub(crate) fn collision_escape_for(
    cfg: &AiConfig,
    skill: &PilotSkill,
    position: Vec3,
    velocity: Vec3,
    others: impl Iterator<Item = (Vec3, Vec3, bool)>,
) -> Option<(Vec3, f32)> {
    let mut most_urgent: Option<(f32, Vec3)> = None;
    for (other_position, other_velocity, is_player) in others {
        let r = other_position - position;
        let v = other_velocity - velocity;
        let closing = r.dot(v) < 0.0;
        if !closing {
            continue;
        }
        let t = (-r.dot(v) / v.length_squared().max(1e-3)).max(0.0);
        let miss = r + v * t;
        let min_pass = if is_player {
            skill.min_pass
        } else {
            cfg.formation_min_pass
        };
        let near = r.length() < skill.avoid_distance || t < cfg.collision_horizon;
        if miss.length() >= min_pass || !near {
            continue;
        }
        if most_urgent.is_none_or(|(urgent, _)| t < urgent) {
            // Away from where the other will be at the closest approach;
            // dead ahead, pull up and to the right.
            let away = (-miss).try_normalize().unwrap_or_else(|| {
                (Vec3::Y + velocity.normalize_or_zero().cross(Vec3::Y)).normalize()
            });
            let ahead = velocity.normalize_or_zero();
            most_urgent = Some((t, (away + ahead * 0.3).normalize_or(away)));
        }
    }
    most_urgent.map(|(t, escape)| (escape, t))
}

fn collision_escape(
    cfg: &AiConfig,
    skill: &PilotSkill,
    me: Entity,
    position: Vec3,
    velocity: Vec3,
    traffic: &[Traffic],
) -> Option<(Vec3, f32)> {
    let others = traffic
        .iter()
        .filter(|t| t.entity != Some(me))
        .map(|t| (t.position, t.velocity, t.entity.is_none()));
    collision_escape_for(cfg, skill, position, velocity, others)
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
            // Only a pilot who knows about the player checks its tail.
            let on_my_tail = pilot.aware > 0.0
                && distance < cfg.tail_range
                && (-forward).angle_between(to_player) < cfg.tail_cone
                && player.forward.angle_between(-to_player) < cfg.player_aim_cone;
            pilot.threat_time = if on_my_tail || incoming.is_some() {
                pilot.threat_time + dt
            } else {
                0.0
            };
            if pilot.threat_time > 0.0
                && pilot.threat_time >= pilot.skill.reaction
                && pilot.break_cooldown <= 0.0
            {
                pilot.mode = PilotMode::Break {
                    side: if rng.coin() { 1.0 } else { -1.0 },
                    vertical: rng.range(-0.3, 0.45),
                };
                pilot.mode_time = rng.range(pilot.skill.break_time.0, pilot.skill.break_time.1);
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
                pilot.break_cooldown = pilot.skill.break_cooldown;
                pilot.threat_time = 0.0;
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

/// Shortest look-ahead step (s) and most steps per check: a zero or tiny
/// configured step must not turn the check into an endless loop.
const MIN_LOOKAHEAD_STEP: f32 = 0.05;
const MAX_LOOKAHEAD_STEPS: u32 = 100;

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
    let step = cfg.lookahead_step.max(MIN_LOOKAHEAD_STEP);
    let steps = ((cfg.lookahead.max(0.0) / step).ceil() as u32).min(MAX_LOOKAHEAD_STEPS);
    let deficit = (0..=steps)
        .map(|i| {
            let p = position + velocity * (i as f32 * step);
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
