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

/// Most zeppelins a Battle sends.
pub const MAX_ZEPPELINS: u32 = 3;

/// Most fighters a zeppelin keeps in the air at once.
pub const MAX_ZEPPELIN_FIGHTERS: u32 = 6;

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

    /// The level's numbers for `mode`: the same table, with the mode's own
    /// missile count. Protect fights bombers that take two missiles each;
    /// Super Easy leaves a reserve (24 for 16 needed), Easy has just
    /// enough, Normal and up leave about four bombers for the gun. A custom level keeps its own
    /// single missile count in every mode.
    pub fn params_for(&self, mode: GameMode) -> DifficultyParams {
        let params = self.params();
        let missiles = match (mode, self) {
            (_, Difficulty::Custom(_)) | (GameMode::Hunt, _) => params.missiles,
            (GameMode::Protect, Difficulty::SuperEasy) => 24,
            (GameMode::Protect, Difficulty::Easy) => 16,
            (GameMode::Protect, Difficulty::Normal) => 16,
            (GameMode::Protect, Difficulty::Hard) => 12,
            (GameMode::Protect, Difficulty::Insane) => 12,
            // Battle: bombers take two missiles, fighters one, a zeppelin
            // about nine. Super Easy has a reserve (24 for 20 needed), Easy
            // just enough (20), Normal stays doable (32 for 30 + 9), Hard and
            // Insane leave much to the gun (20 for 36 + 9; 36 for 48 + 3 × 9,
            // eight more for each further zeppelin).
            (GameMode::Battle, Difficulty::SuperEasy) => 24,
            (GameMode::Battle, Difficulty::Easy) => 20,
            (GameMode::Battle, Difficulty::Normal) => 32,
            (GameMode::Battle, Difficulty::Hard) => 20,
            (GameMode::Battle, Difficulty::Insane) => 36,
        };
        DifficultyParams { missiles, ..params }
    }

    /// The numbers behind the level. Change these to tune.
    pub fn params(&self) -> DifficultyParams {
        // Austin's line: easy levels bring more enemies but plenty of
        // ammunition; enemies fly slower than designed on all but Insane.
        let base = DifficultyParams::default();
        match *self {
            Difficulty::SuperEasy => DifficultyParams {
                missiles: 24,
                enemies: 6,
                flight_size: 2,
                pilots: PilotMix::Rookies,
                enemy_speed: 0.5,
                enemy_accuracy: 0.5,
                unlimited_gun: true,
                attackers: 1,
                lock_time: 0.6,
                bomber_waves: 2,
                wave_interval: 150.0,
                city_loss: 0.4,
                zeppelins: 0,
                gun_assist: true,
                ..base
            },
            Difficulty::Easy => DifficultyParams {
                missiles: 12,
                enemies: 6,
                flight_size: 2,
                pilots: PilotMix::Rookies,
                enemy_speed: 0.6,
                enemy_accuracy: 0.7,
                unlimited_gun: true,
                attackers: 1,
                lock_time: 0.6,
                bomber_waves: 2,
                wave_interval: 130.0,
                city_loss: 0.35,
                zeppelins: 0,
                gun_assist: true,
                ..base
            },
            Difficulty::Normal => base,
            Difficulty::Hard => DifficultyParams {
                missiles: 8,
                enemies: 12,
                flight_size: 4,
                pilots: PilotMix::RookiesThenVeterans,
                enemy_speed: 0.9,
                enemy_accuracy: 1.0,
                gun_ammo: 1_000,
                attackers: 3,
                lock_time: 1.0,
                bomber_waves: 3,
                wave_interval: 90.0,
                ..base
            },
            Difficulty::Insane => DifficultyParams {
                missiles: 8,
                enemies: 16,
                flight_size: 4,
                pilots: PilotMix::Veterans,
                enemy_speed: 1.0,
                enemy_accuracy: 1.2,
                gun_ammo: 500,
                attackers: 4,
                lock_time: 1.0,
                bomber_waves: 4,
                wave_interval: 90.0,
                zeppelins: 3,
                zeppelin_fighters: 6,
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
    /// The player's gun never runs dry ...
    pub unlimited_gun: bool,
    /// ... or holds this many rounds.
    pub gun_ammo: u32,
    /// At most this many enemies attack at once; the others wait.
    pub attackers: u32,
    /// Seconds a target must stay in the seeker cone to be locked.
    pub lock_time: f32,
    /// Protect mode: bomber flights in the round ...
    pub bomber_waves: u32,
    /// ... of this many bombers ...
    pub bombers_per_wave: u32,
    /// ... arriving this many seconds apart (or as soon as the sky is clear).
    pub wave_interval: f32,
    /// The round is lost once this share of the city centre is destroyed.
    pub city_loss: f32,
    /// Battle sends this many zeppelins (0–3), each alone in its own wave,
    /// between the bomber waves and from other sides.
    pub zeppelins: u32,
    /// A zeppelin that has spotted the player launches a fighter every
    /// 20 s, keeping at most this many (0–6) in the air.
    pub zeppelin_fighters: u32,
    /// The gun's lead circle shows where to aim.
    pub gun_assist: bool,
}

impl Default for DifficultyParams {
    /// Normal.
    fn default() -> Self {
        Self {
            missiles: 8,
            enemies: 10,
            flight_size: 2,
            pilots: PilotMix::Mixed,
            enemy_speed: 0.75,
            enemy_accuracy: 0.85,
            unlimited_gun: false,
            gun_ammo: 2_000,
            attackers: 2,
            lock_time: 0.8,
            bomber_waves: 3,
            bombers_per_wave: 4,
            wave_interval: 110.0,
            city_loss: 0.3,
            zeppelins: 1,
            zeppelin_fighters: 5,
            gun_assist: false,
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
            missiles: self.missiles.min(100),
            gun_ammo: self.gun_ammo.clamp(50, 9_999),
            enemies: self.enemies.clamp(1, 48),
            flight_size: self.flight_size.clamp(1, 8),
            enemy_speed: factor(self.enemy_speed, 0.3, 2.0),
            enemy_accuracy: factor(self.enemy_accuracy, 0.1, 3.0),
            attackers: self.attackers.clamp(1, 12),
            zeppelins: self.zeppelins.min(MAX_ZEPPELINS),
            zeppelin_fighters: self.zeppelin_fighters.min(MAX_ZEPPELIN_FIGHTERS),
            bomber_waves: self.bomber_waves.clamp(1, 12),
            bombers_per_wave: self.bombers_per_wave.clamp(1, 8),
            wave_interval: if self.wave_interval.is_finite() {
                self.wave_interval.clamp(30.0, 600.0)
            } else {
                110.0
            },
            city_loss: if self.city_loss.is_finite() {
                self.city_loss.clamp(0.05, 0.95)
            } else {
                0.3
            },
            lock_time: if self.lock_time.is_finite() {
                self.lock_time.clamp(0.2, 3.0)
            } else {
                1.0
            },
            ..self
        }
    }
}
