//! Arcade flight: config, flight model, keyboard input and the player jet.
//!
//! [`FlightPlugin`] holds only the simulation and runs headless (tests use
//! `MinimalPlugins`). [`PlayerFlightPlugin`] adds keyboard input, the jet's
//! mesh and the on-screen readout and needs the full `DefaultPlugins`.

mod config;
mod input;
mod jet;
mod model;
mod readout;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

pub use config::FlightConfig;
pub use model::{FlightInput, FlightState, step};

/// Marks the aircraft flown by the player.
#[derive(Component, Debug, Default)]
pub struct PlayerAircraft;

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

/// Player jet with keyboard control, mesh and readout.
pub struct PlayerFlightPlugin;

impl Plugin for PlayerFlightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (jet::spawn_player_jet, readout::spawn_readout))
            .add_systems(
                Update,
                (
                    (input::keyboard_input, input::respawn).in_set(FlightSystems::Input),
                    readout::update_readout.after(FlightSystems::Simulate),
                    jet::fall_back_to_primitive_jet,
                ),
            );
    }
}

fn fly(
    cfg: Res<FlightConfig>,
    time: Res<Time>,
    mut aircraft: Query<(&FlightInput, &mut FlightState, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (input, mut state, mut transform) in &mut aircraft {
        step(&cfg, input, &mut state, &mut transform, dt);
    }
}
