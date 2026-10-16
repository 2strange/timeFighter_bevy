//! Pure layout helpers of the HUD, testable without a window.

use bevy::prelude::*;

use crate::{combat::CrashInto, game_state::Outcome};

/// Where an off-screen indicator sits: on the rectangle `margin` inside a
/// viewport of `size`, in screen direction `direction` (x right, y down)
/// from the centre.
pub fn edge_point(size: Vec2, direction: Vec2, margin: f32) -> Vec2 {
    let center = size / 2.0;
    let half = (center - Vec2::splat(margin)).max(Vec2::ZERO);
    let dir = direction.try_normalize().unwrap_or(Vec2::Y);
    let tx = if dir.x.abs() > f32::EPSILON {
        half.x / dir.x.abs()
    } else {
        f32::INFINITY
    };
    let ty = if dir.y.abs() > f32::EPSILON {
        half.y / dir.y.abs()
    } else {
        f32::INFINITY
    };
    center + dir * tx.min(ty)
}

/// `m:ss` for a duration in seconds.
pub fn format_time(seconds: f32) -> String {
    let total = seconds.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

/// The enemy the gun pipper is for: the locked target if it is in front,
/// otherwise the enemy closest to the nose within `cone`. Candidates are
/// `(entity, position, velocity)`.
pub fn gun_target(
    position: Vec3,
    forward: Vec3,
    locked: Option<Entity>,
    cone: f32,
    candidates: impl Iterator<Item = (Entity, Vec3, Vec3)>,
) -> Option<(Entity, Vec3, Vec3)> {
    let mut best: Option<((Entity, Vec3, Vec3), f32)> = None;
    for candidate in candidates {
        let angle = forward.angle_between(candidate.1 - position);
        if angle > cone {
            continue;
        }
        if Some(candidate.0) == locked {
            return Some(candidate);
        }
        if best.is_none_or(|(_, a)| angle < a) {
            best = Some((candidate, angle));
        }
    }
    best.map(|(candidate, _)| candidate)
}

/// Headline of the result screen: what happened, in plain words.
pub fn outcome_title(outcome: Outcome) -> String {
    match outcome {
        Outcome::Victory => "MISSION COMPLETE".into(),
        Outcome::ShotDown => "SHOT DOWN".into(),
        Outcome::CityDestroyed => "THE CITY IS IN RUINS".into(),
        Outcome::Crashed(into) => match into {
            CrashInto::Aircraft(name) => format!("RAMMED BY {}", name.to_uppercase()),
            CrashInto::Building => "CRASHED INTO A BUILDING".into(),
            CrashInto::Bridge => "CRASHED INTO A BRIDGE".into(),
            CrashInto::Water => "CRASHED INTO THE RIVER".into(),
            CrashInto::Ground => "CRASHED INTO THE GROUND".into(),
            CrashInto::Obstacle => "CRASHED".into(),
        },
    }
}
