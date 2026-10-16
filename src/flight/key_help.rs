//! Key help on F1. Hidden by default; speed and altitude are on the HUD.

use bevy::prelude::*;

const HELP: &str = "W/S pitch   A/D roll   Q/E or Left/Right yaw\n\
                    Up/Down throttle   R reset   P/Esc pause\n\
                    F1 hide this help";

#[derive(Component)]
pub(super) struct KeyHelp;

pub(super) fn spawn_key_help(mut commands: Commands) {
    commands.spawn((
        KeyHelp,
        Text::new(HELP),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        Visibility::Hidden,
    ));
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

    fn help_visibility(app: &mut App) -> Visibility {
        *app.world_mut()
            .query_filtered::<&Visibility, With<KeyHelp>>()
            .single(app.world())
            .unwrap()
    }

    #[test]
    fn f1_toggles_the_key_help_which_starts_hidden() {
        let mut app = headless_app(GamePlugin);
        app.update();
        assert_eq!(help_visibility(&mut app), Visibility::Hidden);
        tap(&mut app, KeyCode::F1);
        assert_eq!(help_visibility(&mut app), Visibility::Visible);
        tap(&mut app, KeyCode::F1);
        assert_eq!(help_visibility(&mut app), Visibility::Hidden);
    }
}
