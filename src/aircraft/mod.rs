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

use crate::physics::{AircraftSize, Layer, aircraft_body_sized};

pub use showroom::{ShowroomPlugin, ShowroomView};

/// Normalised aircraft models available in `assets/aircraft/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AircraftModel {
    F14Tomcat,
    Mig29,
    Tu2,
    F4uCorsair,
    Spitfire,
}

impl AircraftModel {
    pub const ALL: [AircraftModel; 5] = [
        AircraftModel::F14Tomcat,
        AircraftModel::Mig29,
        AircraftModel::Tu2,
        AircraftModel::F4uCorsair,
        AircraftModel::Spitfire,
    ];

    /// File name in `assets/aircraft/`, without extension. Matches the
    /// `name` in `tools/aircraft.json`.
    pub fn file_stem(self) -> &'static str {
        match self {
            AircraftModel::F14Tomcat => "f14_tomcat",
            AircraftModel::Mig29 => "mig_29",
            AircraftModel::Tu2 => "tupolev_tu_2",
            AircraftModel::F4uCorsair => "f4u_corsair",
            AircraftModel::Spitfire => "spitfire",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AircraftModel::F14Tomcat => "F-14 Tomcat",
            AircraftModel::Mig29 => "MiG-29",
            AircraftModel::Tu2 => "Tu-2",
            AircraftModel::F4uCorsair => "F4U Corsair",
            AircraftModel::Spitfire => "Spitfire",
        }
    }

    /// Bounding box of the normalised model (checked against the file by
    /// a test); its collider is built from it.
    pub fn size(self) -> AircraftSize {
        let (min, max) = match self {
            AircraftModel::F14Tomcat => return AircraftSize::JET,
            AircraftModel::Mig29 => ([-5.82, -1.63, -8.65], [5.82, 2.36, 8.65]),
            AircraftModel::Tu2 => ([-9.40, -2.23, -6.90], [9.40, 1.72, 6.90]),
            AircraftModel::F4uCorsair => ([-5.35, -1.49, -5.10], [5.35, 2.31, 5.10]),
            AircraftModel::Spitfire => ([-4.91, -1.83, -4.56], [4.91, 1.62, 4.56]),
        };
        AircraftSize {
            min: Vec3::from_array(min),
            max: Vec3::from_array(max),
        }
    }

    /// Kinematic body and collider for this model on `layer`.
    pub fn body(self, layer: Layer) -> impl Bundle {
        aircraft_body_sized(layer, self.size())
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
