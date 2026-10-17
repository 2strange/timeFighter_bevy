//! Scoring rules, the files shared with the leaderboard worker, and the
//! result screen in the headless game.

use std::time::Duration;

use bevy::time::TimeUpdateStrategy;

use super::{
    rules::{
        Breakdown, KillWeapon, Level, MissionSize, ScoreSheet, kill_points, level_factor,
        level_key, limits_json, mode_key, score, sheet_json,
    },
    store::{NAME_KEY, OUTBOX_KEY, split_outbox, top_key},
    *,
};
use crate::{
    AppMode, AppModePlugin,
    combat::{Hit, Weapon},
    game_state::{Difficulty, GameMode},
    test_support::{headless_app, tap},
};

fn level(factor: f32, par_s: u32) -> Level {
    Level { factor, par_s }
}

#[test]
fn kills_pay_by_type_and_the_gun_pays_half_as_much_again() {
    let mut sheet = ScoreSheet::default();
    sheet.add_kill(EnemyKind::Bf109K4, KillWeapon::Missile);
    sheet.add_kill(EnemyKind::Bf109K4, KillWeapon::Gun);
    sheet.add_kill(EnemyKind::Me309, KillWeapon::Crash);
    sheet.add_kill(EnemyKind::Ju390, KillWeapon::Gun);
    sheet.add_kill(EnemyKind::Zeppelin, KillWeapon::Missile);
    assert_eq!(sheet.kill_count(), 5);
    assert_eq!(sheet.kill_points(), 100 + 150 + 150 + 600 + 1_500);
    assert_eq!(kill_points(EnemyKind::He177), 200);
}

#[test]
fn end_bonuses_come_only_with_a_victory() {
    let mut sheet = ScoreSheet {
        missiles_left: 4,
        city_percent: Some(87),
        duration_s: 100,
        zeppelins_outside: 1,
        ..default()
    };
    sheet.add_kill(EnemyKind::He177, KillWeapon::Missile);
    let lost = score(&sheet, level(1.0, 160));
    assert_eq!(lost.total, 200 + 500, "kills and the zeppelin bonus only");
    sheet.victory = true;
    let won = score(&sheet, level(1.0, 160));
    assert_eq!(
        won,
        Breakdown {
            kills: 200,
            zeppelins_outside: 500,
            missiles: 300,
            city: 1_740,
            victory: 1_000,
            seconds_under_par: 60,
            time: 300,
            subtotal: 4_040,
            factor: 1.0,
            total: 4_040,
        }
    );
    // Slower than par: no time bonus, never negative.
    sheet.duration_s = 500;
    assert_eq!(score(&sheet, level(1.0, 160)).time, 0);
}

#[test]
fn the_level_multiplies_the_total() {
    let factors: Vec<f32> = Difficulty::PRESETS
        .iter()
        .map(|d| level_factor(*d))
        .collect();
    assert_eq!(factors, [0.5, 0.75, 1.0, 1.5, 2.5]);
    let mut sheet = ScoreSheet::default();
    sheet.add_kill(EnemyKind::Me309, KillWeapon::Missile);
    // 150 x 0.5 and 150 x 0.75 = 112.5: rounded half up.
    assert_eq!(score(&sheet, level(0.5, 0)).total, 75);
    assert_eq!(score(&sheet, level(0.75, 0)).total, 113);
    assert_eq!(score(&sheet, level(2.5, 0)).total, 375);
}

#[test]
fn par_time_grows_with_the_mission() {
    let size = |mode, difficulty| {
        MissionSize::of(&Mission::from_settings(&GameSettings {
            mode,
            difficulty,
            ..default()
        }))
    };
    let hunt = size(GameMode::Hunt, Difficulty::Normal);
    assert!(hunt.fighters > 0 && hunt.bombers == 0 && hunt.zeppelins == 0);
    assert_eq!(hunt.par_s(), hunt.fighters * 15);
    let battle = size(GameMode::Battle, Difficulty::Insane);
    assert!(battle.zeppelins >= 1, "{battle:?}");
    assert!(battle.par_s() > size(GameMode::Battle, Difficulty::Easy).par_s());
}

#[test]
fn custom_levels_stay_off_the_online_list() {
    let custom = GameSettings {
        difficulty: Difficulty::Custom(default()),
        ..default()
    };
    assert!(!rules::online(&custom));
    assert!(rules::online(&GameSettings::default()));
    assert_eq!(level_key(custom.difficulty), "custom");
}

