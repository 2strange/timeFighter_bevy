//! HUD layout helpers and a smoke test of the HUD in the full game.

use bevy::prelude::*;

use super::{
    ALERT, HUD, HealthBar, MissileBox, Rung, SoundSwitch, SpeakerStrike, SpeakerWave, TRACKING,
    TargetMarker, edge_point, format_time, gun_target, health_tint, outcome_title, selected_target,
};
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

#[test]
fn pipper_target_prefers_the_lock_then_the_nose() {
    let a = Entity::from_raw_u32(1).unwrap();
    let b = Entity::from_raw_u32(2).unwrap();
    let c = Entity::from_raw_u32(3).unwrap();
    let cone = 30_f32.to_radians();
    let candidates = || {
        [
            (a, Vec3::new(100.0, 0.0, -1_000.0), Vec3::ZERO), // ~6° off
            (b, Vec3::new(10.0, 0.0, -400.0), Vec3::ZERO),    // ~1.4° off
            (c, Vec3::new(0.0, 0.0, 400.0), Vec3::ZERO),      // behind
        ]
        .into_iter()
    };
    let pick =
        |locked| gun_target(Vec3::ZERO, Vec3::NEG_Z, locked, cone, candidates()).map(|t| t.0);
    assert_eq!(pick(None), Some(b), "closest to the nose");
    assert_eq!(pick(Some(a)), Some(a), "the lock wins");
    assert_eq!(pick(Some(c)), Some(b), "a lock behind does not count");
}

#[test]
fn result_titles_name_the_cause() {
    use crate::{combat::CrashInto, game_state::Outcome};
    assert_eq!(outcome_title(Outcome::Victory), "MISSION COMPLETE");
    assert_eq!(outcome_title(Outcome::ShotDown), "SHOT DOWN");
    assert_eq!(
        outcome_title(Outcome::Crashed(CrashInto::Aircraft("Me 309"))),
        "RAMMED BY ME 309"
    );
    assert_eq!(
        outcome_title(Outcome::Crashed(CrashInto::Building)),
        "CRASHED INTO A BUILDING"
    );
    assert_eq!(
        outcome_title(Outcome::Crashed(CrashInto::Water)),
        "CRASHED INTO THE RIVER"
    );
}

#[test]
fn key_labels_read_like_the_keyboard() {
    use crate::combat::{CombatKeys, key_label, key_labels};
    assert_eq!(key_label(KeyCode::ShiftRight), "R-Shift");
    assert_eq!(key_label(KeyCode::KeyR), "R");
    let keys = CombatKeys::default();
    assert_eq!(key_labels(&keys.gun), "Space");
    assert_eq!(key_labels(&keys.next_target), "Tab / R-Alt");
}

#[test]
fn the_ladder_has_a_rung_every_ten_degrees() {
    let mut app = headless_app(GamePlugin);
    app.update();
    let mut pitches: Vec<i32> = app
        .world_mut()
        .query::<&Rung>()
        .iter(app.world())
        .map(|r| r.pitch)
        .collect();
    pitches.sort();
    assert_eq!(pitches, (-90..=90).step_by(10).collect::<Vec<_>>());
}

#[test]
fn the_selected_target_is_the_tracked_or_locked_one() {
    use crate::combat::LockState;
    let target = Entity::from_raw_u32(7).unwrap();
    assert_eq!(selected_target(LockState::Searching), None);
    assert_eq!(
        selected_target(LockState::Tracking {
            target,
            progress: 0.2
        }),
        Some(target)
    );
    assert_eq!(selected_target(LockState::Locked { target }), Some(target));
    // The bar turns amber below a half, red below a quarter.
    assert_eq!(health_tint(1.0), HUD);
    assert_eq!(health_tint(0.4), TRACKING);
    assert_eq!(health_tint(0.1), ALERT);
}

#[test]
fn every_marker_carries_a_health_bar_and_own_missiles_get_a_box() {
    use crate::combat::{CombatConfig, Missile, Munition, Team, Weapon};
    let mut app = headless_app(GamePlugin);
    for _ in 0..5 {
        app.update();
    }
    let markers: Vec<Entity> = app
        .world_mut()
        .query_filtered::<Entity, With<TargetMarker>>()
        .iter(app.world())
        .collect();
    assert!(!markers.is_empty());
    for marker in markers {
        let bars = app
            .world()
            .get::<Children>(marker)
            .unwrap()
            .iter()
            .filter(|c| app.world().get::<HealthBar>(*c).is_some())
            .count();
        assert_eq!(bars, 1);
    }

    let missile = |team| {
        (
            Munition {
                shooter: Entity::PLACEHOLDER,
                team,
                weapon: Weapon::Missile,
                damage: 0.0,
                fuse_radius: 0.0,
                last_position: Vec3::new(0.0, 3_000.0, 0.0),
            },
            Missile {
                spec: CombatConfig::default().missile,
                target: None,
                speed: 0.0,
            },
            Transform::from_xyz(0.0, 3_000.0, 0.0),
        )
    };
    let own = app.world_mut().spawn(missile(Team::Player)).id();
    app.world_mut().spawn(missile(Team::Enemy));
    app.update();
    app.update();
    let boxes = |app: &mut App| {
        app.world_mut()
            .query::<&MissileBox>()
            .iter(app.world())
            .map(|b| b.missile)
            .collect::<Vec<_>>()
    };
    assert_eq!(boxes(&mut app), vec![own], "only the player's missile");
    app.world_mut().despawn(own);
    app.update();
    app.update();
    assert!(boxes(&mut app).is_empty(), "gone with its missile");
}

#[test]
fn the_speaker_top_left_shows_whether_there_is_sound() {
    let mut app = headless_app(GamePlugin);
    app.update();
    let count = |app: &mut App| {
        app.world_mut()
            .query::<&SoundSwitch>()
            .iter(app.world())
            .count()
    };
    assert_eq!(count(&mut app), 1);
    let shown = |app: &mut App| {
        let waves = app
            .world_mut()
            .query_filtered::<&Visibility, With<SpeakerWave>>()
            .iter(app.world())
            .all(|v| *v != Visibility::Hidden);
        let strike = app
            .world_mut()
            .query_filtered::<&Visibility, With<SpeakerStrike>>()
            .iter(app.world())
            .all(|v| *v != Visibility::Hidden);
        (waves, strike)
    };
    assert_eq!(shown(&mut app), (true, false), "on: waves, no line");
    app.world_mut()
        .resource_mut::<crate::audio::SoundSettings>()
        .enabled = false;
    app.update();
    assert_eq!(shown(&mut app), (false, true), "off: struck through");
}
