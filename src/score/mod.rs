//! Points, the pilot name and the hall of fame.
//!
//! Kills count while the round runs (the gun pays half as much again as a
//! missile), the end bonuses (missiles left, city, victory, time) only
//! with a victory, and everything is multiplied by the level's factor
//! ([`rules`]). The HUD corner shows the points live; the result screen
//! gets a panel with the breakdown, the name field and the top 10.
//!
//! Scores go to a local top 10 per mode and level; except for Custom they
//! are also queued for the online list, which the page's script sends to
//! the leaderboard worker (see [`store`]).
//!
//! When the result screen shows, a name dialog opens over everything, like
//! the menu: it takes every key and click while open, so typing never
//! restarts the round or reaches the game. It types like a keyboard (any
//! printable character, the layout's own via Bevy's text events), up to
//! 18 characters, prefilled with the last name. Enter saves (local list,
//! and online unless Custom), Escape closes it without saving; then the
//! usual keys apply again, and C opens it once more until a score is
//! saved.

pub mod board;
pub mod rules;
pub mod store;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use bevy::{
    input::{
        InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};

use board::{
    Entry, LOCAL_TOP, NameField, format_list, insert, json_escape, parse_list, valid_name,
};
use rules::{Breakdown, KillWeapon, Level, ScoreSheet, grouped, level_key, mode_key};
use store::{NAME_KEY, ONLINE_KEY, OnlineAnswer, Store, parse_online, top_key};

use crate::{
    ai::{Enemy, EnemyKind},
    combat::{DestroyCause, Destroyed, GoingDown, MissileRack, Team},
    flight::PlayerAircraft,
    game_state::{GameSettings, Mission, Outcome, Pause, Round, RoundPhase, RoundSystems},
    mobile::MobileInput,
    world::{CityLayout, damage::CityHealth},
};

const HUD: Color = Color::srgba(0.72, 1.0, 0.78, 0.95);
const DIM: Color = Color::srgba(0.72, 1.0, 0.78, 0.55);
const AMBER: Color = Color::srgb(1.0, 0.75, 0.2);
const PANEL: Color = Color::srgba(0.03, 0.05, 0.07, 0.82);

/// The round being scored and what became of its score.
#[derive(Resource, Debug, Default)]
pub struct ScoreState {
    pub sheet: ScoreSheet,
    /// Kind of every enemy seen this round (they are gone when the
    /// destroyed message is read).
    kinds: HashMap<Entity, EnemyKind>,
    /// Zeppelins on their way down and their last position.
    wrecks: HashMap<Entity, Vec3>,
    /// Set when the result screen shows.
    pub result: Option<RoundResult>,
    pub field: NameField,
    /// The name dialog is open and takes every key and click.
    pub dialog: bool,
    pub submitted: Option<Submitted>,
    /// Numbers submissions, so a late answer is not shown for a new round.
    next_id: u64,
    poll: f32,
}

/// The finished round's score.
#[derive(Clone, Debug, PartialEq)]
pub struct RoundResult {
    pub settings: GameSettings,
    pub sheet: ScoreSheet,
    pub breakdown: Breakdown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Submitted {
    pub id: u64,
    pub name: String,
    /// Place on the local list (0 = first), if it made it.
    pub local_place: Option<usize>,
    pub online: Online,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Online {
    /// Custom levels, or no leaderboard configured: local only.
    Off,
    Waiting,
    Answered(OnlineAnswer),
}

/// The name dialog's key and click handling (PreUpdate). Systems that
/// read clicks or keys in PreUpdate go after it, so the open dialog
/// swallows them for them too.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DialogInput;

pub struct ScorePlugin;

impl Plugin for ScorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Store>()
            .init_resource::<ScoreState>()
            .add_systems(Startup, (load_font, spawn_ui).chain())
            // Before anything else reads the keys of the frame.
            .add_systems(
                PreUpdate,
                result_keys.in_set(DialogInput).after(InputSystems),
            )
            .add_systems(
                Update,
                (
                    start_round,
                    remember_enemies,
                    count_kills,
                    watch_wrecks,
                    finish_round,
                    poll_online,
                    update_live,
                    update_panel,
                    update_dialog,
                )
                    .chain()
                    .after(RoundSystems::Track),
            );
    }
}

/// A new round clears everything; a score not saved by then is not
/// saved. (A result while the round is no longer over also means a new
/// round, should the start have passed within one frame.)
fn start_round(round: Res<Round>, mut state: ResMut<ScoreState>) {
    let left_result = state.result.is_some() && !matches!(round.phase, RoundPhase::Over(_));
    if round.phase != RoundPhase::Starting && !left_result {
        return;
    }
    let next_id = state.next_id;
    *state = ScoreState {
        next_id,
        ..default()
    };
}

fn remember_enemies(enemies: Query<(Entity, &Enemy), Added<Enemy>>, mut state: ResMut<ScoreState>) {
    for (entity, enemy) in &enemies {
        state.kinds.insert(entity, enemy.kind);
    }
}

fn count_kills(
    round: Res<Round>,
    mut destroyed: MessageReader<Destroyed>,
    mut state: ResMut<ScoreState>,
) {
    let counting = matches!(round.phase, RoundPhase::Running | RoundPhase::Ending { .. });
    for event in destroyed.read() {
        if !counting || event.team != Team::Enemy {
            continue;
        }
        let Some(kind) = state.kinds.get(&event.entity).copied() else {
            continue;
        };
        let weapon = match event.cause {
            DestroyCause::Gun => KillWeapon::Gun,
            DestroyCause::Missile => KillWeapon::Missile,
            DestroyCause::Crash(_) => KillWeapon::Crash,
        };
        state.sheet.add_kill(kind, weapon);
    }
}

/// Follows zeppelins going down; when one is gone, it came down where it
/// was last seen: outside the built-up blocks, that is a bonus.
fn watch_wrecks(
    round: Res<Round>,
    wrecks: Query<(Entity, &Enemy, &Transform), With<GoingDown>>,
    layout: Option<Res<CityLayout>>,
    mut state: ResMut<ScoreState>,
) {
    if !matches!(round.phase, RoundPhase::Running | RoundPhase::Ending { .. }) {
        return;
    }
    let mut seen = Vec::new();
    for (entity, enemy, transform) in &wrecks {
        if enemy.kind == EnemyKind::Zeppelin {
            state.wrecks.insert(entity, transform.translation);
            seen.push(entity);
        }
    }
    let landed: Vec<(Entity, Vec3)> = state
        .wrecks
        .iter()
        .filter(|(entity, _)| !seen.contains(entity))
        .map(|(entity, at)| (*entity, *at))
        .collect();
    for (entity, at) in landed {
        state.wrecks.remove(&entity);
        if !over_built_up_land(layout.as_deref(), at) {
            state.sheet.zeppelins_outside += 1;
        }
    }
}

/// Whether `at` lies over a built-up block of the city.
pub fn over_built_up_land(layout: Option<&CityLayout>, at: Vec3) -> bool {
    layout.is_some_and(|layout| {
        layout
            .blocks
            .iter()
            .any(|block| block.zone.is_urban() && block.rect.contains(at.xz()))
    })
}

/// The result screen: the end of the sheet, the score, the stored name.
#[allow(clippy::too_many_arguments)]
fn finish_round(
    round: Res<Round>,
    mission: Res<Mission>,
    city: Option<Res<CityHealth>>,
    rack: Query<&MissileRack, With<PlayerAircraft>>,
    store: Res<Store>,
    mut state: ResMut<ScoreState>,
) {
    let RoundPhase::Over(outcome) = round.phase else {
        return;
    };
    if state.result.is_some() {
        return;
    }
    let settings = GameSettings {
        mode: mission.mode,
        difficulty: mission.difficulty,
        ..default()
    };
    let mut sheet = state.sheet.clone();
    sheet.victory = outcome == Outcome::Victory;
    sheet.duration_s = round.elapsed.max(0.0) as u32;
    sheet.missiles_left = rack.single().map_or(0, |r| r.count);
    sheet.city_percent = mission
        .city_loss
        .and(city.as_deref())
        .map(|health| (health.center_intact_ratio.clamp(0.0, 1.0) * 100.0).round() as u32);
    let breakdown = rules::score(&sheet, Level::of(&settings));
    // The name dialog opens by itself, with the last name.
    let saved = store.get(NAME_KEY).filter(|n| valid_name(n));
    state.dialog = true;
    state.field = NameField::with_text(&saved.unwrap_or_default());
    state.result = Some(RoundResult {
        settings,
        sheet,
        breakdown,
    });
}

/// Enters the score on the local list and, unless Custom, in the outbox.
/// Does nothing without a valid name.
fn submit(state: &mut ScoreState, store: &mut Store) {
    let (Some(result), Some(name)) = (state.result.clone(), state.field.name()) else {
        return;
    };
    store.set(NAME_KEY, &name);
    let mode = mode_key(result.settings.mode);
    let level = level_key(result.settings.difficulty);
    let key = top_key(mode, level);
    let mut list = parse_list(&store.get(&key).unwrap_or_default());
    let local_place = insert(
        &mut list,
        Entry {
            name: name.clone(),
            score: result.breakdown.total,
        },
        LOCAL_TOP,
    );
    store.set(&key, &format_list(&list));

    let id = state.next_id;
    state.next_id += 1;
    let online = if rules::online(&result.settings) {
        store.push_outbox(&format!(
            "{{\"id\":{id},\"name\":\"{}\",\"mode\":\"{mode}\",\"level\":\"{level}\",\"score\":{},\"duration_s\":{},\"version\":\"{}\",\"breakdown\":{}}}",
            json_escape(&name),
            result.breakdown.total,
            result.sheet.duration_s,
            env!("CARGO_PKG_VERSION"),
            rules::sheet_json(&result.sheet),
        ));
        Online::Waiting
    } else {
        Online::Off
    };
    state.dialog = false;
    state.submitted = Some(Submitted {
        id,
        name,
        local_place,
        online,
    });
}

/// Keys on the result screen, before the rest of the game sees them:
/// while the dialog is open it gets every key, and nothing else gets any
/// key or click.
#[allow(clippy::too_many_arguments)]
fn result_keys(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    mut typed: MessageReader<KeyboardInput>,
    pause: Option<Res<Pause>>,
    mut state: ResMut<ScoreState>,
    mut store: ResMut<Store>,
    mobile: Option<Res<MobileInput>>,
    mut seen: Local<Option<(u32, u32)>>,
) {
    // The phone's own field (on-screen keyboard): its text, and its Enter
    // and cancel taps as counters.
    let taps = mobile.as_ref().map(|m| (m.name_ok, m.name_cancel));
    let before = seen.unwrap_or(taps.unwrap_or_default());
    *seen = taps;
    let menu_open = pause.is_some_and(|p| p.menu);
    if state.result.is_none() || menu_open {
        typed.clear();
        return;
    }
    if state.dialog {
        if let Some(text) = mobile.as_ref().and_then(|m| m.name.as_deref())
            && state.field.text() != text
        {
            state.field = NameField::with_text(text);
        }
        if let Some((ok, cancel)) = taps {
            if ok > before.0 && state.field.name().is_some() {
                submit(&mut state, &mut store);
            } else if cancel > before.1 {
                state.dialog = false;
            }
        }
        for event in typed.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match event.key_code {
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    if state.field.name().is_some() {
                        submit(&mut state, &mut store);
                    }
                }
                KeyCode::Escape => state.dialog = false,
                KeyCode::Backspace => {
                    state.field.backspace();
                }
                KeyCode::Delete => {
                    state.field.delete();
                }
                KeyCode::ArrowLeft => state.field.left(),
                KeyCode::ArrowRight => state.field.right(),
                KeyCode::Home => state.field.home(),
                KeyCode::End => state.field.end(),
                _ => {
                    if let Some(text) = typed_text(event) {
                        state.field.type_text(&text);
                    }
                }
            }
        }
        // A dialog, like the menu: nothing else sees a key or a click.
        keys.reset_all();
        if let Some(mut mouse) = mouse {
            mouse.reset_all();
        }
        return;
    }
    typed.clear();
    if state.submitted.is_none() && keys.just_pressed(KeyCode::KeyC) {
        state.dialog = true;
        keys.clear_just_pressed(KeyCode::KeyC);
    }
}

