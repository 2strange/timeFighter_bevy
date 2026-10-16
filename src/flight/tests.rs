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
fn throttle_accelerates_at_configured_rate_up_to_max_speed() {
    let mut app = app(60);
    let cfg = app.world().resource::<FlightConfig>().clone();
    let jet = spawn_jet(&mut app, FlightInput { throttle: 1.0, ..default() });

    run_for(&mut app, 2.0);
    let expected = cfg.speed_initial + 2.0 * cfg.throttle_rate;
    assert!((state(&app, jet).speed - expected).abs() < 0.5, "{:?}", state(&app, jet));

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
    let jet = spawn_jet(&mut app, FlightInput { roll: 1.0, ..default() });

    let mut previous = 0.0;
    for frame in 0..180 {
        app.update();
        if frame == 20 {
            // Rolling left lowers the left wing, so the right wing points up.
            assert!(transform(&app, jet).right().y > 0.1);
        }
        let rate = state(&app, jet).roll_rate;
        assert!(rate <= cfg.max_roll_rate + 1e-6, "roll rate {rate} over limit");
        assert!(rate >= previous, "roll rate must build up monotonically");
        previous = rate;
    }
    // After three seconds the stick has (almost) reached full rate.
    assert!(previous > 0.99 * cfg.max_roll_rate, "roll rate {previous}");
}

#[test]
fn centred_stick_damps_rotation_and_flies_straight() {
    let mut app = app(60);
    let jet = spawn_jet(&mut app, FlightInput { pitch: 1.0, yaw: 1.0, ..default() });
    run_for(&mut app, 1.0);
    let spinning = state(&app, jet);
    assert!(spinning.pitch_rate > 1.0 && spinning.yaw_rate > 0.5);

    app.world_mut().entity_mut(jet).insert(FlightInput::default());
    run_for(&mut app, 2.0);
    let settled = state(&app, jet);
    assert!(settled.pitch_rate.abs() < 0.01 * spinning.pitch_rate, "{settled:?}");
    assert!(settled.yaw_rate.abs() < 0.01 * spinning.yaw_rate, "{settled:?}");

    // Without rotation the heading stays put.
    run_for(&mut app, 1.0);
    let before = transform(&app, jet).forward();
    run_for(&mut app, 1.0);
    let after = transform(&app, jet).forward();
    assert!(before.angle_between(*after) < 1e-3);
}

#[test]
fn result_does_not_depend_on_frame_rate() {
    let input = FlightInput { roll: 1.0, pitch: 0.5, throttle: 1.0, ..default() };
    let mut slow = app(30);
    let mut fast = app(144);
    let jet_slow = spawn_jet(&mut slow, input);
    let jet_fast = spawn_jet(&mut fast, input);
    run_for(&mut slow, 2.0);
    run_for(&mut fast, 2.0);

    let (a, b) = (state(&slow, jet_slow), state(&fast, jet_fast));
    assert!((a.roll_rate - b.roll_rate).abs() < 0.02 * b.roll_rate, "{a:?} vs {b:?}");
    assert!((a.speed - b.speed).abs() < 0.5);
    let (ta, tb) = (transform(&slow, jet_slow), transform(&fast, jet_fast));
    assert!(ta.rotation.angle_between(tb.rotation) < 0.1);
    assert!(ta.translation.distance(tb.translation) < 10.0);
}

#[test]
fn jet_never_sinks_below_the_ground() {
    let mut app = app(60);
    let cfg = app.world().resource::<FlightConfig>().clone();
    let jet = spawn_jet(&mut app, FlightInput { pitch: -1.0, throttle: 1.0, ..default() });
    run_for(&mut app, 15.0);
    assert!(transform(&app, jet).translation.y >= cfg.min_altitude);
}
