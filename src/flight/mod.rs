//! Arcade flight: config, flight model, keyboard input and the player jet.
//!
//! [`FlightPlugin`] holds only the simulation and runs headless (tests use
//! `MinimalPlugins`). [`PlayerFlightPlugin`] adds keyboard input, the jet's
//! model and the key help (I) and needs the full `DefaultPlugins`.

mod config;
mod input;
mod jet;
mod jet_fx;
mod key_help;
mod model;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

use crate::game_state::RoundSystems;

pub use config::{FlightConfig, PLAYER_SPAWN, SPAWN_AIM, spawn_pitch_towards};
pub use jet_fx::{JetFxConfig, JetFxPoints, flame_length, vortex_strength};
pub use key_help::{KeyHelpAppExt, KeyHelpLines, KeyHelpSection};
pub use model::{FlightInput, FlightState, step};

/// Marks the aircraft flown by the player.
#[derive(Component, Debug, Default)]
pub struct PlayerAircraft;

/// The aircraft is destroyed: the flight model no longer moves it, so the
/// wreck stays where it hit (no diving through the ground and flying on).
/// Combat sets it in the frame of the kill and removes it on a new round.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Wrecked;

/// Systems that move aircraft. Anything following an aircraft (camera, HUD)
/// should run after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlightSystems {
    /// Writes [`FlightInput`] from the player's devices.
    Input,
    /// Integrates the flight model.
    Simulate,
}

/// Headless flight simulation.
pub struct FlightPlugin;

impl Plugin for FlightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightConfig>()
            .configure_sets(
                Update,
                (FlightSystems::Input, FlightSystems::Simulate).chain(),
            )
            .add_systems(Update, fly.in_set(FlightSystems::Simulate));
    }
}

/// Player jet with keyboard control, model and key help.
pub struct PlayerFlightPlugin;

impl Plugin for PlayerFlightPlugin {
    fn build(&self, app: &mut App) {
        app.add_key_help(KeyHelpSection::Flight, "W / S", "pitch (W = nose down)")
            .add_key_help(KeyHelpSection::Flight, "A / D", "roll")
            .add_key_help(KeyHelpSection::Flight, "Q / E or Left / Right", "yaw")
            .add_key_help(KeyHelpSection::Flight, "Up / Down", "throttle")
            .add_key_help(KeyHelpSection::Help, "I", "show / hide this help")
            .add_plugins(jet_fx::JetFxPlugin)
            .add_systems(Startup, (jet::spawn_player_jet, key_help::spawn_key_help))
            .add_systems(
                Update,
                (
                    (input::keyboard_input, input::respawn).in_set(FlightSystems::Input),
                    (
                        key_help::toggle_key_help.before(RoundSystems::Start),
                        key_help::update_key_help,
                    ),
                    jet::fall_back_to_primitive_jet,
                ),
            );
    }
}

fn fly(
    cfg: Res<FlightConfig>,
    time: Res<Time>,
    mut aircraft: Query<(&FlightInput, &mut FlightState, &mut Transform), Without<Wrecked>>,
) {
    let dt = time.delta_secs();
    for (input, mut state, mut transform) in &mut aircraft {
        step(&cfg, input, &mut state, &mut transform, dt);
    }
}
