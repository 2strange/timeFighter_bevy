use bevy::prelude::*;

use super::{FlightConfig, FlightInput, FlightState, PlayerAircraft};

/// "Invert pitch" from the menu: W (and tilting the phone away) pulls the
/// nose up instead of pushing it down.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PitchInvert(pub bool);

impl PitchInvert {
    /// `pitch` as the player wants it.
    pub fn apply(self, pitch: f32) -> f32 {
        if self.0 { -pitch } else { pitch }
    }
}

/// Keyboard layout of the earlier prototype:
/// W/S pitch (W = nose down), A/D roll, Q/E yaw, arrow up/down throttle.
/// Arrow left/right also yaw, so the right hand can steer while the left flies.
pub(super) fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    invert: Option<Res<PitchInvert>>,
    mut player: Query<&mut FlightInput, With<PlayerAircraft>>,
) {
    let invert = invert.map_or_else(PitchInvert::default, |i| *i);
    let axis = |positive: &[KeyCode], negative: &[KeyCode]| -> f32 {
        f32::from(keys.any_pressed(positive.iter().copied()))
            - f32::from(keys.any_pressed(negative.iter().copied()))
    };
    let input = FlightInput {
        pitch: invert.apply(axis(&[KeyCode::KeyS], &[KeyCode::KeyW])),
        roll: axis(&[KeyCode::KeyA], &[KeyCode::KeyD]),
        yaw: axis(
            &[KeyCode::KeyQ, KeyCode::ArrowLeft],
            &[KeyCode::KeyE, KeyCode::ArrowRight],
        ),
        throttle: axis(&[KeyCode::ArrowUp], &[KeyCode::ArrowDown]),
    };
    for mut current in &mut player {
        current.set_if_neq(input);
    }
}

/// R puts the jet back to its spawn point. Not during a combat round:
/// there R restarts the round from the result screen, and a free reset in
/// mid-fight would dodge every crash.
pub(super) fn respawn(
    keys: Res<ButtonInput<KeyCode>>,
    cfg: Res<FlightConfig>,
    round: Option<Res<crate::game_state::Round>>,
    mut player: Query<(&mut FlightState, &mut Transform), With<PlayerAircraft>>,
) {
    if round.is_some() || !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    for (mut state, mut transform) in &mut player {
        *state = FlightState::new(&cfg);
        *transform = cfg.spawn_transform();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Presses `keys` and returns the input the player jet receives.
    fn input_for(keys: &[KeyCode]) -> FlightInput {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, keyboard_input);
        let jet = app
            .world_mut()
            .spawn((PlayerAircraft, FlightInput::default()))
            .id();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for key in keys {
            input.press(*key);
        }
        app.update();
        *app.world().get::<FlightInput>(jet).unwrap()
    }

    #[test]
    fn inverted_pitch_turns_w_and_s_around() {
        assert_eq!(input_for(&[KeyCode::KeyW]).pitch, -1.0, "W: nose down");
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(PitchInvert(true))
            .add_systems(Update, keyboard_input);
        let jet = app
            .world_mut()
            .spawn((PlayerAircraft, FlightInput::default()))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        assert_eq!(app.world().get::<FlightInput>(jet).unwrap().pitch, 1.0);
    }

    #[test]
    fn arrow_keys_yaw_like_q_and_e() {
        assert_eq!(input_for(&[KeyCode::ArrowLeft]).yaw, 1.0);
        assert_eq!(input_for(&[KeyCode::ArrowRight]).yaw, -1.0);
        assert_eq!(input_for(&[KeyCode::KeyQ]).yaw, 1.0);
        assert_eq!(input_for(&[KeyCode::KeyE]).yaw, -1.0);
        // Both keys for the same direction do not add up.
        assert_eq!(input_for(&[KeyCode::KeyQ, KeyCode::ArrowLeft]).yaw, 1.0);
        assert_eq!(input_for(&[KeyCode::KeyQ, KeyCode::ArrowRight]).yaw, 0.0);
    }

    #[test]
    fn arrow_up_and_down_stay_throttle() {
        let up = input_for(&[KeyCode::ArrowUp]);
        assert_eq!((up.throttle, up.yaw), (1.0, 0.0));
        let down = input_for(&[KeyCode::ArrowDown]);
        assert_eq!((down.throttle, down.yaw), (-1.0, 0.0));
    }
}
