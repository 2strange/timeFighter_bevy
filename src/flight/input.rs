use bevy::prelude::*;

use super::{FlightConfig, FlightInput, FlightState, PlayerAircraft};

/// Keyboard layout of the earlier prototype:
/// W/S pitch (W = nose down), A/D roll, Q/E yaw, arrow up/down throttle.
pub(super) fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut player: Query<&mut FlightInput, With<PlayerAircraft>>,
) {
    let axis = |positive: KeyCode, negative: KeyCode| -> f32 {
        f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative))
    };
    let input = FlightInput {
        pitch: axis(KeyCode::KeyS, KeyCode::KeyW),
        roll: axis(KeyCode::KeyA, KeyCode::KeyD),
        yaw: axis(KeyCode::KeyQ, KeyCode::KeyE),
        throttle: axis(KeyCode::ArrowUp, KeyCode::ArrowDown),
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
