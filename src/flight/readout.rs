//! Minimal flight data readout for test flights. The real HUD comes later.

use bevy::prelude::*;

use super::{FlightConfig, FlightState, PlayerAircraft};

#[derive(Component)]
pub(super) struct FlightReadout;

pub(super) fn spawn_readout(mut commands: Commands) {
    commands.spawn((
        FlightReadout,
        Text::default(),
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
    ));
}

pub(super) fn update_readout(
    cfg: Res<FlightConfig>,
    player: Query<(&FlightState, &Transform), With<PlayerAircraft>>,
    mut readout: Query<&mut Text, With<FlightReadout>>,
) {
    let (Ok((state, transform)), Ok(mut text)) = (player.single(), readout.single_mut()) else {
        return;
    };
    let throttle = (state.speed - cfg.speed_min) / (cfg.speed_max - cfg.speed_min) * 100.0;
    text.0 = format!(
        "SPD {:>5.0} km/h  ({:>3.0} m/s)\nALT {:>5.0} m\nTHR {:>5.0} %\n\n\
         W/S pitch   A/D roll   Q/E or Left/Right yaw\nUp/Down throttle   R reset\nP/Esc pause",
        state.speed * 3.6,
        state.speed,
        transform.translation.y,
        throttle,
    );
}
