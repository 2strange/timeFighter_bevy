use std::f32::consts::PI;

use bevy::prelude::*;

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

    /// The jet is held at least this high above the ground plane (m).
    /// Placeholder until terrain collision and crashing exist.
    pub min_altitude: f32,

    /// Spawn position (m).
    pub spawn_position: Vec3,
    /// Spawn pitch (rad); negative = nose down.
    pub spawn_pitch: f32,
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

            max_dt: 0.1,
            min_altitude: 3.0,

            spawn_position: Vec3::new(0.0, 500.0, 800.0),
            spawn_pitch: (-15.0_f32).to_radians(),
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
