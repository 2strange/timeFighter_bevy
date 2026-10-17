//! The zeppelin as a carrier: it comes alone and, once it has spotted the
//! player or been hit, drops a fighter from under its hull every
//! [`LAUNCH_INTERVAL`] seconds, keeping a few of them in the air. Its
//! fighters circle it while they wait for an attack slot.

use bevy::prelude::*;

use super::{Enemy, EnemyKind, EnemyTuning, FlightGroup, PilotSkill, enemy_bundle};
use crate::{
    combat::{GoingDown, Health, Hit},
    flight::PlayerAircraft,
};

/// Seconds between two launches ...
pub const LAUNCH_INTERVAL: f32 = 20.0;
/// ... and from waking to the first (the crew scrambles).
pub const FIRST_LAUNCH_DELAY: f32 = 3.0;

/// A carrier launches at most this many fighters in all, whatever the
/// level: the most enemies a round can bring stays fixed (for the score).
pub const MAX_LAUNCHES_PER_CARRIER: u32 = 12;

/// The lookouts spot the player within this distance, all round (m).
pub const CARRIER_SPOT_RANGE: f32 = 2_500.0;

/// Fighters drop free this far below the hull's centre line (m): clear of
/// the gondola, with height to pick up speed.
pub const LAUNCH_DROP: f32 = 70.0;

/// Flight ids of launched fighters: the carrier's flight id plus this, so
/// they never share an id with a wave's flight.
const LAUNCHED_FLIGHT_OFFSET: u32 = 1 << 20;

/// Places in the launched fighters' vic behind their leader.
const MAX_FORMATION: u32 = 6;

/// Spacing of the launched fighters' formation (m).
const LAUNCHED_SPACING: f32 = 45.0;

/// What a carrier launches.
#[derive(Clone, Debug, PartialEq)]
pub struct CarrierSpec {
    /// Fighter types in launch order, repeated.
    pub fighters: Vec<EnemyKind>,
    /// At most this many of its fighters alive at once (0: none).
    pub max_alive: u32,
}

/// A zeppelin's hangar: what it launches, whether it is awake yet, and the
/// fighters it has in the air.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Carrier {
    pub spec: CarrierSpec,
    pub skill: PilotSkill,
    pub tuning: EnemyTuning,
    /// The carrier's own flight id; its fighters escort it.
    pub flight: u32,
    /// Spotted the player or was hit: launching.
    pub awake: bool,
    /// Seconds until the next launch may happen.
    pub cooldown: f32,
    pub launched: Vec<Entity>,
    /// Fighters launched so far.
    pub count: u32,
}

impl Carrier {
    pub fn new(spec: CarrierSpec, skill: PilotSkill, tuning: EnemyTuning, flight: u32) -> Self {
        Self {
            spec,
            skill,
            tuning,
            flight,
            awake: false,
            cooldown: 0.0,
            launched: Vec::new(),
            count: 0,
        }
    }
}

/// A carrier dropped a fighter (the round counts it as an enemy).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FighterLaunched {
    pub carrier: Entity,
    pub fighter: Entity,
}

/// Wakes carriers that see the player or take a hit, and launches their
/// fighters on schedule. A carrier going down launches nothing more.
#[allow(clippy::type_complexity)]
pub(super) fn launch_fighters(
    time: Res<Time>,
    mut commands: Commands,
    mut hits: MessageReader<Hit>,
    mut launches: MessageWriter<FighterLaunched>,
    player: Query<&Transform, With<PlayerAircraft>>,
    alive: Query<(), (With<Enemy>, Without<GoingDown>)>,
    mut carriers: Query<(Entity, &mut Carrier, &Transform, &Health), Without<GoingDown>>,
) {
    let dt = time.delta_secs();
    let hit: Vec<Entity> = hits.read().map(|h| h.target).collect();
    let player = player.single().ok().map(|t| t.translation);
    for (entity, mut carrier, transform, health) in &mut carriers {
        if health.is_dead() {
            continue;
        }
        let position = transform.translation;
        if !carrier.awake {
            let spotted = player.is_some_and(|p| p.distance(position) < CARRIER_SPOT_RANGE);
            if !(spotted || hit.contains(&entity)) {
                continue;
            }
            carrier.awake = true;
            carrier.cooldown = FIRST_LAUNCH_DELAY;
        }
        carrier.cooldown -= dt;
        carrier.launched.retain(|f| alive.contains(*f));
        let room = (carrier.launched.len() as u32) < carrier.spec.max_alive
            && carrier.count < MAX_LAUNCHES_PER_CARRIER;
        if carrier.cooldown > 0.0 || !room || carrier.spec.fighters.is_empty() {
            continue;
        }
        let n = carrier.count;
        let kind = carrier.spec.fighters[n as usize % carrier.spec.fighters.len()];
        let drop = Transform::from_translation(position - Vec3::Y * LAUNCH_DROP)
            .with_rotation(transform.rotation);
        let flight = FlightGroup {
            id: carrier.flight + LAUNCHED_FLIGHT_OFFSET,
            slot: n,
            // Every fighter keeps off the leader's spot, whoever leads.
            offset: formation_slot(1 + n % MAX_FORMATION),
            holding: position,
            escort: Some(carrier.flight),
        };
        let fighter = commands
            .spawn((
                enemy_bundle(
                    kind,
                    kind.pilot().unwrap_or(carrier.skill),
                    carrier.tuning,
                    drop,
                ),
                flight,
            ))
            .id();
        carrier.launched.push(fighter);
        carrier.count += 1;
        carrier.cooldown = LAUNCH_INTERVAL;
        launches.write(FighterLaunched {
            carrier: entity,
            fighter,
        });
    }
}

/// Place `index` in a vic behind the leader (x right, z back), as the
/// waves' flights fly.
fn formation_slot(index: u32) -> Vec3 {
    if index == 0 {
        return Vec3::ZERO;
    }
    let rank = index.div_ceil(2) as f32;
    let side = if index % 2 == 1 { -1.0 } else { 1.0 };
    Vec3::new(side * rank, 0.0, rank) * LAUNCHED_SPACING
}
