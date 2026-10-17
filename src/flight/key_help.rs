//! Key help on I (function keys need fn on a Mac). Shown for a few seconds
//! when a round starts, so new pilots find the weapon keys; otherwise hidden
//! until I is pressed. Speed and altitude are on the HUD.
//!
//! The help lists what plugins register with [`KeyHelpAppExt::add_key_help`],
//! so whoever adds a key documents it next to the code that reads it.

use bevy::prelude::*;

use crate::game_state::{Pause, RestartRound, Round, RoundPhase};

/// Key that shows and hides the help.
pub const HELP_KEY: KeyCode = KeyCode::KeyI;

/// How long the help stays up after a round starts (s of game time, so a
/// pause does not eat it).
pub const AUTO_SHOW_SECONDS: f32 = 8.0;

/// Where a line goes in the help; lower comes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyHelpSection {
    Flight,
    Weapons,
    Round,
    Game,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
struct KeyHelpLine {
    section: KeyHelpSection,
    keys: String,
    action: String,
}

/// All registered key help lines.
#[derive(Resource, Default, Debug)]
pub struct KeyHelpLines(Vec<KeyHelpLine>);

impl KeyHelpLines {
    /// The help text: by section, then in registration order.
    pub fn text(&self) -> String {
        let mut lines = self.0.clone();
        lines.sort_by_key(|line| line.section);
        lines
            .iter()
            .map(|line| format!("{}   {}", line.keys, line.action))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub trait KeyHelpAppExt {
    /// Adds a line to the key help, e.g.
    /// `app.add_key_help(KeyHelpSection::Weapons, "Space", "machine gun")`.
    fn add_key_help(&mut self, section: KeyHelpSection, keys: &str, action: &str) -> &mut Self;
}

impl KeyHelpAppExt for App {
    fn add_key_help(&mut self, section: KeyHelpSection, keys: &str, action: &str) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<KeyHelpLines>()
            .0
            .push(KeyHelpLine {
                section,
                keys: keys.to_owned(),
                action: action.to_owned(),
            });
        self
    }
}

/// The key help text (hidden on touch screens).
#[derive(Component)]
pub struct KeyHelp;

pub(super) fn spawn_key_help(mut commands: Commands, lines: Res<KeyHelpLines>) {
    commands.spawn((
        KeyHelp,
        Text::new(lines.text()),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        Visibility::Hidden,
    ));
}

/// Keeps the text current if lines are added later.
pub(super) fn update_key_help(lines: Res<KeyHelpLines>, mut help: Query<&mut Text, With<KeyHelp>>) {
    if !lines.is_changed() {
        return;
    }
    for mut text in &mut help {
        text.0 = lines.text();
    }
}

/// I toggles the help. A round start shows it for [`AUTO_SHOW_SECONDS`];
/// pressing I meanwhile takes over (and cancels the timer).
#[allow(clippy::too_many_arguments)]
pub(super) fn toggle_key_help(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    (round, pause): (Option<Res<Round>>, Option<Res<Pause>>),
    mut restarts: MessageReader<RestartRound>,
    mut was_running: Local<bool>,
    mut auto_hide_in: Local<Option<f32>>,
    mut help: Query<&mut Visibility, With<KeyHelp>>,
    mobile: Option<Res<crate::mobile::MobileInput>>,
) {
    // Touch screens have no keys to explain.
    if crate::mobile::touch(mobile.as_deref()) {
        *auto_hide_in = None;
        for mut visibility in &mut help {
            visibility.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    // Runs before the round systems: a restart shows up as a phase other
    // than Running for one frame, or as a pending restart request.
    if restarts.read().count() > 0 {
        *was_running = false;
    }
    // Behind the start menu a round only waits as a backdrop: it counts as
    // started once the menu is gone.
    if pause.is_some_and(|p| p.menu) {
        *was_running = false;
        *auto_hide_in = None;
        return;
    }
    let running = round.is_some_and(|r| r.phase == RoundPhase::Running);
    let round_started = running && !*was_running;
    *was_running = running;

    let show = if keys.just_pressed(HELP_KEY) {
        *auto_hide_in = None;
        help.iter().next().map(|v| *v == Visibility::Hidden)
    } else if round_started {
        *auto_hide_in = Some(AUTO_SHOW_SECONDS);
        Some(true)
    } else if let Some(left) = auto_hide_in.as_mut() {
        *left -= time.delta_secs();
        (*left <= 0.0).then(|| {
            *auto_hide_in = None;
            false
        })
    } else {
        None
    };

    if let Some(show) = show {
        for mut visibility in &mut help {
            *visibility = if show {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::{input::ButtonState, time::TimeUpdateStrategy};

    use super::*;
    use crate::{
        GamePlugin,
        flight::{FlightInput, PlayerAircraft},
        game_state::Pause,
        test_support::{headless_app, tap},
    };

    fn help(app: &mut App) -> (Visibility, String) {
        let (visibility, text) = app
            .world_mut()
            .query_filtered::<(&Visibility, &Text), With<KeyHelp>>()
            .single(app.world())
            .unwrap();
        (*visibility, text.0.clone())
    }

    /// The game with 0.1 s frames, so seconds of game time pass quickly.
    fn game() -> App {
        let mut app = headless_app(GamePlugin);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
        app
    }

    fn frames(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    #[test]
    fn help_shows_when_a_round_starts_and_hides_after_a_few_seconds() {
        let mut app = game();
        frames(&mut app, 3);
        assert_eq!(help(&mut app).0, Visibility::Visible);
        frames(&mut app, (AUTO_SHOW_SECONDS * 10.0) as usize - 10);
        assert_eq!(help(&mut app).0, Visibility::Visible, "hidden too early");
        frames(&mut app, 15);
        assert_eq!(help(&mut app).0, Visibility::Hidden);

        // The next round shows it again.
        app.world_mut().write_message(RestartRound);
        frames(&mut app, 3);
        assert_eq!(help(&mut app).0, Visibility::Visible);
    }

    #[test]
    fn i_toggles_the_help_and_overrides_the_timer() {
        let mut app = game();
        frames(&mut app, 3);
        tap(&mut app, HELP_KEY);
        assert_eq!(help(&mut app).0, Visibility::Hidden);
        tap(&mut app, HELP_KEY);
        assert_eq!(help(&mut app).0, Visibility::Visible);
        // Opened by hand: stays up past the auto-hide time.
        frames(&mut app, (AUTO_SHOW_SECONDS * 10.0) as usize + 10);
        assert_eq!(help(&mut app).0, Visibility::Visible);
        tap(&mut app, HELP_KEY);
        assert_eq!(help(&mut app).0, Visibility::Hidden);
    }

    #[test]
    fn i_is_not_used_by_anything_else() {
        let mut app = game();
        frames(&mut app, 3);
        let before = *app
            .world_mut()
            .query_filtered::<&FlightInput, With<PlayerAircraft>>()
            .single(app.world())
            .unwrap();
        crate::test_support::key(&mut app, HELP_KEY, ButtonState::Pressed);
        app.update();
        let during = *app
            .world_mut()
            .query_filtered::<&FlightInput, With<PlayerAircraft>>()
            .single(app.world())
            .unwrap();
        assert_eq!(before, during);
        assert!(!app.world().resource::<Pause>().is_paused());
    }

    #[test]
    fn help_lists_flight_weapons_round_and_game_keys_in_order() {
        let mut app = headless_app(GamePlugin);
        app.update();
        let text = help(&mut app).1;
        let position = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle:?} missing in:\n{text}"))
        };
        let order = [
            position("pitch"),
            position("machine gun"),
            position("missile"),
            position("fly again"),
            position("pause"),
            position("this help"),
        ];
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "wrong order:\n{text}"
        );
        assert!(
            !text.contains("reset"),
            "R no longer resets mid-flight:\n{text}"
        );
    }

    #[test]
    fn lines_added_after_startup_show_up() {
        let mut app = headless_app(GamePlugin);
        app.update();
        app.add_key_help(KeyHelpSection::Weapons, "B", "bomb");
        app.update();
        assert!(help(&mut app).1.contains("B   bomb"));
    }
}
