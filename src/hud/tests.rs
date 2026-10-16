//! HUD layout helpers and a smoke test of the HUD in the full game.

use bevy::prelude::*;

use super::{TargetMarker, edge_point, format_time};
use crate::{GamePlugin, ai::Enemy, test_support::headless_app};

#[test]
fn edge_indicator_sits_on_the_inset_screen_border() {
    let size = Vec2::new(1_280.0, 720.0);
    let margin = 20.0;
    assert_eq!(edge_point(size, Vec2::X, margin), Vec2::new(1_260.0, 360.0));
    assert_eq!(
        edge_point(size, Vec2::NEG_Y, margin),
        Vec2::new(640.0, 20.0)
    );
    // Diagonal: hits the nearer border (top/bottom on a wide screen).
    let p = edge_point(size, Vec2::new(1.0, 1.0), margin);
    assert!((p.y - 700.0).abs() < 1e-3 && p.x < 1_260.0, "{p:?}");
    // No direction (target exactly behind): bottom centre.
    assert_eq!(
        edge_point(size, Vec2::ZERO, margin),
        Vec2::new(640.0, 700.0)
    );
}

#[test]
fn time_is_shown_as_minutes_and_seconds() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(65.9), "1:05");
    assert_eq!(format_time(600.0), "10:00");
}

#[test]
fn full_game_runs_headless_with_one_marker_per_enemy() {
    let mut app = headless_app(GamePlugin);
    for _ in 0..5 {
        app.update();
    }
    let enemies = app
        .world_mut()
        .query_filtered::<Entity, With<Enemy>>()
        .iter(app.world())
        .count();
    let markers = app
        .world_mut()
        .query::<&TargetMarker>()
        .iter(app.world())
        .count();
    assert!(enemies > 0, "first wave spawned");
    assert_eq!(markers, enemies);
}
