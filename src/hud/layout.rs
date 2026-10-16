//! Pure layout helpers of the HUD, testable without a window.

use bevy::prelude::*;

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
