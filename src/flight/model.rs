//! The arcade flight model as a pure function, independent of the ECS.

use bevy::prelude::*;

use super::FlightConfig;

/// Pilot commands, each axis in `-1.0..=1.0`.
///
/// Signs follow the right-hand rule around the aircraft's local axes:
/// `pitch > 0` raises the nose, `yaw > 0` turns the nose left,
/// `roll > 0` lowers the left wing. `throttle > 0` speeds up.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightInput {
    pub pitch: f32,
    pub roll: f32,
    pub yaw: f32,
    pub throttle: f32,
}

/// Dynamic state of an aircraft besides its `Transform`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightState {
    /// Current roll rate (rad/s), sign as in [`FlightInput::roll`].
    pub roll_rate: f32,
    /// Current pitch rate (rad/s), sign as in [`FlightInput::pitch`].
    pub pitch_rate: f32,
    /// Current yaw rate (rad/s), sign as in [`FlightInput::yaw`].
    pub yaw_rate: f32,
    /// Airspeed along the nose (m/s).
    pub speed: f32,
    /// World-space velocity (m/s), written by [`step`]. Arcade model: always
    /// along the nose, so the jet goes exactly where it points.
    pub velocity: Vec3,
}

impl FlightState {
    pub fn new(cfg: &FlightConfig) -> Self {
        Self {
            speed: cfg.speed_initial,
            ..default()
        }
    }
}

/// Advances one aircraft by `dt` seconds.
pub fn step(
    cfg: &FlightConfig,
    input: &FlightInput,
    state: &mut FlightState,
    transform: &mut Transform,
    dt: f32,
) {
    // Clamp kept from the prototype (0.1 s): a hitch or a background tab
    // must not teleport the jet. Below 10 fps the game runs in slow motion.
    let dt = dt.clamp(0.0, cfg.max_dt);

    state.roll_rate = axis_rate(
        state.roll_rate,
        input.roll,
        cfg.max_roll_rate,
        cfg.roll_response,
        cfg.idle_damping_per_second,
        dt,
    );
    state.pitch_rate = axis_rate(
        state.pitch_rate,
        input.pitch,
        cfg.max_pitch_rate,
        cfg.pitch_response,
        cfg.idle_damping_per_second,
        dt,
    );
    state.yaw_rate = axis_rate(
        state.yaw_rate,
        input.yaw,
        cfg.max_yaw_rate,
        cfg.yaw_response,
        cfg.idle_damping_per_second,
        dt,
    );

    state.speed = (state.speed + input.throttle.clamp(-1.0, 1.0) * cfg.throttle_rate * dt)
        .clamp(cfg.speed_min, cfg.speed_max);

    // Rotate around the aircraft's own axes: yaw, then pitch, then roll.
    transform.rotation = (transform.rotation
        * Quat::from_rotation_y(state.yaw_rate * dt)
        * Quat::from_rotation_x(state.pitch_rate * dt)
        * Quat::from_rotation_z(state.roll_rate * dt))
    .normalize();

    state.velocity = transform.forward() * state.speed;
    transform.translation += state.velocity * dt;

    if transform.translation.y < cfg.min_altitude {
        transform.translation.y = cfg.min_altitude;
    }
}

/// Moves a rotation rate towards the stick's target rate, damps it when the
/// stick is centred and keeps it within the axis limit.
fn axis_rate(rate: f32, stick: f32, max_rate: f32, response: f32, damping: f32, dt: f32) -> f32 {
    let stick = stick.clamp(-1.0, 1.0);
    let target = stick * max_rate;
    // Exponential approach: the same result at any frame rate, never overshoots.
    let mut rate = rate + (target - rate) * (1.0 - (-response * dt).exp());
    if stick == 0.0 {
        rate *= damping.powf(dt);
    }
    rate.clamp(-max_rate, max_rate)
}