/// What a key press types: the text the platform made of it (layout,
/// Shift, dead keys), else the logical key's character, else, where the
/// platform gives neither (tests), the key's label.
fn typed_text(event: &KeyboardInput) -> Option<String> {
    if let Some(text) = &event.text {
        return Some(text.to_string());
    }
    match &event.logical_key {
        Key::Character(text) => return Some(text.to_string()),
        Key::Space => return Some(" ".into()),
        _ => {}
    }
    let label = format!("{:?}", event.key_code);
    if let Some(letter) = label.strip_prefix("Key").filter(|l| l.len() == 1) {
        return Some(letter.into());
    }
    if let Some(digit) = label.strip_prefix("Digit").filter(|d| d.len() == 1) {
        return Some(digit.into());
    }
    (event.key_code == KeyCode::Space).then(|| " ".into())
}

/// Looks for the page's answer to the last submission, twice a second.
fn poll_online(time: Res<Time<Real>>, store: Res<Store>, mut state: ResMut<ScoreState>) {
    let Some(submitted) = &state.submitted else {
        return;
    };
    if submitted.online != Online::Waiting {
        return;
    }
    let id = submitted.id;
    state.poll -= time.delta_secs();
    if state.poll > 0.0 {
        return;
    }
    state.poll = 0.5;
    if let Some(answer) = store.get(ONLINE_KEY).and_then(|t| parse_online(&t, id))
        && let Some(submitted) = &mut state.submitted
    {
        submitted.online = Online::Answered(answer);
    }
}

