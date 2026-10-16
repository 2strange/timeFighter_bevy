//! Headless tests of the flight model through the ECS (no window, no renderer).

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy};

use super::{FlightConfig, FlightInput, FlightPlugin, FlightState};

/// App with only `MinimalPlugins` + flight, ticking a fixed `1/fps` per update.
fn app(fps: u32) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, FlightPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / f64::from(fps),
        )));
    // The first update only initialises the clock (delta = 0).
    app.update();
    app
}

fn spawn_jet(app: &mut App, input: FlightInput) -> Entity {
    let cfg = app.world().resource::<FlightConfig>().clone();
    app.world_mut()
        .spawn((input, FlightState::new(&cfg), cfg.spawn_transform()))
        .id()
}

fn run_for(app: &mut App, seconds: f32) {
    let until = app.world().resource::<Time>().elapsed_secs() + seconds;
    while app.world().resource::<Time>().elapsed_secs() < until - 1e-4 {
        app.update();
    }
}

fn state(app: &App, jet: Entity) -> FlightState {
    *app.world().get::<FlightState>(jet).unwrap()
}

fn transform(app: &App, jet: Entity) -> Transform {
    *app.world().get::<Transform>(jet).unwrap()
}

#[test]
fn spawn_is_over_the_southern_suburbs_facing_the_centre() {
    let cfg = FlightConfig::default();
    let at = cfg.spawn_transform();
    assert_eq!(at.translation, Vec3::new(0.0, 400.0, 4_000.0));
    // The nose points at the aim point above the centre.
    let to_aim = (super::SPAWN_AIM - at.translation).normalize();
    assert!(
        at.forward().angle_between(to_aim) < 1e-4,
        "{:?}",
        at.forward()
    );
}

#[test]
fn aircraft_axes_and_spawn_pitch_follow_the_world_convention() {
    // Check the world-space convention directly, without relying on Transform::forward().
    assert_eq!(Quat::IDENTITY * Vec3::NEG_Z, Vec3::NEG_Z);
    assert_eq!(Quat::IDENTITY * Vec3::Y, Vec3::Y);

    let cfg = FlightConfig::default();
    // Starts over the suburbs, nose slightly down towards the centre.
    assert!(cfg.spawn_pitch < 0.0 && cfg.spawn_pitch > (-10.0_f32).to_radians());

    let spawn_nose = cfg.spawn_transform().rotation * Vec3::NEG_Z;
    let expected_nose = Vec3::new(0.0, cfg.spawn_pitch.sin(), -cfg.spawn_pitch.cos());
    assert!(
        spawn_nose.distance(expected_nose) < 1e-6,
        "spawn nose {spawn_nose:?}, expected {expected_nose:?}"
    );
    assert!(
        spawn_nose.y < 0.0,
        "negative spawn pitch must point the nose slightly down: {spawn_nose:?}"
    );
}

#[test]
fn throttle_accelerates_at_configured_rate_up_to_max_speed() {
    let mut app = app(60);
    let cfg = app.world().resource::<FlightConfig>().clone();
    let jet = spawn_jet(
        &mut app,
        FlightInput {
            throttle: 1.0,
            ..default()
        },
    );

    run_for(&mut app, 2.0);
    let expected = cfg.speed_initial + 2.0 * cfg.throttle_rate;
    assert!(
        (state(&app, jet).speed - expected).abs() < 0.5,
        "{:?}",
        state(&app, jet)
    );

    run_for(&mut app, 10.0);
    assert_eq!(state(&app, jet).speed, cfg.speed_max);

    // Velocity follows the nose.
    let s = state(&app, jet);
    let forward = transform(&app, jet).forward();
    // acos-based angle_between is too coarse near 0 in f32; compare directions instead.
    assert!(s.velocity.normalize().dot(*forward) > 1.0 - 1e-6);
    assert!((s.velocity.length() - cfg.speed_max).abs() < 1e-2);
}

