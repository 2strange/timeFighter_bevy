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

pub use showroom::{ShowroomPage, ShowroomPlugin, ShowroomScene, ShowroomView, scenes};

/// A gun position on an aircraft (m, model frame: nose −Z, +Y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GunMount {
    pub name: &'static str,
    pub position: Vec3,
}

impl GunMount {
    const fn new(name: &'static str, x: f32, y: f32, z: f32) -> Self {
        Self {
            name,
            position: Vec3::new(x, y, z),
        }
    }
}

const B17_MOUNTS: [GunMount; 4] = [
    GunMount::new("front_gun", 0.00, -0.79, -10.94),
    GunMount::new("rear_gun", 0.01, -1.37, 10.93),
    GunMount::new("top_turret", 0.00, 1.65, -6.24),
    GunMount::new("ball_turret", 0.00, -1.97, -0.12),
];

const LANCASTER_MOUNTS: [GunMount; 3] = [
    GunMount::new("front_turret", 0.00, -0.06, -9.82),
    GunMount::new("rear_turret", 0.00, 0.47, 9.74),
    GunMount::new("top_turret", 0.00, 1.55, 2.92),
];

// Rough gun positions of the second set: the nose and tail ends and the
// top of the fuselage, measured by the normaliser (mounts.json); to be
// placed on real turrets once the set is chosen.
const HE177_MOUNTS: [GunMount; 3] = [
    GunMount::new("front_gun", 0.00, -0.18, -10.27),
    GunMount::new("rear_gun", 0.00, 0.91, 10.26),
    GunMount::new("top_turret", 0.00, 0.66, -1.89),
];

const ME264_MOUNTS: [GunMount; 3] = [
    GunMount::new("front_gun", 0.00, -0.03, -9.41),
    GunMount::new("rear_gun", 0.00, 1.08, 9.39),
    GunMount::new("top_turret", 0.00, 0.83, -1.55),
];

const JU390_MOUNTS: [GunMount; 3] = [
    GunMount::new("front_gun", 0.00, -0.28, -16.47),
    GunMount::new("rear_gun", 0.00, 1.51, 16.36),
    GunMount::new("top_turret", 0.00, 1.78, -1.11),
];

const JU287_MOUNTS: [GunMount; 2] = [
    GunMount::new("front_gun", 0.00, -0.31, -8.46),
    GunMount::new("rear_gun", 0.00, -0.97, 8.46),
];

/// Normalised aircraft models available in `assets/aircraft/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AircraftModel {
    F14Tomcat,
    Mig29,
    Tu2,
    F4uCorsair,
    /// The enemy fighter: "Low Poly Spitfire" (simplified to 5.5k triangles).
    Spitfire,
    /// The first Spitfire model, kept in the showroom for comparison.
    SpitfireClassic,
    B17,
    Lancaster,
    // A low-poly set by one author, for a possible second enemy side.
    Bf109K4,
    Me309,
    He177,
    Me264,
    Ju390,
    Ju287,
    /// An airship, 245 m long; showroom only for now.
    Zeppelin,
}

impl AircraftModel {
    pub const ALL: [AircraftModel; 15] = [
        AircraftModel::F14Tomcat,
        AircraftModel::Mig29,
        AircraftModel::Tu2,
        AircraftModel::F4uCorsair,
        AircraftModel::Spitfire,
        AircraftModel::SpitfireClassic,
        AircraftModel::B17,
        AircraftModel::Lancaster,
        AircraftModel::Bf109K4,
        AircraftModel::Me309,
        AircraftModel::He177,
        AircraftModel::Me264,
        AircraftModel::Ju390,
        AircraftModel::Ju287,
        AircraftModel::Zeppelin,
    ];

    /// File name in `assets/aircraft/`, without extension. Matches the
    /// `name` in `tools/aircraft.json`.
    pub fn file_stem(self) -> &'static str {
        match self {
            AircraftModel::F14Tomcat => "f14_tomcat",
            AircraftModel::Mig29 => "mig_29",
            AircraftModel::Tu2 => "tupolev_tu_2",
            AircraftModel::F4uCorsair => "f4u_corsair",
            AircraftModel::Spitfire => "spitfire_lp",
            AircraftModel::SpitfireClassic => "spitfire",
            AircraftModel::B17 => "b17",
            AircraftModel::Lancaster => "lancaster",
            AircraftModel::Bf109K4 => "bf109k4",
            AircraftModel::Me309 => "me309",
            AircraftModel::He177 => "he177",
            AircraftModel::Me264 => "me264",
            AircraftModel::Ju390 => "ju390",
            AircraftModel::Ju287 => "ju287",
            AircraftModel::Zeppelin => "zeppelin",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AircraftModel::F14Tomcat => "F-14 Tomcat",
            AircraftModel::Mig29 => "MiG-29",
            AircraftModel::Tu2 => "Tu-2",
            AircraftModel::F4uCorsair => "F4U Corsair",
            AircraftModel::Spitfire => "Spitfire",
            AircraftModel::SpitfireClassic => "Spitfire (first model)",
            AircraftModel::B17 => "B-17",
            AircraftModel::Lancaster => "Lancaster",
            AircraftModel::Bf109K4 => "Bf 109 K-4",
            AircraftModel::Me309 => "Me 309",
            AircraftModel::He177 => "He 177",
            AircraftModel::Me264 => "Me 264",
            AircraftModel::Ju390 => "Ju 390",
            AircraftModel::Ju287 => "Ju 287",
            AircraftModel::Zeppelin => "Zeppelin",
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
            AircraftModel::Spitfire => ([-5.09, -0.84, -4.56], [5.09, 1.64, 4.56]),
            AircraftModel::SpitfireClassic => ([-4.91, -1.83, -4.56], [4.91, 1.62, 4.56]),
            AircraftModel::B17 => ([-14.93, -2.38, -11.33], [15.31, 3.01, 11.33]),
            AircraftModel::Lancaster => ([-15.23, -3.145, -10.56], [15.24, 2.44, 10.56]),
            AircraftModel::Bf109K4 => ([-5.33, -0.86, -4.51], [5.33, 1.64, 4.51]),
            AircraftModel::Me309 => ([-5.33, -0.92, -4.73], [5.33, 1.75, 4.73]),
            AircraftModel::He177 => ([-15.59, -1.30, -11.00], [15.59, 4.05, 11.00]),
            AircraftModel::Me264 => ([-18.35, -1.36, -10.06], [18.35, 2.59, 10.06]),
            AircraftModel::Ju390 => ([-26.83, -1.92, -17.10], [26.83, 4.62, 17.10]),
            AircraftModel::Ju287 => ([-11.51, -2.76, -9.15], [11.51, 2.44, 9.15]),
            AircraftModel::Zeppelin => ([-23.89, -22.02, -122.50], [23.89, 23.01, 122.50]),
        };
        AircraftSize {
            min: Vec3::from_array(min),
            max: Vec3::from_array(max),
        }
    }

    /// Gun positions of bombers (m, model frame), measured by the
    /// normaliser in `assets/aircraft/mounts.json`; empty for fighters.
    /// Front and rear come first.
    pub fn gun_mounts(self) -> &'static [GunMount] {
        match self {
            AircraftModel::B17 => &B17_MOUNTS,
            AircraftModel::Lancaster => &LANCASTER_MOUNTS,
            AircraftModel::He177 => &HE177_MOUNTS,
            AircraftModel::Me264 => &ME264_MOUNTS,
            AircraftModel::Ju390 => &JU390_MOUNTS,
            AircraftModel::Ju287 => &JU287_MOUNTS,
            _ => &[],
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
