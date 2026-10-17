//! The page's touch state: parsing, the tilt/stick/thrust mapping, buttons
//! as key presses, and the whole path in the headless game.

use std::time::Duration;

use bevy::{input::InputPlugin, time::TimeUpdateStrategy};

use super::*;
use crate::{
    AppMode, AppModePlugin, combat::Trigger, game_state::Outcome, test_support::headless_app,
};

#[test]
fn the_page_line_is_read_and_damage_is_skipped() {
    let input = MobileInput::parse(
        "src=gyro;p=4.5;r=-12;t=0.75;g=1;m=3;n=1;x=2;a=0;M=5;name=J%C3%BCrgen%20%3B%3D%20%E2%9C%88;ok=1;esc=0;zz=9",
    );
    assert_eq!(input.steering, Steering::Gyro);
    assert_eq!((input.pitch, input.roll), (4.5, -12.0));
    assert_eq!(input.thrust, Some(0.75));
    assert!(input.gun);
    assert_eq!((input.missile, input.next_target, input.pause), (3, 1, 2));
    assert_eq!((input.again, input.menu), (0, 5));
    assert_eq!(input.name.as_deref(), Some("Jürgen ;= ✈"));
    assert_eq!((input.name_ok, input.name_cancel), (1, 0));

    let broken = MobileInput::parse("src=stick;p=NaN;r=x;t=7;m=-1;garbage");
    assert_eq!(broken.steering, Steering::Stick);
    assert_eq!((broken.pitch, broken.roll), (0.0, 0.0));
    assert_eq!(broken.thrust, Some(1.0), "clamped");
    assert_eq!(broken.missile, 0);
    assert_eq!(MobileInput::parse(""), MobileInput::default());
}

#[test]
fn tilt_has_a_dead_zone_and_full_deflection() {
    assert_eq!(tilt_axis(0.0), 0.0);
    assert_eq!(tilt_axis(TILT_DEADZONE), 0.0);
    assert_eq!(tilt_axis(-TILT_DEADZONE * 0.9), 0.0);
    assert_eq!(tilt_axis(TILT_FULL), 1.0);
    assert_eq!(tilt_axis(-90.0), -1.0);
    let half = (TILT_DEADZONE + TILT_FULL) / 2.0;
    assert!((tilt_axis(half) - 0.5).abs() < 1e-6);
    assert!((tilt_axis(-half) + 0.5).abs() < 1e-6);
    assert_eq!(stick_axis(0.05), 0.0);
    assert_eq!(stick_axis(-1.0), -1.0);
    assert!(stick_axis(0.5) > 0.4 && stick_axis(0.5) < 0.5);
}

#[test]
fn tilting_away_dives_and_right_down_rolls_right() {
    let gyro = MobileInput {
        steering: Steering::Gyro,
        pitch: 30.0,
        roll: 30.0,
        ..default()
    };
    // Keyboard convention: pitch > 0 = nose up (S), roll > 0 = left wing
    // down (A).
    assert_eq!(gyro.flight_axes(), Some((-1.0, -1.0)));
    let stick = MobileInput {
        steering: Steering::Stick,
        pitch: -1.0,
        roll: -1.0,
        ..default()
    };
    assert_eq!(stick.flight_axes(), Some((1.0, 1.0)), "pulled back, left");
    assert_eq!(MobileInput::default().flight_axes(), None);
}

#[test]
fn the_thrust_slider_flies_to_a_speed() {
    let cfg = FlightConfig::default();
    assert_eq!(thrust_throttle(1.0, cfg.speed_min, &cfg), 1.0);
    assert_eq!(thrust_throttle(0.0, cfg.speed_max, &cfg), -1.0);
    let middle = (cfg.speed_min + cfg.speed_max) / 2.0;
    assert_eq!(thrust_throttle(0.5, middle, &cfg), 0.0);
    assert!(thrust_throttle(0.5, middle - 3.0, &cfg) > 0.0);
}

#[test]
fn percent_escapes_decode_as_utf8() {
    assert_eq!(percent_decode("a%20b"), "a b");
    assert_eq!(percent_decode("%E9%A3%9B%E8%A1%8C"), "飛行");
    assert_eq!(percent_decode("100%"), "100%", "a lone % stays");
    assert_eq!(percent_decode("%zz"), "%zz");
}

/// Keys and the jet with only the module's systems.
fn keys_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, InputPlugin))
        .init_resource::<FlightConfig>()
        .configure_sets(
            Update,
            (FlightSystems::Input, FlightSystems::Simulate).chain(),
        )
        .add_plugins(MobilePlugin);
    app.world_mut().spawn((
        PlayerAircraft,
        FlightInput::default(),
        FlightState {
            speed: 150.0,
            ..default()
        },
    ));
    app.update();
    app
}

fn keys(app: &App) -> &ButtonInput<KeyCode> {
    app.world().resource::<ButtonInput<KeyCode>>()
}

fn set(app: &mut App, change: impl FnOnce(&mut MobileInput)) {
    change(&mut app.world_mut().resource_mut::<MobileInput>());
    app.update();
}