#[test]
fn roll_rate_approaches_but_never_exceeds_limit() {
    let mut app = app(60);
    let cfg = app.world().resource::<FlightConfig>().clone();
    let jet = spawn_jet(
        &mut app,
        FlightInput {
            roll: 1.0,
            ..default()
        },
    );

    let mut previous = 0.0;
    for frame in 0..180 {
        app.update();
        if frame == 20 {
            // Rolling left lowers the left wing, so the right wing points up.
            assert!(transform(&app, jet).right().y > 0.1);
        }
        let rate = state(&app, jet).roll_rate;
        assert!(
            rate <= cfg.max_roll_rate + 1e-6,
            "roll rate {rate} over limit"
        );
        assert!(rate >= previous, "roll rate must build up monotonically");
        previous = rate;
    }
    // After three seconds the stick has (almost) reached full rate.
    assert!(previous > 0.99 * cfg.max_roll_rate, "roll rate {previous}");
}

#[test]
fn centred_stick_damps_rotation_and_flies_straight() {
    let mut app = app(60);
    let jet = spawn_jet(
        &mut app,
        FlightInput {
            pitch: 1.0,
            yaw: 1.0,
            ..default()
        },
    );
    run_for(&mut app, 1.0);
    let spinning = state(&app, jet);
    assert!(spinning.pitch_rate > 1.0 && spinning.yaw_rate > 0.5);

    app.world_mut()
        .entity_mut(jet)
        .insert(FlightInput::default());
    run_for(&mut app, 2.0);
    let settled = state(&app, jet);
    assert!(
        settled.pitch_rate.abs() < 0.01 * spinning.pitch_rate,
        "{settled:?}"
    );
    assert!(
        settled.yaw_rate.abs() < 0.01 * spinning.yaw_rate,
        "{settled:?}"
    );

    // Without rotation the heading stays put.
    run_for(&mut app, 1.0);
    let before = transform(&app, jet).forward();
    run_for(&mut app, 1.0);
    let after = transform(&app, jet).forward();
    assert!(before.angle_between(*after) < 1e-3);
}

#[test]
fn result_does_not_depend_on_frame_rate() {
    let input = FlightInput {
        roll: 1.0,
        pitch: 0.5,
        throttle: 1.0,
        ..default()
    };
    let mut slow = app(30);
    let mut fast = app(144);
    let jet_slow = spawn_jet(&mut slow, input);
    let jet_fast = spawn_jet(&mut fast, input);
    run_for(&mut slow, 2.0);
    run_for(&mut fast, 2.0);

    let (a, b) = (state(&slow, jet_slow), state(&fast, jet_fast));
    assert!(
        (a.roll_rate - b.roll_rate).abs() < 0.02 * b.roll_rate,
        "{a:?} vs {b:?}"
    );
    assert!((a.speed - b.speed).abs() < 0.5);
    let (ta, tb) = (transform(&slow, jet_slow), transform(&fast, jet_fast));
    assert!(ta.rotation.angle_between(tb.rotation) < 0.1);
    assert!(ta.translation.distance(tb.translation) < 10.0);
}

#[test]
fn driven_flight_is_consistent_at_30_60_and_144_fps() {
    let input = FlightInput {
        roll: 1.0,
        pitch: 0.5,
        throttle: 1.0,
        ..default()
    };
    let mut results = Vec::new();

    for fps in [30, 60, 144] {
        let mut app = app(fps);
        let jet = spawn_jet(&mut app, input);
        run_for(&mut app, 2.0);
        results.push((fps, state(&app, jet), transform(&app, jet)));
    }

    for first in 0..results.len() {
        for second in first + 1..results.len() {
            let (fps_a, state_a, transform_a) = results[first];
            let (fps_b, state_b, transform_b) = results[second];
            let comparison = format!("{fps_a} fps vs {fps_b} fps");

            assert!(
                (state_a.roll_rate - state_b.roll_rate).abs() < 1e-4,
                "{comparison}: {state_a:?} vs {state_b:?}"
            );
            assert!(
                (state_a.pitch_rate - state_b.pitch_rate).abs() < 1e-4,
                "{comparison}: {state_a:?} vs {state_b:?}"
            );
            assert!(
                (state_a.speed - state_b.speed).abs() < 0.01,
                "{comparison}: {state_a:?} vs {state_b:?}"
            );
            assert!(
                transform_a.rotation.angle_between(transform_b.rotation) < 0.075,
                "{comparison}: rotations diverged"
            );
            assert!(
                transform_a.translation.distance(transform_b.translation) < 8.0,
                "{comparison}: positions diverged"
            );
        }
    }
}

