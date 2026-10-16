//! TimeFighter: arcade flight combat.
//!
//! Conventions: 1 unit = 1 metre, +Y up, aircraft noses point along −Z.

pub mod camera;
pub mod flight;
pub mod world;

use bevy::prelude::*;

/// Everything the game adds on top of Bevy's `DefaultPlugins`.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            flight::FlightPlugin,
            flight::PlayerFlightPlugin,
            camera::ChaseCameraPlugin,
            world::WorldPlugin,
        ));
    }
}
