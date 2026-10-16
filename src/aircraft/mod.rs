//! Aircraft models: the catalogue of normalised glTF files and a showroom
//! for checking them visually.
//!
//! Every file in `assets/aircraft/` is normalised once by
//! `tools/normalize-aircraft` (1 unit = 1 m, +Y up, nose along −Z, origin on
//! the fuselage axis), so it is spawned without any correction transform.
//! Keep Bevy's glTF coordinate conversion off (the default): the files are
//! already in Bevy's convention.

mod showroom;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

pub use showroom::{ShowroomPlugin, ShowroomState};

/// Normalised aircraft models available in `assets/aircraft/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AircraftModel {
    F14Tomcat,
    Mig29,
    Tu2,
    F4uCorsair,
}

impl AircraftModel {
    pub const ALL: [AircraftModel; 4] = [
        AircraftModel::F14Tomcat,
        AircraftModel::Mig29,
        AircraftModel::Tu2,
        AircraftModel::F4uCorsair,
    ];

    /// File name in `assets/aircraft/`, without extension. Matches the
    /// `name` in `tools/aircraft.json`.
    pub fn file_stem(self) -> &'static str {
        match self {
            AircraftModel::F14Tomcat => "f14_tomcat",
            AircraftModel::Mig29 => "mig_29",
            AircraftModel::Tu2 => "tupolev_tu_2",
            AircraftModel::F4uCorsair => "f4u_corsair",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AircraftModel::F14Tomcat => "F-14 Tomcat",
            AircraftModel::Mig29 => "MiG-29",
            AircraftModel::Tu2 => "Tu-2",
            AircraftModel::F4uCorsair => "F4U Corsair",
        }
    }

    /// Asset path of the model's scene, relative to `assets/`.
    pub fn scene_path(self) -> String {
        format!("aircraft/{}.glb", self.file_stem())
    }

    /// Component that spawns the model below the entity it is added to.
    pub fn scene(self, assets: &AssetServer) -> WorldAssetRoot {
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(self.scene_path())))
    }
}
