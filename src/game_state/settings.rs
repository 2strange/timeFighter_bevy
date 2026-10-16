//! What the player chose before a round: mode, difficulty, lighting.
//!
//! [`GameSettings`] is written by the start menu and read when a round
//! starts. Difficulties are a plain data table ([`Difficulty::params`]):
//! tuning means changing numbers, nothing else.

use bevy::prelude::*;

use super::GameMode;
use crate::world::Lighting;

/// The player's choices. A round reads them when it starts, so changes
/// apply from the next round on.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct GameSettings {
    pub mode: GameMode,
    pub difficulty: Difficulty,
    pub lighting: Lighting,
}

/// How hard a round is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Difficulty {
    SuperEasy,
    Easy,
    #[default]
    Normal,
    Hard,
    Insane,
    /// Every value chosen by the player.
    Custom(DifficultyParams),
}

impl Difficulty {
    /// The fixed levels, easiest first (for menus).
    pub const PRESETS: [Difficulty; 5] = [
        Difficulty::SuperEasy,
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
        Difficulty::Insane,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            Difficulty::SuperEasy => "Super Easy",
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
            Difficulty::Insane => "Insane",
            Difficulty::Custom(_) => "Custom",
        }
    }

    /// The numbers behind the level. Change these to tune.
    pub fn params(&self) -> DifficultyParams {
        let base = DifficultyParams::default();
        match *self {
            Difficulty::SuperEasy => DifficultyParams {
                missiles: 20,
                enemies: 4,
                flight_size: 2,
                pilots: PilotMix::Rookies,
                enemy_speed: 0.8,
                enemy_accuracy: 0.5,
                ..base
            },
            Difficulty::Easy => DifficultyParams {
                missiles: 10,
                enemies: 6,
                flight_size: 2,
                pilots: PilotMix::Rookies,
                enemy_speed: 0.9,
                enemy_accuracy: 0.7,
                ..base
            },
            Difficulty::Normal => base,
            Difficulty::Hard => DifficultyParams {
                missiles: 6,
                enemies: 8,
                flight_size: 4,
                pilots: PilotMix::RookiesThenVeterans,
                enemy_speed: 1.0,
                enemy_accuracy: 1.0,
                ..base
            },
            Difficulty::Insane => DifficultyParams {
                missiles: 6,
                enemies: 12,
                flight_size: 4,
                pilots: PilotMix::Veterans,
                enemy_speed: 1.0,
                enemy_accuracy: 1.2,
                ..base
            },
            Difficulty::Custom(params) => params,
        }
    }
}

/// Who flies the enemy aircraft.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PilotMix {
    Rookies,
    /// Rookies and veterans flight by flight.
    #[default]
    Mixed,
    /// Rookies in the first half of the flights, veterans after.
    RookiesThenVeterans,
    Veterans,
}

/// One difficulty as numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DifficultyParams {
    /// Missiles on the player's jet.
    pub missiles: u32,
    /// Enemies in the whole round.
    pub enemies: u32,
    /// Enemies per flight; flights arrive one after another.
    pub flight_size: u32,
    pub pilots: PilotMix,
    /// Factor on the enemies' speeds (1 = as designed).
    pub enemy_speed: f32,
    /// Factor on the enemies' gun accuracy (1 = as designed; the gun's
    /// scatter is divided by it).
    pub enemy_accuracy: f32,
    /// The player's gun never runs dry.
    pub unlimited_gun: bool,
}

impl Default for DifficultyParams {
    /// Normal.
    fn default() -> Self {
        Self {
            missiles: 8,
            enemies: 8,
            flight_size: 2,
            pilots: PilotMix::Mixed,
            enemy_speed: 1.0,
            enemy_accuracy: 0.85,
            unlimited_gun: false,
        }
    }
}

impl DifficultyParams {
    /// The same within sane limits (a custom level can hold anything).
    pub fn clamped(self) -> Self {
        // NaN or infinity (e.g. from a broken saved setting) means "as
        // designed"; `clamp` alone would keep NaN.
        let factor = |f: f32, lo: f32, hi: f32| if f.is_finite() { f.clamp(lo, hi) } else { 1.0 };
        Self {
            missiles: self.missiles.min(99),
            enemies: self.enemies.clamp(1, 48),
            flight_size: self.flight_size.clamp(1, 8),
            enemy_speed: factor(self.enemy_speed, 0.3, 2.0),
            enemy_accuracy: factor(self.enemy_accuracy, 0.1, 3.0),
            ..self
        }
    }
}
