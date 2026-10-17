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
fn protect_can_be_chosen_with_the_keys() {
    let mut app = game();
    go_to(&mut app, RowId::Mode);
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
    // Normal's 2000 rounds, four steps left: unlimited.
    go_to(&mut app, RowId::Custom(CustomField::Gun));
    for _ in 0..4 {
        tap(&mut app, KeyCode::ArrowLeft);
    }
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

#[test]
fn choosing_protect_in_the_menu_starts_a_protect_round() {
    use crate::game_state::{GameMode, Mission};

    let mut app = game();
    go_to(&mut app, RowId::Mode);
    tap(&mut app, KeyCode::ArrowRight); // Protect
    tap(&mut app, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert!(!menu(&app).open);
    // The round that actually runs, not just the settings resource.
    assert_eq!(app.world().resource::<Mission>().mode, GameMode::Protect);
    assert!(app.world().resource::<Round>().is_running());
}

#[test]
fn start_page_requests_pick_mode_and_level_by_name_or_index() {
    let (selection, menu) =
        after_start_page(MenuSelection::default(), Some("mode=protect;level=hard"));
    assert_eq!(selection.mode, MenuMode::Protect);
    assert_eq!(selection.settings().difficulty, Difficulty::Hard);
    assert!(!menu.open, "a fixed level starts right away");

    let (selection, _) = after_start_page(MenuSelection::default(), Some("level=super-easy"));
    assert_eq!(selection.settings().difficulty, Difficulty::SuperEasy);
    let (selection, _) = after_start_page(MenuSelection::default(), Some("level=4"));
    assert_eq!(selection.settings().difficulty, Difficulty::Insane);

    // Unknown values change nothing, but still start.
    let (selection, menu) = after_start_page(
        MenuSelection::default(),
        Some("mode=dogfight;level=impossible"),
    );
    assert_eq!(selection, MenuSelection::default());
    assert!(!menu.open);

    // The one-shot flag alone: start with the stored selection.
    let mut stored = MenuSelection::default();
    stored.select(RowId::Difficulty, 1);
    let (selection, menu) = after_start_page(stored.clone(), Some(""));
    assert_eq!(selection, stored);
    assert!(!menu.open);

    // No request: the menu as usual.
    assert_eq!(after_start_page(stored.clone(), None).1, Menu::default());
}

#[test]
fn custom_from_the_start_page_opens_the_menu_on_the_custom_values() {
    let (selection, menu) = after_start_page(MenuSelection::default(), Some("level=custom"));
    assert!(selection.is_custom());
    assert!(menu.open);
    assert!(matches!(menu.cursor, RowId::Custom(_)), "{:?}", menu.cursor);
}

#[test]
fn url_queries_count_as_start_requests_only_with_mode_or_level() {
    use super::storage::query_names_a_start;
    assert!(query_names_a_start("?mode=protect&level=hard"));
    assert!(query_names_a_start("level=easy"));
    assert!(!query_names_a_start("?showroom"));
    assert!(!query_names_a_start(""));
}

#[test]
fn autostarted_game_runs_without_menu_or_pause() {
    let mut app = headless_app(AppModePlugin(AppMode::Game));
    let (selection, start) =
        after_start_page(MenuSelection::default(), Some("mode=protect;level=easy"));
    app.insert_resource(selection).insert_resource(start);
    for _ in 0..3 {
        app.update();
    }
    assert!(!menu(&app).open);
    assert!(!app.world().resource::<Pause>().is_paused());
    assert!(app.world().resource::<Round>().is_running());
    assert_eq!(settings(&app).difficulty, Difficulty::Easy);
    assert_eq!(
        app.world().resource::<crate::game_state::Mission>().mode,
        crate::game_state::GameMode::Protect
    );
}

#[test]
fn choosing_battle_in_the_menu_starts_a_battle_round() {
    use crate::game_state::{GameMode, Mission};

    let mut app = game();
    go_to(&mut app, RowId::Mode);
    tap(&mut app, KeyCode::ArrowRight);
    tap(&mut app, KeyCode::ArrowRight); // Battle
    tap(&mut app, KeyCode::Enter);
    for _ in 0..3 {
        app.update();
    }
    assert!(!menu(&app).open);
    assert_eq!(app.world().resource::<Mission>().mode, GameMode::Battle);
    assert!(app.world().resource::<Round>().is_running());
}

#[test]
fn start_page_request_for_battle() {
    let (selection, menu) =
        after_start_page(MenuSelection::default(), Some("mode=battle;level=normal"));
    assert_eq!(
        selection.settings().mode,
        crate::game_state::GameMode::Battle
    );
    assert!(!menu.open);
}

#[test]
fn start_page_hands_over_the_light() {
    let (selection, _) = after_start_page(
        MenuSelection::default(),
        Some("mode=hunt;level=easy;light=evening"),
    );
    assert_eq!(selection.lighting, crate::world::Lighting::Evening);
    let (selection, _) = after_start_page(selection, Some("light=day"));
    assert_eq!(selection.lighting, crate::world::Lighting::Day);
    let (selection, _) = after_start_page(selection, Some("light=night"));
    assert_eq!(
        selection.lighting,
        crate::world::Lighting::Day,
        "unknown light ignored"
    );
}

#[test]
fn q_and_e_switch_the_enemy_tab_and_the_cursor_follows() {
    let mut app = game();
    {
        let mut selection = app.world_mut().resource_mut::<MenuSelection>();
        selection.select(RowId::Mode, 2);
        selection.select(RowId::Difficulty, 5);
    }
    app.update();
    go_to(&mut app, RowId::Custom(CustomField::FlightSize));
    tap(&mut app, KeyCode::KeyE);
    let tab = |app: &App| app.world().resource::<MenuSelection>().active_tab();
    assert_eq!(tab(&app), EnemyTab::Bombers);
    // The fighters' rows are gone: the cursor waits on the tab bar.
    assert_eq!(menu(&app).cursor, RowId::Tabs);
    tap(&mut app, KeyCode::KeyQ);
    assert_eq!(tab(&app), EnemyTab::Fighters);
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(tab(&app), EnemyTab::Bombers);
    go_to(&mut app, RowId::Custom(CustomField::Zeppelins));
}

#[test]
fn tabs_are_buttons_and_unused_ones_cannot_be_clicked() {
    let mut app = game();
    app.world_mut()
        .resource_mut::<MenuSelection>()
        .select(RowId::Difficulty, 5);
    app.update();
    let tabs: Vec<MenuButton> = app
        .world_mut()
        .query::<&MenuButton>()
        .iter(app.world())
        .copied()
        .filter(|b| b.row == RowId::Tabs)
        .collect();
    // Hunt: fighters open, bombers greyed out.
    assert_eq!(tabs.len(), 2);
    assert!(tabs.iter().any(|b| b.index == Some(0) && b.enabled));
    assert!(tabs.iter().any(|b| b.index == Some(1) && !b.enabled));
}

#[test]
fn the_panel_shrinks_only_when_the_window_is_too_small() {
    let window = Vec2::new(1280.0, 700.0);
    assert_eq!(fit_scale(Vec2::new(700.0, 600.0), window), 1.0);
    let scale = fit_scale(Vec2::new(700.0, 800.0), window);
    assert!((scale * 800.0 - 0.96 * 700.0).abs() < 0.01);
    assert!(fit_scale(Vec2::new(1400.0, 300.0), window) < 1.0);
    // Before the first layout.
    assert_eq!(fit_scale(Vec2::ZERO, window), 1.0);
}

/// The panel's laid-out size (logical px) in a 1280 x 700 window.
fn panel_size(setup: impl FnOnce(&mut MenuSelection)) -> Vec2 {
    let mut app = headless_app(AppModePlugin(AppMode::Game));
    app.world_mut().spawn((
        Window {
            resolution: (1280, 700).into(),
            ..default()
        },
        PrimaryWindow,
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    setup(&mut app.world_mut().resource_mut::<MenuSelection>());
    for _ in 0..4 {
        app.update();
    }
    let (node, transform) = app
        .world_mut()
        .query_filtered::<(&ComputedNode, &UiTransform), With<MenuPanel>>()
        .single(app.world())
        .unwrap();
    // Fits as it is: not scaled down.
    assert_eq!(transform.scale, Vec2::ONE);
    node.size() * node.inverse_scale_factor()
}

#[test]
fn the_custom_menu_fits_a_low_laptop_window() {
    // Every mode with its tabs; 600 px leaves room for the browser's bars
    // in a 700 px window.
    for (mode, tab) in [(0, 0), (1, 1), (2, 0), (2, 1)] {
        let size = panel_size(|s| {
            s.select(RowId::Mode, mode);
            s.select(RowId::Difficulty, 5);
            s.select(RowId::Tabs, tab);
        });
        assert!(size.y > 300.0, "laid out: {size:?}");
        assert!(
            size.y <= 600.0 && size.x <= 900.0,
            "mode {mode} tab {tab}: {size:?}"
        );
    }
}

#[test]
fn the_sound_row_sets_the_volume_and_follows_the_n_key() {
    let mut app = game();
    app.world_mut()
        .resource_mut::<MenuSelection>()
        .select(RowId::Sound, 4);
    go_to(&mut app, RowId::Sound);
    let sound = |app: &App| *app.world().resource::<crate::audio::SoundSettings>();
    // 100 %, two steps down: 50 %.
    tap(&mut app, KeyCode::ArrowLeft);
    tap(&mut app, KeyCode::ArrowLeft);
    assert!(sound(&app).enabled);
    assert!((sound(&app).volume - 0.5).abs() < 1e-6, "{:?}", sound(&app));
    // On to 25 % and off: the volume stays at 25 % behind the switch.
    tap(&mut app, KeyCode::ArrowLeft);
    tap(&mut app, KeyCode::ArrowLeft);
    assert!(!sound(&app).enabled);

    // Switched on with its own key: the row shows the volume again.
    app.world_mut()
        .resource_mut::<crate::audio::SoundSettings>()
        .enabled = true;
    app.update();
    assert_eq!(app.world().resource::<MenuSelection>().sound, 25);
}