#[derive(Component)]
struct LiveScore;

#[derive(Component)]
struct ResultPanel;

#[derive(Component)]
struct NameDialog;

#[derive(Component)]
struct NameText;

/// A font for names: the built-in one has only ASCII, names may use any
/// script (DejaVu Sans Mono, free licence, see `assets/fonts/`).
#[derive(Resource, Clone, Default)]
struct NameFont(Handle<Font>);

const NAME_FONT: &str = "fonts/DejaVuSansMono.ttf";

fn load_font(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    let handle = assets.map_or_else(Handle::default, |a| a.load(NAME_FONT));
    commands.insert_resource(NameFont(handle));
}

fn label(content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// Text in the name font.
fn name_label(font: &NameFont, content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: (&font.0).into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn spawn_ui(mut commands: Commands, font: Res<NameFont>) {
    commands.spawn((
        LiveScore,
        label("", 20.0, HUD),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            right: px(16),
            ..default()
        },
        Visibility::Hidden,
    ));
    commands.spawn((
        ResultPanel,
        Node {
            position_type: PositionType::Absolute,
            top: percent(12),
            right: px(24),
            width: px(330),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::all(px(14)),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(DIM),
        // Above the result screen.
        GlobalZIndex(6),
        Visibility::Hidden,
    ));
    // The name dialog: over everything, the rest dimmed, like the menu.
    commands
        .spawn((
            NameDialog,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(20),
            Visibility::Hidden,
        ))
        .with_child((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
                padding: UiRect::axes(px(28), px(20)),
                border: UiRect::all(px(1)),
                min_width: px(380),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(DIM),
            children![
                label("YOUR NAME FOR THE HALL OF FAME", 14.0, DIM),
                (
                    Node {
                        padding: UiRect::axes(px(12), px(6)),
                        border: UiRect::all(px(2)),
                        min_width: px(300),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BorderColor::all(AMBER),
                    children![(NameText, name_label(&font, "", 24.0, AMBER))],
                ),
                (
                    label("Enter: save    Esc: cancel", 14.0, DIM),
                    crate::mobile::KeyHint
                ),
            ],
        ));
}

/// Shows the dialog while it is open, the name with its cursor.
fn update_dialog(
    state: Res<ScoreState>,
    pause: Option<Res<Pause>>,
    mut dialog: Query<&mut Visibility, With<NameDialog>>,
    mut text: Query<&mut Text, With<NameText>>,
) {
    if !state.is_changed() && !pause.as_ref().is_some_and(|p| p.is_changed()) {
        return;
    }
    let open = state.dialog && state.result.is_some() && !pause.is_some_and(|p| p.menu);
    for mut visibility in &mut dialog {
        visibility.set_if_neq(if open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }
    let (before, after) = state.field.split();
    for mut text in &mut text {
        let content = format!("{before}|{after}");
        if text.0 != content {
            text.0 = content;
        }
    }
}

/// Points so far: the kills and zeppelin bonuses times the level factor.
fn update_live(
    round: Res<Round>,
    mission: Res<Mission>,
    state: Res<ScoreState>,
    mut live: Query<(&mut Text, &mut Visibility), With<LiveScore>>,
) {
    if !state.is_changed() && !round.is_changed() {
        return;
    }
    let show = matches!(round.phase, RoundPhase::Running | RoundPhase::Ending { .. });
    let factor = rules::level_factor(mission.difficulty);
    let points =
        state.sheet.kill_points() + state.sheet.zeppelins_outside * rules::ZEPPELIN_OUTSIDE;
    for (mut text, mut visibility) in &mut live {
        visibility.set_if_neq(if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
        let content = format!("SCORE {}", grouped(rules::apply_factor(points, factor)));
        if text.0 != content {
            text.0 = content;
        }
    }
}

/// Lines of the result panel: `(left, right, color)`; names may use any
/// script, so the panel uses the name font.
pub fn panel_lines(state: &ScoreState, store: &Store, touch: bool) -> Vec<(String, String, Color)> {
    let Some(result) = &state.result else {
        return Vec::new();
    };
    let b = &result.breakdown;
    let s = &result.sheet;
    let mut lines = vec![(
        "SCORE".to_owned(),
        format!(
            "{} {}",
            mode_key(result.settings.mode).to_uppercase(),
            result.settings.difficulty.display_name().to_uppercase()
        ),
        DIM,
    )];
    let mut line = |left: String, points: u32| lines.push((left, grouped(points), HUD));
    line(format!("Kills  {}", s.kill_count()), b.kills);
    if b.zeppelins_outside > 0 {
        line(
            format!("Zeppelins down outside  {}", s.zeppelins_outside),
            b.zeppelins_outside,
        );
    }
    if s.victory {
        line(format!("Missiles left  {}", s.missiles_left), b.missiles);
        if let Some(percent) = s.city_percent {
            line(format!("City  {percent} %"), b.city);
        }
        line("Victory".to_owned(), b.victory);
        line(format!("Under par  {} s", b.seconds_under_par), b.time);
    }
    lines.push(("Level".to_owned(), format!("x {}", b.factor), DIM));
    lines.push(("TOTAL".to_owned(), grouped(b.total), AMBER));
    lines.push((String::new(), String::new(), DIM));

    // Name and what happens next.
    lines.push((format!("Pilot  {}", state.field.text()), String::new(), HUD));
    // Short lines: the panel is narrow.
    let hints: Vec<String> = match (&state.submitted, state.dialog) {
        (None, true) => vec!["Enter your name".into()],
        (None, false) if touch => vec!["Not saved".into(), "Tap SAVE SCORE".into()],
        (None, false) => vec!["Not saved".into(), "C: save it after all".into()],
        (Some(submitted), _) => {
            let local = submitted
                .local_place
                .map_or("Not in the local top 10".to_owned(), |p| {
                    format!("Saved: local #{}", p + 1)
                });
            let online = match &submitted.online {
                Online::Off => "Custom: local list only".to_owned(),
                Online::Waiting => "Online: sending...".to_owned(),
                Online::Answered(OnlineAnswer::Ranked { all_time, week, .. }) => {
                    let rank = |r: &Option<u32>| r.map_or("-".to_owned(), |r| format!("#{r}"));
                    format!("Online {}, week {}", rank(all_time), rank(week))
                }
                Online::Answered(OnlineAnswer::Failed(why)) => format!("Online: {why}"),
            };
            vec![local, online]
        }
    };
    for hint in hints {
        lines.push((hint, String::new(), DIM));
    }
    lines.push((String::new(), String::new(), DIM));

    // The online top 10 once it is there, else the local one.
    let (title, list) = match state.submitted.as_ref().map(|s| &s.online) {
        Some(Online::Answered(OnlineAnswer::Ranked { top, .. })) => {
            ("ONLINE TOP 10", parse_list(top))
        }
        _ => {
            let key = top_key(
                mode_key(result.settings.mode),
                level_key(result.settings.difficulty),
            );
            (
                "LOCAL TOP 10",
                parse_list(&store.get(&key).unwrap_or_default()),
            )
        }
    };
    lines.push((title.to_owned(), String::new(), DIM));
    let mine = state.submitted.as_ref().map(|s| (s.name.clone(), b.total));
    let mut marked = false;
    for (i, entry) in list.iter().take(LOCAL_TOP).enumerate() {
        let own = !marked && mine.as_ref() == Some(&(entry.name.clone(), entry.score));
        marked |= own;
        lines.push((
            format!("{:>2}. {}", i + 1, entry.name),
            grouped(entry.score),
            if own { AMBER } else { HUD },
        ));
    }
    if list.is_empty() {
        lines.push(("  no scores yet".to_owned(), String::new(), DIM));
    }
    lines
}

fn update_panel(
    mut commands: Commands,
    state: Res<ScoreState>,
    store: Res<Store>,
    font: Res<NameFont>,
    pause: Option<Res<Pause>>,
    panel: Query<(Entity, &mut Visibility), With<ResultPanel>>,
    mobile: Option<Res<MobileInput>>,
) {
    let touch = crate::mobile::touch(mobile.as_deref());
    let menu_open = pause.as_ref().is_some_and(|p| p.menu);
    if !state.is_changed() && !pause.as_ref().is_some_and(|p| p.is_changed()) {
        return;
    }
    for (entity, mut visibility) in panel {
        let show = state.result.is_some() && !menu_open;
        visibility.set_if_neq(if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
        commands.entity(entity).despawn_related::<Children>();
        let lines = panel_lines(&state, &store, touch);
        commands.entity(entity).with_children(|panel| {
            for (left, right, color) in lines {
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        min_height: px(16),
                        ..default()
                    })
                    .with_children(|line| {
                        line.spawn(name_label(&font, left, 15.0, color));
                        line.spawn(name_label(&font, right, 15.0, color));
                    });
            }
        });
    }
}