#[test]
fn breakdown_json_lists_only_what_happened() {
    let mut sheet = ScoreSheet {
        missiles_left: 2,
        victory: true,
        ..default()
    };
    sheet.add_kill(EnemyKind::Bf109K4, KillWeapon::Gun);
    sheet.add_kill(EnemyKind::Bf109K4, KillWeapon::Gun);
    sheet.add_kill(EnemyKind::He177, KillWeapon::Crash);
    assert_eq!(
        sheet_json(&sheet),
        "{\"kills\":{\"bf109k4\":{\"gun\":2},\"he177\":{\"crash\":1}},\"zeppelins_outside\":0,\"missiles_left\":2,\"city_percent\":null,\"victory\":true}"
    );
}

/// Example rounds with their scores, shared with the worker's tests.
fn vectors_json() -> String {
    let mut items = Vec::new();
    let cases: [(GameMode, Difficulty, bool, u32); 6] = [
        (GameMode::Hunt, Difficulty::Normal, true, 140),
        (GameMode::Hunt, Difficulty::SuperEasy, false, 40),
        (GameMode::Protect, Difficulty::Easy, true, 200),
        (GameMode::Protect, Difficulty::Hard, false, 90),
        (GameMode::Battle, Difficulty::Normal, true, 400),
        (GameMode::Battle, Difficulty::Insane, true, 900),
    ];
    for (i, (mode, difficulty, victory, duration_s)) in cases.into_iter().enumerate() {
        let settings = GameSettings {
            mode,
            difficulty,
            ..default()
        };
        let size = MissionSize::of(&Mission::from_settings(&settings));
        let mut sheet = ScoreSheet {
            victory,
            duration_s,
            missiles_left: (i as u32 * 3) % 7,
            city_percent: (mode != GameMode::Hunt).then_some(60 + i as u32 * 5),
            zeppelins_outside: size.zeppelins.min(1),
            ..default()
        };
        let weapons = KillWeapon::ALL;
        let fighters = if victory {
            size.fighters
        } else {
            size.fighters / 2
        };
        for f in 0..fighters {
            let kind = if f % 2 == 0 {
                EnemyKind::Bf109K4
            } else {
                EnemyKind::Me309
            };
            sheet.add_kill(kind, weapons[f as usize % 3]);
        }
        let bombers = if victory {
            size.bombers
        } else {
            size.bombers / 2
        };
        for b in 0..bombers {
            let kind = if b == 0 {
                EnemyKind::Ju390
            } else {
                EnemyKind::He177
            };
            sheet.add_kill(kind, weapons[(b as usize + 1) % 3]);
        }
        for _ in 0..size.zeppelins {
            sheet.add_kill(EnemyKind::Zeppelin, KillWeapon::Missile);
        }
        let total = score(&sheet, Level::of(&settings)).total;
        items.push(format!(
            "  {{ \"mode\": \"{}\", \"level\": \"{}\", \"score\": {total}, \"duration_s\": {duration_s}, \"breakdown\": {} }}",
            mode_key(mode),
            level_key(difficulty),
            sheet_json(&sheet)
        ));
    }
    format!("[\n{}\n]\n", items.join(",\n"))
}

/// Compares a generated leaderboard file with the repo; with
/// `UPDATE_LEADERBOARD=1` writes it instead.
fn check_generated(path: &str, generated: String) {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    if std::env::var("UPDATE_LEADERBOARD").is_ok_and(|v| v == "1") {
        std::fs::write(&file, &generated).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        stored == generated,
        "{path} is out of date with the game; run the tests with UPDATE_LEADERBOARD=1:\n{generated}"
    );
}

#[test]
fn the_workers_limits_match_the_missions() {
    check_generated("leaderboard/limits.json", limits_json());
}

#[test]
fn the_workers_score_examples_match_the_rules() {
    check_generated("leaderboard/test/vectors.json", vectors_json());
}

// --- In the game --------------------------------------------------------------

fn game() -> App {
    let mut app = headless_app(AppModePlugin(AppMode::Game));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    app.update();
    // Past the menu: fly a Normal Hunt.
    app.world_mut().resource_mut::<crate::menu::Menu>().open = false;
    *app.world_mut().resource_mut::<GameSettings>() = GameSettings::default();
    app.world_mut()
        .write_message(crate::game_state::RestartRound);
    for _ in 0..3 {
        app.update();
    }
    assert!(app.world().resource::<Round>().is_running());
    app
}

fn state(app: &App) -> &ScoreState {
    app.world().resource::<ScoreState>()
}

