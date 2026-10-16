use std::f32::consts::PI;

use bevy::prelude::*;

use crate::aircraft::AircraftModel;

/// Tunables of the arcade flight model.
///
/// Units: metres, seconds, radians. The defaults reproduce the feel of the
/// earlier prototype; see the per-field notes where a value was adjusted.
#[derive(Resource, Clone, Debug)]
pub struct FlightConfig {
    /// Lowest airspeed the throttle allows (m/s). The jet never stalls.
    pub speed_min: f32,
    /// Highest airspeed the throttle allows (m/s).
    pub speed_max: f32,
    /// Airspeed right after spawning (m/s).
    pub speed_initial: f32,
    /// How fast the throttle keys change airspeed (m/s²).
    pub throttle_rate: f32,

    /// Maximum roll rate (rad/s).
    pub max_roll_rate: f32,
    /// Maximum pitch rate (rad/s).
    pub max_pitch_rate: f32,
    /// Maximum yaw rate (rad/s).
    pub max_yaw_rate: f32,

    /// How quickly the roll rate follows the stick (1/s, exponential approach).
    pub roll_response: f32,
    /// How quickly the pitch rate follows the stick (1/s).
    pub pitch_response: f32,
    /// How quickly the yaw rate follows the stick (1/s).
    pub yaw_response: f32,

    /// Fraction of an axis' rotation rate that survives one second without
    /// input on that axis. The prototype multiplied by 0.98 every frame, which
    /// depended on the frame rate; 0.98^60 keeps its 60 fps feel at any rate.
    pub idle_damping_per_second: f32,

    /// Longest time step the model integrates at once (s). Protects against
    /// huge jumps after a stall, e.g. when the browser tab was in background.
    pub max_dt: f32,

    /// Safety net (m): the jet's centre never goes below this height. It
    /// lies under the lowest ground (the river's water at −25 m), so the jet
    /// always touches a world collider first and the physics reports
    /// `PlayerCrashed`; the clamp only keeps it from falling out of the
    /// world until gameplay reacts.
    pub min_altitude: f32,

    /// Spawn position (m).
    pub spawn_position: Vec3,
    /// Spawn pitch (rad); negative = nose down.
    pub spawn_pitch: f32,

    /// Visual model of the player's jet: a normalised glTF model from
    /// `assets/aircraft/`, or `None` for the built-in primitive jet. The
    /// primitive jet is also the fallback when the model fails to load.
    pub player_model: Option<AircraftModel>,
}

/// Where the player starts: over the southern suburbs, 4 km south (+Z) of
/// the city centre at 400 m, flying north (−Z) towards downtown.
pub const PLAYER_SPAWN: Vec3 = Vec3::new(0.0, 400.0, 4_000.0);

/// The point the spawn heading looks at: the city centre, at about the
/// height of the downtown towers, so the skyline fills the view.
pub const SPAWN_AIM: Vec3 = Vec3::new(0.0, 100.0, 0.0);

/// Pitch (rad) that points a nose along −Z from `from` at `to`'s height.
pub fn spawn_pitch_towards(from: Vec3, to: Vec3) -> f32 {
    (to.y - from.y).atan2((from.z - to.z).max(1.0))
}

impl Default for FlightConfig {
    fn default() -> Self {
        Self {
            speed_min: 50.0,
            speed_max: 500.0,
            speed_initial: 50.0,
            throttle_rate: 95.0,

            max_roll_rate: PI * 0.8,
            max_pitch_rate: PI * 0.5,
            max_yaw_rate: PI * 0.4,

            roll_response: PI * 1.5,
            pitch_response: PI,
            yaw_response: PI,

            idle_damping_per_second: 0.98_f32.powi(60),

            max_dt: crate::MAX_STEP,
            min_altitude: -30.0,

            spawn_position: PLAYER_SPAWN,
            spawn_pitch: spawn_pitch_towards(PLAYER_SPAWN, SPAWN_AIM),

            player_model: Some(AircraftModel::F14Tomcat),
        }
    }
}

impl FlightConfig {
    /// Spawn transform: at `spawn_position`, nose along −Z, pitched by `spawn_pitch`.
    pub fn spawn_transform(&self) -> Transform {
        Transform::from_translation(self.spawn_position)
            .with_rotation(Quat::from_rotation_x(self.spawn_pitch))
    }
}
