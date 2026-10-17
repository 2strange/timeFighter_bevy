//! Phones and tablets: the touch controls and the gyroscope live in the
//! page (`web/mobile.js`, an HTML overlay), which writes its state as one
//! line to `<html data-tf-input="…">`. This module reads it every frame and
//! turns it into the game's own inputs:
//!
//! - buttons become key presses (gun = Space held, missile = Right Shift,
//!   next target = Tab, pause = P, fly again = R, menu = M), so combat,
//!   pause and round flow need nothing extra; taps travel as counters, so
//!   none is lost between two frames;
//! - tilt (or the virtual stick) and the thrust slider go into the jet's
//!   [`FlightInput`], after the keyboard;
//! - the name dialog takes the text of the page's own input field (the
//!   phone's on-screen keyboard), see [`crate::score`].
//!
//! The other way round it writes `data-tf-mode` (menu, play, paused, over,
//! dialog), so the overlay shows the controls that fit, and `data-tf-name`
//! (the name to prefill) when the dialog opens.

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use bevy::{input::InputSystems, prelude::*};

use crate::{
    flight::{FlightConfig, FlightInput, FlightState, FlightSystems, PitchInvert, PlayerAircraft},
    game_state::{Pause, Round, RoundPhase},
    score::{DialogInput, ScoreState},
};

/// Tilt (degrees from the recentred hold) below this does nothing ...
pub const TILT_DEADZONE: f32 = 3.0;
/// ... and from this on it is full deflection.
pub const TILT_FULL: f32 = 20.0;
/// The virtual stick's dead zone (share of its travel).
pub const STICK_DEADZONE: f32 = 0.08;
/// Speed difference (m/s) at which the thrust slider asks for full throttle.
pub const THRUST_GAIN: f32 = 12.0;

/// Where pitch and roll come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Steering {
    #[default]
    None,
    /// Degrees of tilt from the recentred hold: pitch > 0 = top edge away,
    /// roll > 0 = right side down.
    Gyro,
    /// The virtual stick, -1..1: pitch > 0 = pushed forward, roll > 0 =
    /// right.
    Stick,
}

/// The page's touch controls, as read from `data-tf-input`.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MobileInput {
    pub steering: Steering,
    pub pitch: f32,
    pub roll: f32,
    /// Thrust slider, 0 (slowest) to 1 (fastest); `None` without slider.
    pub thrust: Option<f32>,
    /// The gun button is held.
    pub gun: bool,
    /// Tap counters.
    pub missile: u32,
    pub next_target: u32,
    pub pause: u32,
    pub again: u32,
    pub menu: u32,
    /// The text in the page's name field while the dialog is open.
    pub name: Option<String>,
    pub name_ok: u32,
    pub name_cancel: u32,
}

impl MobileInput {
    /// Reads `key=value;…`; unknown keys and bad values are skipped.
    pub fn parse(line: &str) -> Self {
        let mut input = Self::default();
        for (key, value) in line.split(';').filter_map(|pair| pair.split_once('=')) {
            let number = || value.parse::<f32>().ok().filter(|v| v.is_finite());
            let count = || value.parse::<u32>().unwrap_or(0);
            match key {
                "src" => {
                    input.steering = match value {
                        "gyro" => Steering::Gyro,
                        "stick" => Steering::Stick,
                        _ => Steering::None,
                    }
                }
                "p" => input.pitch = number().unwrap_or(0.0),
                "r" => input.roll = number().unwrap_or(0.0),
                "t" => input.thrust = number().map(|t| t.clamp(0.0, 1.0)),
                "g" => input.gun = value == "1",
                "m" => input.missile = count(),
                "n" => input.next_target = count(),
                "x" => input.pause = count(),
                "a" => input.again = count(),
                "M" => input.menu = count(),
                "name" => input.name = Some(percent_decode(value)),
                "ok" => input.name_ok = count(),
                "esc" => input.name_cancel = count(),
                _ => {}
            }
        }
        input
    }

    /// Pitch and roll for the jet (keyboard convention: pitch > 0 = nose up,
    /// roll > 0 = left wing down), or `None` without steering.
    pub fn flight_axes(&self) -> Option<(f32, f32)> {
        let (forward, right) = match self.steering {
            Steering::None => return None,
            Steering::Gyro => (tilt_axis(self.pitch), tilt_axis(self.roll)),
            Steering::Stick => (stick_axis(self.pitch), stick_axis(self.roll)),
        };
        // Top edge away / stick forward: nose down. Right down: roll right.
        Some((-forward, -right))
    }
}

/// Degrees of tilt to -1..1: nothing within the dead zone, full from
/// [`TILT_FULL`], linear between.
pub fn tilt_axis(degrees: f32) -> f32 {
    let span = TILT_FULL - TILT_DEADZONE;
    let amount = ((degrees.abs() - TILT_DEADZONE) / span).clamp(0.0, 1.0);
    amount.copysign(degrees)
}

