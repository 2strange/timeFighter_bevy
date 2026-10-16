//! Headless tests of the start menu in the full game (no window, no GPU).

use std::time::Duration;

use bevy::{input::ButtonState, time::TimeUpdateStrategy};

use super::*;
use crate::{
    AppMode, AppModePlugin,
    game_state::{Difficulty, Outcome},
    test_support::{headless_app, key, tap},
};

fn game() -> App {
    let mut app = headless_app(AppModePlugin(AppMode::Game));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    app.update();
    app.update();
    app
}

fn menu(app: &App) -> Menu {
    *app.world().resource::<Menu>()
}

fn settings(app: &App) -> GameSettings {
    *app.world().resource::<GameSettings>()
}

/// Whether the key help (the text listing "this help") is shown.
fn help_visible(app: &mut App) -> bool {
    app.world_mut()
        .query::<(&Text, &Visibility)>()
        .iter(app.world())
        .any(|(text, visibility)| {
            text.0.contains("this help") && *visibility == Visibility::Visible
        })
}

/// Moves the cursor to `row` with the arrow keys.
fn go_to(app: &mut App, row: RowId) {
    for _ in 0..20 {
        if menu(app).cursor == row {
            return;
        }
        let rows = app.world().resource::<MenuSelection>().rows();
        let at = rows.iter().position(|r| r.id == menu(app).cursor).unwrap();
        let target = rows.iter().position(|r| r.id == row).unwrap();
        tap(
            app,
            if target < at {
                KeyCode::ArrowUp
            } else {
                KeyCode::ArrowDown
            },
        );
    }
    panic!("cursor never reached {row:?}");
}

#[test]
fn the_game_opens_in_the_menu_paused_and_without_help() {
    let mut app = game();
    assert!(menu(&app).open);
    assert!(app.world().resource::<Pause>().is_paused());
    assert!(!help_visible(&mut app), "help must wait for the round");
    // P does not pause over the menu.
    tap(&mut app, KeyCode::KeyP);
    assert!(!app.world().resource::<Pause>().player);
}

#[test]
fn choosing_with_arrows_and_enter_starts_a_round_with_those_settings() {
    let mut app = game();
    go_to(&mut app, RowId::Difficulty);
    tap(&mut app, KeyCode::ArrowRight); // Normal -> Hard
    go_to(&mut app, RowId::Lighting);
    tap(&mut app, KeyCode::ArrowRight); // Evening
    assert_eq!(
        *app.world().resource::<Lighting>(),
        Lighting::Evening,
        "light preview"
    );
    tap(&mut app, KeyCode::Enter);

    assert!(!menu(&app).open);
    assert!(!app.world().resource::<Pause>().is_paused());
    assert_eq!(settings(&app).difficulty, Difficulty::Hard);
    assert_eq!(settings(&app).lighting, Lighting::Evening);
    for _ in 0..3 {
        app.update();
    }
    assert!(help_visible(&mut app), "help shows when the round starts");
}

#[test]
fn protect_can_be_chosen_and_greyed_out_modes_are_skipped() {
    let mut app = game();
    go_to(&mut app, RowId::Mode);
    tap(&mut app, KeyCode::ArrowRight);
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(
        app.world().resource::<MenuSelection>().mode,
        MenuMode::Protect
    );
    tap(&mut app, KeyCode::Enter);
    assert_eq!(settings(&app).mode, crate::game_state::GameMode::Protect);
}

#[test]
fn long_value_lists_show_as_arrows() {
    let mut app = game();
    {
        let mut selection = app.world_mut().resource_mut::<MenuSelection>();
        selection.select(RowId::Mode, 1);
        selection.select(RowId::Difficulty, 5);
    }
    app.update();
    let buttons: Vec<MenuButton> = app
        .world_mut()
        .query::<&MenuButton>()
        .iter(app.world())
        .copied()
        .filter(|b| b.row == RowId::Custom(CustomField::WaveInterval))
        .collect();
    // 25 steps, but only "<" and ">".
    assert_eq!(buttons.len(), 2, "{}", buttons.len());
}

#[test]
fn custom_values_reach_the_settings() {
    let mut app = game();
    go_to(&mut app, RowId::Difficulty);
    for _ in 0..6 {
        tap(&mut app, KeyCode::ArrowRight);
    }
    assert!(app.world().resource::<MenuSelection>().is_custom());
    go_to(&mut app, RowId::Custom(CustomField::UnlimitedGun));
    tap(&mut app, KeyCode::ArrowRight);
    go_to(&mut app, RowId::Custom(CustomField::Enemies));
    tap(&mut app, KeyCode::ArrowLeft);
    tap(&mut app, KeyCode::Enter);

    let Difficulty::Custom(params) = settings(&app).difficulty else {
        panic!("custom expected, got {:?}", settings(&app).difficulty);
    };
    assert!(params.unlimited_gun);
    assert!(params.enemies < Difficulty::Insane.params().enemies);
}

#[test]
fn result_screen_offers_the_menu_on_m() {
    let mut app = game();
    tap(&mut app, KeyCode::Enter);
    assert!(!menu(&app).open);
    // M during a round does nothing.
    tap(&mut app, MENU_KEY);
    assert!(!menu(&app).open);

    app.world_mut().resource_mut::<Round>().phase = RoundPhase::Over(Outcome::Victory);
    app.update();
    tap(&mut app, MENU_KEY);
    assert!(menu(&app).open);
    assert_eq!(menu(&app).cursor, RowId::Start);
}

#[test]
fn holding_a_key_moves_once() {
    let mut app = game();
    go_to(&mut app, RowId::Mode);
    key(&mut app, KeyCode::ArrowDown, ButtonState::Pressed);
    for _ in 0..5 {
        app.update();
    }
    key(&mut app, KeyCode::ArrowDown, ButtonState::Released);
    app.update();
    assert_eq!(menu(&app).cursor, RowId::Difficulty);
}

fn shots_fired(app: &mut App) -> usize {
    let messages = app.world().resource::<Messages<crate::combat::ShotFired>>();
    messages.iter_current_update_messages().count()
}

#[test]
fn space_held_through_start_does_not_fire_until_pressed_again() {
    let mut app = game();
    // Space starts the game from the menu and is held down afterwards.
    key(&mut app, KeyCode::Space, ButtonState::Pressed);
    app.update();
    assert!(!menu(&app).open);
    let mut fired = 0;
    for _ in 0..30 {
        app.update();
        fired += shots_fired(&mut app);
    }
    assert_eq!(fired, 0, "held Space fired the gun after Start");

    // Released and pressed again: now it fires.
    key(&mut app, KeyCode::Space, ButtonState::Released);
    app.update();
    key(&mut app, KeyCode::Space, ButtonState::Pressed);
    let mut fired = 0;
    for _ in 0..10 {
        app.update();
        fired += shots_fired(&mut app);
    }
    assert!(fired > 0, "gun did not fire after a fresh press");
}

#[test]
fn other_keys_held_at_start_do_not_block_the_weapons() {
    let mut app = game();
    // Accelerating while pressing Enter: throttle is not a weapon input.
    key(&mut app, KeyCode::ArrowUp, ButtonState::Pressed);
    app.update();
    tap(&mut app, KeyCode::Enter);
    assert!(!menu(&app).open);
    app.update();
    assert!(!app.world().resource::<TriggerHold>().is_holding());
}
