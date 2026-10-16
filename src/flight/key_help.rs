//! Key help on F1. Hidden by default; speed and altitude are on the HUD.
//!
//! The help lists what plugins register with [`KeyHelpAppExt::add_key_help`],
//! so whoever adds a key documents it next to the code that reads it.

use bevy::prelude::*;

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
    /// Adds a line to the F1 key help, e.g.
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

#[derive(Component)]
pub(super) struct KeyHelp;

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

pub(super) fn toggle_key_help(
    keys: Res<ButtonInput<KeyCode>>,
    mut help: Query<&mut Visibility, With<KeyHelp>>,
) {
    if !keys.just_pressed(KeyCode::F1) {
        return;
    }
    for mut visibility in &mut help {
        *visibility = match *visibility {
            Visibility::Hidden => Visibility::Visible,
            _ => Visibility::Hidden,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GamePlugin,
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

    #[test]
    fn f1_toggles_the_key_help_which_starts_hidden() {
        let mut app = headless_app(GamePlugin);
        app.update();
        assert_eq!(help(&mut app).0, Visibility::Hidden);
        tap(&mut app, KeyCode::F1);
        assert_eq!(help(&mut app).0, Visibility::Visible);
        tap(&mut app, KeyCode::F1);
        assert_eq!(help(&mut app).0, Visibility::Hidden);
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
