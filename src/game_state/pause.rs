//! Pausing: P or Esc toggles, losing window focus or visibility pauses.
//!
//! While paused, game time (`Time<Virtual>`) stands still, so flight, camera
//! and everything else driven by `Time` freezes. When the player paused, the
//! window also only redraws on input, so a laptop's CPU and GPU can idle.

use std::time::Duration;

use bevy::{
    prelude::*,
    window::{WindowFocused, WindowOccluded},
    winit::{UpdateMode, WinitSettings},
};

/// Why the game is paused. Game time runs only when nothing holds it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pause {
    /// Paused by the player (P / Esc) or because the window lost focus.
    /// Shows the pause screen and throttles rendering.
    pub player: bool,
    /// Paused by the aircraft showroom, which keeps rendering.
    pub showroom: bool,
}

impl Pause {
    pub fn is_paused(&self) -> bool {
        self.player || self.showroom
    }
}

/// Redraw interval while the player has paused and gives no input.
const IDLE_REDRAW: Duration = Duration::from_secs(1);

#[derive(Component)]
struct PauseScreen;

/// Window update settings to restore when the game resumes.
#[derive(Resource)]
struct RunningWinitSettings(WinitSettings);

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pause>()
            .add_systems(Startup, spawn_pause_screen)
            .add_systems(
                Update,
                (toggle_pause, pause_when_window_hidden, apply_pause).chain(),
            );
    }
}

fn toggle_pause(keys: Option<Res<ButtonInput<KeyCode>>>, mut pause: ResMut<Pause>) {
    if keys.is_some_and(|keys| keys.any_just_pressed([KeyCode::KeyP, KeyCode::Escape])) {
        pause.player = !pause.player;
    }
}

/// Switching tabs, minimising or clicking elsewhere pauses. Getting focus
/// back does not resume by itself: the player decides when to go on.
fn pause_when_window_hidden(
    mut focus: MessageReader<WindowFocused>,
    mut occlusion: MessageReader<WindowOccluded>,
    mut pause: ResMut<Pause>,
) {
    let lost_focus = focus.read().any(|event| !event.focused);
    let hidden = occlusion.read().any(|event| event.occluded);
    if lost_focus || hidden {
        pause.player = true;
    }
}

fn apply_pause(
    mut commands: Commands,
    pause: Res<Pause>,
    mut time: ResMut<Time<Virtual>>,
    winit: Option<ResMut<WinitSettings>>,
    running: Option<Res<RunningWinitSettings>>,
    mut screen: Query<&mut Visibility, With<PauseScreen>>,
) {
    if !pause.is_changed() {
        return;
    }

    if pause.is_paused() {
        time.pause();
    } else {
        // The first frame after resuming gets a normal delta: game time was
        // paused when this frame's time was taken, and from the next frame on
        // the window redraws continuously again.
        time.unpause();
    }

    if let Some(mut winit) = winit {
        match (pause.player, running) {
            (true, None) => {
                commands.insert_resource(RunningWinitSettings(winit.clone()));
                winit.focused_mode = UpdateMode::reactive_low_power(IDLE_REDRAW);
                winit.unfocused_mode = UpdateMode::reactive_low_power(IDLE_REDRAW);
            }
            (false, Some(running)) => {
                *winit = running.0.clone();
                commands.remove_resource::<RunningWinitSettings>();
            }
            _ => {}
        }
    }

    for mut visibility in &mut screen {
        *visibility = if pause.player {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn spawn_pause_screen(mut commands: Commands) {
    commands
        .spawn((
            PauseScreen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            Visibility::Hidden,
        ))
        .with_children(|screen| {
            screen.spawn((
                Text::new("PAUSED"),
                TextFont {
                    font_size: FontSize::Px(56.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            screen.spawn((
                Text::new("P or Esc to continue"),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
            ));
        });
}

#[cfg(test)]
mod tests {
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::flight::{FlightConfig, FlightInput, FlightPlugin, FlightState};

    const FRAME: Duration = Duration::from_nanos(16_666_667);

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, FlightPlugin, PausePlugin))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<WindowFocused>()
            .add_message::<WindowOccluded>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        app.update();
        let cfg = app.world().resource::<FlightConfig>().clone();
        let jet = app
            .world_mut()
            .spawn((
                FlightInput::default(),
                FlightState::new(&cfg),
                cfg.spawn_transform(),
            ))
            .id();
        (app, jet)
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    fn position(app: &App, jet: Entity) -> Vec3 {
        app.world().get::<Transform>(jet).unwrap().translation
    }

    #[test]
    fn p_and_escape_freeze_and_resume_the_simulation() {
        for key in [KeyCode::KeyP, KeyCode::Escape] {
            let (mut app, jet) = app();
            app.update();

            press(&mut app, key);
            assert!(app.world().resource::<Pause>().player);
            let frozen = position(&app, jet);
            for _ in 0..30 {
                app.update();
            }
            assert_eq!(
                position(&app, jet),
                frozen,
                "{key:?}: jet moved while paused"
            );

            press(&mut app, key);
            assert!(!app.world().resource::<Pause>().is_paused());
            app.update();
            assert_ne!(position(&app, jet), frozen, "{key:?}: jet did not resume");
        }
    }

    #[test]
    fn resuming_after_a_long_pause_has_no_time_jump() {
        let (mut app, jet) = app();
        app.update();
        let speed = app.world().get::<FlightState>(jet).unwrap().speed;
        let normal_step = speed * FRAME.as_secs_f32();

        press(&mut app, KeyCode::KeyP);
        // The window sleeps while paused: the next frame comes 5 s later.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(5)));
        let before = position(&app, jet);
        press(&mut app, KeyCode::KeyP);
        assert_eq!(
            position(&app, jet),
            before,
            "the long frame must not move the jet"
        );

        app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        for _ in 0..5 {
            let before = position(&app, jet);
            app.update();
            let step = position(&app, jet).distance(before);
            assert!(
                (step - normal_step).abs() < 0.01 * normal_step,
                "step {step} m"
            );
        }
    }

    #[test]
    fn losing_focus_or_visibility_pauses_and_stays_paused() {
        let (mut app, _) = app();
        let window = app.world_mut().spawn_empty().id();

        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.update();
        assert!(app.world().resource::<Pause>().player);

        // Focus coming back does not resume by itself.
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        app.update();
        assert!(app.world().resource::<Pause>().player);

        press(&mut app, KeyCode::KeyP);
        assert!(!app.world().resource::<Pause>().player);
        app.world_mut().write_message(WindowOccluded {
            window,
            occluded: true,
        });
        app.update();
        assert!(app.world().resource::<Pause>().player);
    }

    #[test]
    fn showroom_pause_freezes_without_player_pause() {
        let (mut app, jet) = app();
        app.update();
        app.world_mut().resource_mut::<Pause>().showroom = true;
        app.update();
        let frozen = position(&app, jet);
        app.update();
        assert_eq!(position(&app, jet), frozen);
        assert!(!app.world().resource::<Pause>().player);
    }
}
