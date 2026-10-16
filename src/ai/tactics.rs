//! Who attacks: only a few enemies go for the player at once ("attack
//! slots"); the others circle in their flights at a holding point and wait
//! for a free slot. Pilots only notice the player ahead of them (they
//! cannot look back), unless they were hit or a missile comes.

use bevy::{platform::collections::HashMap, prelude::*};

use super::{AiConfig, Enemy, EnemyPilot};
use crate::flight::PlayerAircraft;

/// At most this many enemies attack at the same time.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackSlots {
    pub max: u32,
}

impl Default for AttackSlots {
    /// Unlimited: everyone attacks (a round sets the difficulty's value).
    fn default() -> Self {
        Self { max: u32::MAX }
    }
}

/// An enemy's flight: whom it flies with, where in the formation, and
/// where the flight circles while waiting.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FlightGroup {
    pub id: u32,
    /// Place in the formation; the lowest waiting one leads.
    pub slot: u32,
    /// Offset from the leader in the leader's frame (x right, z back).
    pub offset: Vec3,
    /// Centre of the holding circle.
    pub holding: Vec3,
}

impl FlightGroup {
    /// Where a flight entering at `lead` waits: on the same bearing from
    /// the map centre, at `distance`, at the lead's altitude.
    pub fn holding_point(lead: Vec3, distance: f32) -> Vec3 {
        let bearing = Vec3::new(lead.x, 0.0, lead.z).normalize_or(Vec3::NEG_Z);
        bearing * distance + Vec3::Y * lead.y
    }
}

/// Radius of the holding circle (m).
pub const HOLDING_RADIUS: f32 = 700.0;

/// Gives free attack slots to waiting enemies (hit ones first, then the
/// closest to the player) and takes back slots above the limit (farthest
/// first).
pub(super) fn assign_attack_slots(
    slots: Res<AttackSlots>,
    cfg: Res<AiConfig>,
    player: Query<&Transform, With<PlayerAircraft>>,
    mut enemies: Query<(Entity, &Transform, &mut EnemyPilot), With<Enemy>>,
) {
    let player = player.single().ok().map(|t| t.translation);
    let target = player.unwrap_or(Vec3::ZERO);
    let mut attackers: Vec<(Entity, f32)> = Vec::new();
    let mut waiting: Vec<(Entity, bool, f32)> = Vec::new();
    for (entity, transform, pilot) in &enemies {
        let distance = transform.translation.distance(target);
        if pilot.attacking {
            attackers.push((entity, distance));
        } else if pilot.slot_rest <= 0.0 || pilot.provoked > 0.0 {
            // A pilot that just gave up a search rests first, unless hit.
            waiting.push((entity, pilot.provoked > 0.0, distance));
        }
    }
    let max = slots.max as usize;
    if attackers.len() > max {
        attackers.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (entity, _) in attackers.iter().take(attackers.len() - max) {
            if let Ok((_, _, mut pilot)) = enemies.get_mut(*entity) {
                pilot.attacking = false;
            }
        }
    } else if attackers.len() < max {
        // Provoked first, then nearest.
        waiting.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.total_cmp(&b.2)));
        for (entity, _, _) in waiting.iter().take(max - attackers.len()) {
            if let Ok((_, _, mut pilot)) = enemies.get_mut(*entity) {
                // Sent in with the player's position (as if by radio).
                pilot.attacking = true;
                pilot.aware = cfg.awareness;
                pilot.last_seen = player;
                pilot.searching = 0.0;
            }
        }
    }
}

/// Snapshot of each flight's current leader among the waiting enemies:
/// position, rotation and velocity.
pub(super) fn flight_leaders<'a>(
    members: impl Iterator<Item = (Option<&'a FlightGroup>, bool, &'a Transform, Vec3)>,
) -> HashMap<u32, (u32, Vec3, Quat, Vec3)> {
    let mut leaders: HashMap<u32, (u32, Vec3, Quat, Vec3)> = HashMap::new();
    for (group, attacking, transform, velocity) in members {
        let Some(group) = group else {
            continue;
        };
        if attacking {
            continue;
        }
        let candidate = (
            group.slot,
            transform.translation,
            transform.rotation,
            velocity,
        );
        leaders
            .entry(group.id)
            .and_modify(|leader| {
                if group.slot < leader.0 {
                    *leader = candidate;
                }
            })
            .or_insert(candidate);
    }
    leaders
}

/// Where a waiting enemy flies, and how fast: the leader circles the
/// holding point, the others keep their place in the formation.
pub(super) fn holding_direction(
    group: Option<&FlightGroup>,
    leader: Option<&(u32, Vec3, Quat, Vec3)>,
    position: Vec3,
    cruise_speed: f32,
) -> (Vec3, f32) {
    let holding = group.map_or(Vec3::new(0.0, position.y, 0.0), |g| g.holding);
    match (group, leader) {
        (Some(group), Some(&(slot, leader_pos, leader_rot, leader_vel))) if slot != group.slot => {
            // Wingman: aim at the formation place a second ahead.
            let place = leader_pos + leader_rot * group.offset;
            let to_place = place + leader_vel - position;
            let speed =
                leader_vel.length() + (place - position).dot(leader_vel.normalize_or_zero()) * 0.2;
            (
                to_place.normalize_or(leader_vel.normalize_or(Vec3::NEG_Z)),
                speed,
            )
        }
        _ => {
            // Leader: circle the holding point, counter-clockwise from above.
            let radial = Vec3::new(position.x - holding.x, 0.0, position.z - holding.z);
            let distance = radial.length().max(1.0);
            let out = radial / distance;
            let tangent = Vec3::Y.cross(out);
            let correction = -out * ((distance - HOLDING_RADIUS) / HOLDING_RADIUS).clamp(-1.0, 3.0);
            let climb = Vec3::Y * ((holding.y - position.y) / 300.0).clamp(-0.3, 0.3);
            (
                (tangent + correction + climb).normalize_or(tangent),
                cruise_speed,
            )
        }
    }
}

/// Whether the player is where the pilot can see it: ahead within
/// `half_angle` (rad) and `range` (m).
pub fn sees(forward: Vec3, to_player: Vec3, half_angle: f32, range: f32) -> bool {
    to_player.length() <= range && forward.angle_between(to_player) <= half_angle
}