fn store(app: &App) -> &Store {
    app.world().resource::<Store>()
}

/// Shoots down every enemy in the air with `weapon`.
fn shoot_down_all(app: &mut App, weapon: Weapon) -> usize {
    let enemies: Vec<Entity> = app
        .world_mut()
        .query_filtered::<Entity, With<Enemy>>()
        .iter(app.world())
        .collect();
    for &target in &enemies {
        app.world_mut().write_message(Hit {
            projectile: Entity::PLACEHOLDER,
            target,
            point: Vec3::ZERO,
            team: Team::Player,
            weapon,
            damage: 10_000.0,
        });
    }
    app.update();
    app.update();
    enemies.len()
}

fn end_round(app: &mut App, outcome: Outcome) {
    app.world_mut().resource_mut::<Round>().phase = RoundPhase::Over(outcome);
    app.update();
    app.update();
}

#[test]
fn kills_count_live_with_the_weapon_that_scored_them() {
    let mut app = game();
    let shot = shoot_down_all(&mut app, Weapon::Gun);
    assert!(shot > 0);
    let sheet = state(&app).sheet.clone();
    assert_eq!(sheet.kill_count(), shot as u32);
    let gun_kills: u32 = sheet.kills.iter().map(|by_weapon| by_weapon[0]).sum();
    assert_eq!(gun_kills, shot as u32);
    let live: Vec<String> = app
        .world_mut()
        .query_filtered::<&Text, With<LiveScore>>()
        .iter(app.world())
        .map(|t| t.0.clone())
        .collect();
    assert_eq!(
        live,
        [format!("SCORE {}", grouped(sheet.kill_points()))],
        "Normal: factor 1"
    );
}

/// Types `text` as a browser would: one key event per character, with the
/// character as its text (so Shift, umlauts and any script come through).
fn type_text(app: &mut App, text: &str) {
    use bevy::input::{
        ButtonState,
        keyboard::{Key, NativeKeyCode},
    };
    for c in text.chars() {
        let key_code = match c.to_ascii_uppercase() {
            'R' => KeyCode::KeyR,
            'M' => KeyCode::KeyM,
            'N' => KeyCode::KeyN,
            'C' => KeyCode::KeyC,
            ' ' => KeyCode::Space,
            _ => KeyCode::Unidentified(NativeKeyCode::Unidentified),
        };
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Character(c.to_string().into()),
                state,
                text: state.is_pressed().then(|| c.to_string().into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }
}

fn over(app: &App) -> bool {
    matches!(app.world().resource::<Round>().phase, RoundPhase::Over(_))
}

#[test]
fn the_dialog_opens_by_itself_and_takes_every_key() {
    let mut app = game();
    shoot_down_all(&mut app, Weapon::Missile);
    end_round(&mut app, Outcome::ShotDown);
    assert!(state(&app).dialog, "opens at the round's end");
    assert_eq!(state(&app).field.text(), "", "no name stored yet");

    // Austin's name: the R does not restart, the M opens no menu, N does
    // not switch the sound.
    type_text(&mut app, "Trendgegner");
    assert!(over(&app));
    assert!(!app.world().resource::<crate::menu::Menu>().open);
    assert_eq!(state(&app).field.text(), "Trendgegner");
    tap(&mut app, KeyCode::KeyM);
    tap(&mut app, KeyCode::KeyN);
    assert!(over(&app));
    assert!(!app.world().resource::<crate::menu::Menu>().open);
    assert!(
        app.world()
            .resource::<crate::audio::SoundSettings>()
            .enabled
    );
    // No click gets through either.
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left)
    );
}