/// The stick's -1..1 with a small dead zone, rescaled to reach 1 again.
pub fn stick_axis(value: f32) -> f32 {
    let amount = ((value.abs() - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).clamp(0.0, 1.0);
    amount.copysign(value)
}

/// The thrust slider as a target speed: the throttle that gets there.
pub fn thrust_throttle(thrust: f32, speed: f32, cfg: &FlightConfig) -> f32 {
    let target = cfg.speed_min + thrust.clamp(0.0, 1.0) * (cfg.speed_max - cfg.speed_min);
    ((target - speed) / THRUST_GAIN).clamp(-1.0, 1.0)
}

/// Decodes `encodeURIComponent` text (UTF-8 percent escapes).
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the page should show, for `data-tf-mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageMode {
    Menu,
    Play,
    Paused,
    Over,
    Dialog,
}

impl PageMode {
    pub fn key(self) -> &'static str {
        match self {
            PageMode::Menu => "menu",
            PageMode::Play => "play",
            PageMode::Paused => "paused",
            PageMode::Over => "over",
            PageMode::Dialog => "dialog",
        }
    }
}

/// Keys the module holds down, and the tap counters already seen.
#[derive(Resource, Debug, Default)]
struct Pressed {
    held: HashSet<KeyCode>,
    /// Pressed for one frame by a tap; released in the next.
    tapped: Vec<KeyCode>,
    seen: Option<MobileInput>,
}

pub struct MobilePlugin;

impl Plugin for MobilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobileInput>()
            .init_resource::<Pressed>()
            .add_systems(
                PreUpdate,
                (read_page, press_keys)
                    .chain()
                    .after(InputSystems)
                    .before(DialogInput),
            )
            .add_systems(
                Update,
                steer
                    .after(FlightSystems::Input)
                    .before(FlightSystems::Simulate),
            )
            .add_systems(PostUpdate, write_page);
    }
}

/// The page's line, every frame (browser only; tests set the resource).
fn read_page(mut input: ResMut<MobileInput>) {
    #[cfg(target_arch = "wasm32")]
    {
        let line = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .and_then(|e| e.get_attribute("data-tf-input"));
        let parsed = line.map(|l| MobileInput::parse(&l)).unwrap_or_default();
        input.set_if_neq(parsed);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = &mut input;
    }
}

/// The overlay's buttons as key presses.
fn press_keys(
    input: Res<MobileInput>,
    keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mut pressed: ResMut<Pressed>,
) {
    let Some(mut keys) = keys else {
        return;
    };
    for key in std::mem::take(&mut pressed.tapped) {
        if !pressed.held.contains(&key) {
            keys.release(key);
        }
    }
    // The gun is held as long as its button.
    let gun = KeyCode::Space;
    if input.gun && pressed.held.insert(gun) {
        keys.press(gun);
    } else if !input.gun && pressed.held.remove(&gun) {
        keys.release(gun);
    }
    // A counter that went up is one tap.
    let seen = pressed.seen.clone().unwrap_or_else(|| input.clone());
    let taps = [
        (input.missile, seen.missile, KeyCode::ShiftRight),
        (input.next_target, seen.next_target, KeyCode::Tab),
        (input.pause, seen.pause, KeyCode::KeyP),
        (input.again, seen.again, KeyCode::KeyR),
        (input.menu, seen.menu, KeyCode::KeyM),
    ];
    for (now, before, key) in taps {
        if now > before {
            keys.press(key);
            pressed.tapped.push(key);
        }
    }
    pressed.seen = Some(input.clone());
}

/// Tilt or stick and the thrust slider into the player's jet, over what
/// the keyboard wrote.
fn steer(
    input: Res<MobileInput>,
    cfg: Res<FlightConfig>,
    invert: Option<Res<PitchInvert>>,
    mut player: Query<(&mut FlightInput, &FlightState), With<PlayerAircraft>>,
) {
    let axes = input.flight_axes();
    if axes.is_none() && input.thrust.is_none() {
        return;
    }
    for (mut flight, state) in &mut player {
        let mut wanted = *flight;
        if let Some((pitch, roll)) = axes {
            wanted.pitch = invert.as_deref().copied().unwrap_or_default().apply(pitch);
            wanted.roll = roll;
        }
        if let Some(thrust) = input.thrust {
            wanted.throttle = thrust_throttle(thrust, state.speed, &cfg);
        }
        flight.set_if_neq(wanted);
    }
}

/// What the overlay should show now.
pub fn page_mode(
    pause: Option<&Pause>,
    round: Option<&Round>,
    score: Option<&ScoreState>,
) -> PageMode {
    if score.is_some_and(|s| s.dialog && s.result.is_some()) && !pause.is_some_and(|p| p.menu) {
        return PageMode::Dialog;
    }
    if pause.is_some_and(|p| p.menu) {
        return PageMode::Menu;
    }
    if round.is_some_and(|r| matches!(r.phase, RoundPhase::Over(_))) {
        return PageMode::Over;
    }
    if pause.is_some_and(|p| p.player) {
        return PageMode::Paused;
    }
    PageMode::Play
}

/// `data-tf-mode` and, when the dialog opens, `data-tf-name`.
fn write_page(
    pause: Option<Res<Pause>>,
    round: Option<Res<Round>>,
    score: Option<Res<ScoreState>>,
    mut last: Local<Option<PageMode>>,
) {
    let mode = page_mode(pause.as_deref(), round.as_deref(), score.as_deref());
    if *last == Some(mode) {
        return;
    }
    *last = Some(mode);
    #[cfg(target_arch = "wasm32")]
    {
        let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        else {
            return;
        };
        if mode == PageMode::Dialog {
            let name = score.as_deref().map(|s| s.field.text()).unwrap_or_default();
            let _ = root.set_attribute("data-tf-name", &name);
        }
        let _ = root.set_attribute("data-tf-mode", mode.key());
    }
}
