//! What the player chose before a round: mode, difficulty, lighting.
//!
//! [`GameSettings`] is written by the start menu and read when a round
//! starts. Difficulties are a plain data table ([`Difficulty::params`]):
//! tuning means changing numbers, nothing else.

use bevy::prelude::*;

use super::GameMode;
use crate::{ai::EnemyKind, world::Lighting};

/// The player's choices. A round reads them when it starts, so changes
/// apply from the next round on.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct GameSettings {
    pub mode: GameMode,
    pub difficulty: Difficulty,
    pub lighting: Lighting,
}

/// Technical choices behind the modes, not offered in the menu (yet).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct MissionConfig {
    /// Which aircraft the enemy flies.
    pub set: EnemySet,
    /// The bomber type of Protect in the first set: `EnemyKind::Lancaster`
    /// (default) or `EnemyKind::B17`, both with their own model, size and
    /// gunners.
    pub bomber: EnemyKind,
}

impl Default for MissionConfig {
    fn default() -> Self {
        Self {
            set: EnemySet::First,
            bomber: EnemyKind::Lancaster,
        }
    }
}

impl MissionConfig {
    /// The configuration asked for in the page's query (`?set=second`) or,
    /// natively, on the command line (`--set=second`); defaults otherwise.
    pub fn from_query(query: &str) -> Self {
        let set = query
            .trim_start_matches('?')
            .split('&')
            .chain(query.split_whitespace())
            .filter_map(|part| part.trim_start_matches("--").strip_prefix("set="))
            .find_map(EnemySet::from_name)
            .unwrap_or_default();
        Self { set, ..default() }
    }

    /// From the page URL in the browser, from the command line otherwise.
    pub fn detect() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let query = web_sys::window()
                .and_then(|w| w.location().search().ok())
                .unwrap_or_default();
            Self::from_query(&query)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self::from_query(&std::env::args().skip(1).collect::<Vec<_>>().join(" "))
        }
    }
}

/// The enemy's aircraft.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnemySet {
    /// Spitfires, and Lancasters or B-17s.
    #[default]
    First,
    /// Mixed pairs of Bf 109 K-4 (leading, a little slower) and Me 309;
    /// bomber flights of a Ju 390 leading He 177s.
    Second,
}

impl EnemySet {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "first" => Some(EnemySet::First),
            "second" => Some(EnemySet::Second),
            _ => None,
        }
    }

    /// A fighter flight of `count`, in formation order.
    pub fn fighters(self, count: u32) -> Vec<EnemyKind> {
        (0..count)
            .map(|i| match self {
                EnemySet::First => EnemyKind::Spitfire,
                EnemySet::Second if i % 2 == 0 => EnemyKind::Bf109K4,
                EnemySet::Second => EnemyKind::Me309,
            })
            .collect()
    }

    /// A bomber flight of `count`, in formation order (`bomber` is the
    /// first set's type).
    pub fn bombers(self, count: u32, bomber: EnemyKind) -> Vec<EnemyKind> {
        (0..count)
            .map(|i| match self {
                EnemySet::First => bomber,
                EnemySet::Second if i == 0 => EnemyKind::Ju390,
                EnemySet::Second => EnemyKind::He177,
            })
            .collect()
    }
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

    /// The level's numbers for `mode`: the same table, with the mode's own
    /// missile count. Protect fights bombers that take two missiles each;
    /// Super Easy leaves a reserve, Easy has just enough, Normal and up
    /// leave about four bombers for the gun. A custom level keeps its own
    /// single missile count in every mode.
    pub fn params_for(&self, mode: GameMode) -> DifficultyParams {
        let params = self.params();
        let missiles = match (mode, self) {
            (_, Difficulty::Custom(_)) | (GameMode::Hunt, _) => params.missiles,
            (GameMode::Protect, Difficulty::SuperEasy) => 20,
            (GameMode::Protect, Difficulty::Easy) => 16,
            (GameMode::Protect, Difficulty::Normal) => 16,
            (GameMode::Protect, Difficulty::Hard) => 12,
            (GameMode::Protect, Difficulty::Insane) => 12,
            // Battle: bombers take two missiles, fighters one. Super Easy
            // and Easy have enough (14 / 20 for 10 / 20 needed), Normal and
            // up leave a share of the targets to the gun (24 of 30, 16 of
            // 36, 16 of 48).
            (GameMode::Battle, Difficulty::SuperEasy) => 14,
            (GameMode::Battle, Difficulty::Easy) => 20,
            (GameMode::Battle, Difficulty::Normal) => 24,
            (GameMode::Battle, Difficulty::Hard) => 16,
            (GameMode::Battle, Difficulty::Insane) => 16,
        };
        DifficultyParams { missiles, ..params }
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
                attackers: 1,
                lock_time: 0.6,
                bomber_waves: 1,
                wave_interval: 150.0,
                city_loss: 0.4,
                ..base
            },
            Difficulty::Easy => DifficultyParams {
                missiles: 10,
                enemies: 6,
                flight_size: 2,
                pilots: PilotMix::Rookies,
                enemy_speed: 0.9,
                enemy_accuracy: 0.7,
                attackers: 1,
                lock_time: 0.6,
                bomber_waves: 2,
                wave_interval: 130.0,
                city_loss: 0.35,
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
                attackers: 3,
                lock_time: 1.0,
                bomber_waves: 3,
                wave_interval: 90.0,
                ..base
            },
            Difficulty::Insane => DifficultyParams {
                missiles: 6,
                enemies: 12,
                flight_size: 4,
                pilots: PilotMix::Veterans,
                enemy_speed: 1.0,
                enemy_accuracy: 1.2,
                attackers: 4,
                lock_time: 1.0,
                bomber_waves: 4,
                wave_interval: 90.0,
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
            attackers: 2,
            lock_time: 0.8,
            bomber_waves: 3,
            bombers_per_wave: 4,
            wave_interval: 110.0,
            city_loss: 0.3,
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
            attackers: self.attackers.clamp(1, 12),
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