#[test]
fn buttons_become_key_presses_and_taps_count_once() {
    let mut app = keys_app();
    set(&mut app, |i| i.gun = true);
    assert!(keys(&app).just_pressed(KeyCode::Space));
    app.update();
    assert!(keys(&app).pressed(KeyCode::Space), "held");
    assert!(!keys(&app).just_pressed(KeyCode::Space));
    set(&mut app, |i| i.gun = false);
    assert!(!keys(&app).pressed(KeyCode::Space));

    // Two taps between frames are still taps; a counter that stays is not.
    set(&mut app, |i| i.missile += 1);
    assert!(keys(&app).just_pressed(KeyCode::ShiftRight));
    app.update();
    assert!(!keys(&app).pressed(KeyCode::ShiftRight), "released again");
    app.update();
    assert!(!keys(&app).just_pressed(KeyCode::ShiftRight));
    for (key, tap) in [
        (
            KeyCode::Tab,
            (|i: &mut MobileInput| i.next_target += 1) as fn(&mut MobileInput),
        ),
        (KeyCode::KeyP, |i| i.pause += 1),
        (KeyCode::KeyR, |i| i.again += 1),
        (KeyCode::KeyM, |i| i.menu += 1),
    ] {
        set(&mut app, tap);
        assert!(keys(&app).just_pressed(key), "{key:?}");
    }

    // A real Space held on a keyboard is not released by the overlay.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    assert!(keys(&app).pressed(KeyCode::Space));
}

#[test]
fn tilt_and_thrust_steer_the_jet_over_the_keyboard() {
    let mut app = keys_app();
    set(&mut app, |i| {
        i.steering = Steering::Gyro;
        i.pitch = -TILT_FULL;
        i.roll = 0.0;
        i.thrust = Some(1.0);
    });
    let input = *app
        .world_mut()
        .query_filtered::<&FlightInput, With<PlayerAircraft>>()
        .single(app.world())
        .unwrap();
    assert_eq!(input.pitch, 1.0, "top edge towards you: nose up");
    assert_eq!(input.roll, 0.0);
    assert_eq!(input.throttle, 1.0, "faster wanted");
}

#[test]
fn invert_pitch_turns_tilt_and_keyboard_around() {
    let mut app = keys_app();
    app.insert_resource(PitchInvert(true));
    set(&mut app, |i| {
        i.steering = Steering::Gyro;
        i.pitch = TILT_FULL;
    });
    let pitch = |app: &mut App| {
        app.world_mut()
            .query_filtered::<&FlightInput, With<PlayerAircraft>>()
            .single(app.world())
            .unwrap()
            .pitch
    };
    assert_eq!(pitch(&mut app), 1.0, "top edge away pulls up when inverted");
    assert_eq!(PitchInvert(true).apply(-1.0), 1.0);
    assert_eq!(PitchInvert(false).apply(-1.0), -1.0);
}

#[test]
fn the_page_mode_follows_menu_round_pause_and_dialog() {
    let pause = Pause::default();
    assert_eq!(page_mode(Some(&pause), None, None), PageMode::Play);
    let menu = Pause {
        menu: true,
        ..default()
    };
    assert_eq!(page_mode(Some(&menu), None, None), PageMode::Menu);
    let paused = Pause {
        player: true,
        ..default()
    };
    assert_eq!(page_mode(Some(&paused), None, None), PageMode::Paused);
    let over = Round {
        phase: RoundPhase::Over(Outcome::Victory),
        ..default()
    };
    assert_eq!(page_mode(Some(&pause), Some(&over), None), PageMode::Over);
}

// --- In the game --------------------------------------------------------------

fn game() -> App {
    let mut app = headless_app(AppModePlugin(AppMode::Game));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    app.update();
    app.world_mut().resource_mut::<crate::menu::Menu>().open = false;
    app.world_mut()
        .write_message(crate::game_state::RestartRound);
    for _ in 0..3 {
        app.update();
    }
    app
}

#[test]
fn the_gun_button_fires_in_a_round() {
    let mut app = game();
    set(&mut app, |i| i.gun = true);
    app.update();
    let trigger = *app
        .world_mut()
        .query_filtered::<&Trigger, With<PlayerAircraft>>()
        .single(app.world())
        .unwrap();
    assert!(trigger.gun);
}

#[test]
fn the_phone_keyboard_names_the_pilot() {
    let mut app = game();
    app.world_mut().resource_mut::<Round>().phase = RoundPhase::Over(Outcome::ShotDown);
    app.update();
    app.update();
    assert!(app.world().resource::<ScoreState>().dialog);
    assert_eq!(
        page_mode(
            Some(app.world().resource::<Pause>()),
            Some(app.world().resource::<Round>()),
            Some(app.world().resource::<ScoreState>())
        ),
        PageMode::Dialog
    );
    set(&mut app, |i| i.name = Some("Jürgen ✈".into()));
    assert_eq!(
        app.world().resource::<ScoreState>().field.text(),
        "Jürgen ✈"
    );
    set(&mut app, |i| i.name_ok += 1);
    let state = app.world().resource::<ScoreState>();
    assert!(!state.dialog);
    assert_eq!(state.submitted.as_ref().unwrap().name, "Jürgen ✈");
    // Fly again by tap.
    set(&mut app, |i| i.again += 1);
    app.update();
    assert!(app.world().resource::<Round>().is_running());
}