#[test]
fn enter_saves_any_name_locally_and_online() {
    let mut app = game();
    shoot_down_all(&mut app, Weapon::Missile);
    end_round(&mut app, Outcome::ShotDown);
    let result = state(&app).result.clone().expect("result");
    type_text(&mut app, "Jürgen \"Ace\" ✈");
    tap(&mut app, KeyCode::Enter);
    assert!(over(&app), "Enter saves, it does not restart");
    assert!(!state(&app).dialog);

    let submitted = state(&app).submitted.clone().expect("submitted");
    assert_eq!(submitted.name, "Jürgen \"Ace\" ✈");
    assert_eq!(submitted.local_place, Some(0));
    assert_eq!(
        store(&app).get(NAME_KEY).as_deref(),
        Some("Jürgen \"Ace\" ✈")
    );
    assert_eq!(
        store(&app).get(&top_key("hunt", "normal")),
        Some(format!("Jürgen \"Ace\" ✈\t{}", result.breakdown.total))
    );
    let outbox = split_outbox(&store(&app).get(OUTBOX_KEY).unwrap());
    assert_eq!(outbox.len(), 1);
    assert!(
        outbox[0].contains(r#""name":"Jürgen \"Ace\" ✈""#),
        "{}",
        outbox[0]
    );
    assert!(outbox[0].contains("\"mode\":\"hunt\",\"level\":\"normal\""));
    assert_eq!(submitted.online, Online::Waiting);

    // The page's answer arrives.
    app.world_mut().resource_mut::<Store>().set(
        ONLINE_KEY,
        &format!(
            "{}\n4\n1\nACE\t99999\nJürgen \"Ace\" ✈\t{}",
            submitted.id, result.breakdown.total
        ),
    );
    for _ in 0..12 {
        app.update();
    }
    assert!(matches!(
        state(&app).submitted.as_ref().unwrap().online,
        Online::Answered(OnlineAnswer::Ranked {
            all_time: Some(4),
            ..
        })
    ));

    // Now the usual keys again: Enter flies again.
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert!(app.world().resource::<Round>().is_running());
    assert!(state(&app).result.is_none());
}

#[test]
fn the_name_stops_at_eighteen_characters_and_needs_a_visible_one() {
    let mut app = game();
    app.world_mut()
        .resource_mut::<Store>()
        .set(NAME_KEY, "MAVERICK");
    end_round(&mut app, Outcome::ShotDown);
    assert_eq!(state(&app).field.text(), "MAVERICK", "the last name");
    for _ in 0..8 {
        tap(&mut app, KeyCode::Backspace);
    }
    type_text(&mut app, "   ");
    tap(&mut app, KeyCode::Enter);
    assert!(state(&app).submitted.is_none(), "only spaces: not saved");
    assert!(state(&app).dialog);
    type_text(&mut app, "abcdefghijklmnopqrstuvwxyz");
    assert_eq!(state(&app).field.text().chars().count(), 18);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(
        state(&app).submitted.as_ref().unwrap().name,
        "   abcdefghijklmno".trim()
    );
}

#[test]
fn escape_closes_without_saving_and_c_opens_it_again() {
    let mut app = game();
    app.world_mut()
        .resource_mut::<Store>()
        .set(NAME_KEY, "VIPER");
    shoot_down_all(&mut app, Weapon::Gun);
    end_round(&mut app, Outcome::Victory);
    tap(&mut app, KeyCode::Escape);
    assert!(!state(&app).dialog);
    assert!(over(&app), "Escape only closes the dialog");
    assert!(store(&app).get(&top_key("hunt", "normal")).is_none());

    tap(&mut app, KeyCode::KeyC);
    assert!(state(&app).dialog);
    assert_eq!(state(&app).field.text(), "VIPER");
    tap(&mut app, KeyCode::Escape);

    // Closed: R restarts as usual, and the score was never saved.
    tap(&mut app, KeyCode::KeyR);
    app.update();
    assert!(app.world().resource::<Round>().is_running());
    assert!(store(&app).get(&top_key("hunt", "normal")).is_none());
    assert!(store(&app).get(OUTBOX_KEY).is_none());
}

#[test]
fn custom_rounds_stay_local() {
    let mut app = game();
    app.world_mut()
        .resource_mut::<Store>()
        .set(NAME_KEY, "GOOSE");
    app.world_mut().resource_mut::<Mission>().difficulty = Difficulty::Custom(default());
    shoot_down_all(&mut app, Weapon::Gun);
    end_round(&mut app, Outcome::ShotDown);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(state(&app).submitted.as_ref().unwrap().online, Online::Off);
    assert!(store(&app).get(OUTBOX_KEY).is_none());
    assert!(store(&app).get(&top_key("hunt", "custom")).is_some());
}

#[test]
fn zeppelin_wrecks_score_outside_the_city_only() {
    let layout = crate::world::CityLayout::generate(&default());
    let urban = layout
        .blocks
        .iter()
        .find(|b| b.zone.is_urban())
        .expect("a built-up block");
    let centre = urban.rect.center();
    assert!(over_built_up_land(
        Some(&layout),
        Vec3::new(centre.x, 0.0, centre.y)
    ));
    assert!(!over_built_up_land(
        Some(&layout),
        Vec3::new(50_000.0, 0.0, 0.0)
    ));
}