#[test]
fn neutral_damping_is_consistent_at_30_60_and_144_fps() {
    let cfg = FlightConfig::default();
    let initial = FlightState {
        roll_rate: 0.5 * cfg.max_roll_rate,
        pitch_rate: 0.5 * cfg.max_pitch_rate,
        yaw_rate: -0.5 * cfg.max_yaw_rate,
        speed: cfg.speed_initial,
        velocity: Vec3::ZERO,
    };

    for fps in [30, 60, 144] {
        let mut app = app(fps);
        let jet = spawn_jet(&mut app, FlightInput::default());
        app.world_mut().entity_mut(jet).insert(initial);
        run_for(&mut app, 1.0);

        let damped = state(&app, jet);
        let expected_roll =
            initial.roll_rate * (-cfg.roll_response).exp() * cfg.idle_damping_per_second;
        let expected_pitch =
            initial.pitch_rate * (-cfg.pitch_response).exp() * cfg.idle_damping_per_second;
        let expected_yaw =
            initial.yaw_rate * (-cfg.yaw_response).exp() * cfg.idle_damping_per_second;

        assert!(
            (damped.roll_rate - expected_roll).abs() < 2e-5,
            "{fps} fps: roll {} vs {expected_roll}",
            damped.roll_rate
        );
        assert!(
            (damped.pitch_rate - expected_pitch).abs() < 2e-5,
            "{fps} fps: pitch {} vs {expected_pitch}",
            damped.pitch_rate
        );
        assert!(
            (damped.yaw_rate - expected_yaw).abs() < 2e-5,
            "{fps} fps: yaw {} vs {expected_yaw}",
            damped.yaw_rate
        );
    }
}

#[test]
fn half_second_frame_is_clamped_to_one_tenth_second_of_simulation() {
    let input = FlightInput {
        pitch: 0.5,
        throttle: 1.0,
        ..default()
    };
    let mut stalled = app(2);
    let mut reference = app(10);
    let cfg = stalled.world().resource::<FlightConfig>().clone();
    assert_eq!(cfg.max_dt, 0.1, "the stall guard is intentionally 100 ms");

    let stalled_jet = spawn_jet(&mut stalled, input);
    let reference_jet = spawn_jet(&mut reference, input);
    stalled.update();
    reference.update();

    let stalled_state = state(&stalled, stalled_jet);
    let reference_state = state(&reference, reference_jet);
    let simulated_dt = (stalled_state.speed - cfg.speed_initial) / cfg.throttle_rate;
    assert!(
        simulated_dt <= 0.1 + 1e-6,
        "500 ms frame advanced {simulated_dt} s of simulation"
    );
    assert!((stalled_state.speed - reference_state.speed).abs() < 1e-5);
    assert!((stalled_state.pitch_rate - reference_state.pitch_rate).abs() < 1e-6);

    let stalled_transform = transform(&stalled, stalled_jet);
    let reference_transform = transform(&reference, reference_jet);
    assert!(
        stalled_transform
            .rotation
            .angle_between(reference_transform.rotation)
            < 1e-6
    );
    assert!(
        stalled_transform
            .translation
            .distance(reference_transform.translation)
            < 1e-5
    );
}

#[test]
fn jet_never_sinks_below_the_ground() {
    let mut app = app(60);
    let cfg = app.world().resource::<FlightConfig>().clone();
    let jet = spawn_jet(
        &mut app,
        FlightInput {
            pitch: -1.0,
            throttle: 1.0,
            ..default()
        },
    );
    run_for(&mut app, 15.0);
    assert!(transform(&app, jet).translation.y >= cfg.min_altitude);
}
