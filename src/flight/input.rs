use bevy::prelude::*;

use super::{FlightConfig, FlightInput, FlightState, PlayerAircraft};

/// Keyboard layout of the earlier prototype:
/// W/S pitch (W = nose down), A/D roll, Q/E yaw, arrow up/down throttle.
/// Arrow left/right also yaw, so the right hand can steer while the left flies.
pub(super) fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut player: Query<&mut FlightInput, With<PlayerAircraft>>,
) {
    let axis = |positive: &[KeyCode], negative: &[KeyCode]| -> f32 {
        f32::from(keys.any_pressed(positive.iter().copied()))
            - f32::from(keys.any_pressed(negative.iter().copied()))
    };
    let input = FlightInput {
        pitch: axis(&[KeyCode::KeyS], &[KeyCode::KeyW]),
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

/// R puts the jet back to its spawn point.
pub(super) fn respawn(
    keys: Res<ButtonInput<KeyCode>>,
    cfg: Res<FlightConfig>,
    mut player: Query<(&mut FlightState, &mut Transform), With<PlayerAircraft>>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
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
