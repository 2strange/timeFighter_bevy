//! What a round consists of: the game mode and its enemy waves, as data.
//!
//! Modes in build order: Hunt (fighters only), Protect (bombers attack the
//! city), Battle (bombers with fighter escort). Hunt is built; the others
//! add enemy kinds, roles and their own victory rule on the same model.

use bevy::prelude::*;

use super::{Difficulty, GameSettings, PilotMix};
use crate::ai::{EnemyKind, EnemyTuning, PilotSkill};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// Shoot down every fighter.
    #[default]
    Hunt,
}

impl GameMode {
    pub fn display_name(self) -> &'static str {
        match self {
            GameMode::Hunt => "HUNT",
        }
    }
}

/// When a wave enters the fight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WaveTrigger {
    /// When the round starts.
    AtStart,
    /// This many seconds into the round.
    AfterSeconds(f32),
    /// As soon as no more than this many enemies are left.
    WhenEnemiesAtMost(u32),
}

/// How a group's aircraft are placed around its lead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Formation {
    /// Lead in front, the others alternately left and right, staggered back.
    Vic { spacing: f32 },
    /// Side by side.
    LineAbreast { spacing: f32 },
}

impl Formation {
    /// Offset of aircraft `index` from the lead, in the group's frame
    /// (x right, z back).
    pub fn slot(self, index: u32) -> Vec3 {
        if index == 0 {
            return Vec3::ZERO;
        }
        let rank = index.div_ceil(2) as f32;
        let side = if index % 2 == 1 { -1.0 } else { 1.0 };
        match self {
            Formation::Vic { spacing } => Vec3::new(side * rank * spacing, 0.0, rank * spacing),
            Formation::LineAbreast { spacing } => Vec3::new(side * rank * spacing, 0.0, 0.0),
        }
    }
}

/// Enemies of one kind flying together.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnGroup {
    pub kind: EnemyKind,
    pub count: u32,
    /// Where the lead appears (m).
    pub lead: Vec3,
    /// Initial flight direction (horizontal is fine; normalised on spawn).
    pub heading: Vec3,
    pub formation: Formation,
    pub skill: PilotSkill,
    pub tuning: EnemyTuning,
}

impl SpawnGroup {
    /// Transforms of all aircraft, noses along `heading`.
    pub fn transforms(&self) -> impl Iterator<Item = Transform> + '_ {
        let heading = self.heading.normalize_or(Vec3::NEG_Z);
        let frame = Transform::from_translation(self.lead).looking_to(heading, Vec3::Y);
        (0..self.count).map(move |i| {
            let position = frame.transform_point(self.formation.slot(i));
            Transform::from_translation(position).with_rotation(frame.rotation)
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaveSpec {
    pub trigger: WaveTrigger,
    pub groups: Vec<SpawnGroup>,
}

impl WaveSpec {
    pub fn enemy_count(&self) -> u32 {
        self.groups.iter().map(|g| g.count).sum()
    }
}

/// The round to play: mode, difficulty, waves and pacing.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Mission {
    pub mode: GameMode,
    pub difficulty: Difficulty,
    pub waves: Vec<WaveSpec>,
    /// Seconds between the deciding moment (last kill, own crash) and the
    /// result screen, so the end can be seen.
    pub end_delay: f32,
}

impl Default for Mission {
    fn default() -> Self {
        Self::hunt(Difficulty::default())
    }
}

/// Where flights enter, in turn: lead position and heading. The player
/// starts at z = +800 m flying along −Z, so the first comes head-on from
/// the city, the next ones from the flanks and from deep behind it.
const ENTRY_POINTS: [(Vec3, Vec3); 6] = [
    (Vec3::new(0.0, 700.0, -2_800.0), Vec3::Z),
    (Vec3::new(-3_200.0, 900.0, -600.0), Vec3::X),
    (Vec3::new(3_200.0, 800.0, -1_500.0), Vec3::NEG_X),
    (Vec3::new(0.0, 1_100.0, -4_200.0), Vec3::Z),
    (
        Vec3::new(-2_400.0, 750.0, -3_200.0),
        Vec3::new(0.6, 0.0, 0.8),
    ),
    (
        Vec3::new(2_400.0, 1_000.0, -3_400.0),
        Vec3::new(-0.6, 0.0, 0.8),
    ),
];

impl Mission {
    /// Mode 1, Hunt: Spitfire flights arriving one after another, each as
    /// the previous one is (nearly) gone. Size, count, pilots and tuning
    /// come from the difficulty.
    pub fn hunt(difficulty: Difficulty) -> Self {
        let params = difficulty.params().clamped();
        let flights = params.enemies.div_ceil(params.flight_size);
        let tuning = EnemyTuning {
            speed: params.enemy_speed,
            accuracy: params.enemy_accuracy,
        };
        let waves = (0..flights)
            .map(|i| {
                let count = params
                    .flight_size
                    .min(params.enemies - i * params.flight_size);
                let (lead, heading) = ENTRY_POINTS[i as usize % ENTRY_POINTS.len()];
                let skill = match params.pilots {
                    PilotMix::Rookies => PilotSkill::ROOKIE,
                    PilotMix::Veterans => PilotSkill::VETERAN,
                    PilotMix::Mixed if i % 2 == 0 => PilotSkill::ROOKIE,
                    PilotMix::Mixed => PilotSkill::VETERAN,
                    PilotMix::RookiesThenVeterans if 2 * i < flights => PilotSkill::ROOKIE,
                    PilotMix::RookiesThenVeterans => PilotSkill::VETERAN,
                };
                WaveSpec {
                    trigger: if i == 0 {
                        WaveTrigger::AtStart
                    } else {
                        // Four-ship flights come when one is left, pairs
                        // when the previous pair is gone.
                        WaveTrigger::WhenEnemiesAtMost(params.flight_size / 4)
                    },
                    groups: vec![SpawnGroup {
                        kind: EnemyKind::Spitfire,
                        count,
                        lead,
                        heading,
                        formation: Formation::Vic { spacing: 45.0 },
                        skill,
                        tuning,
                    }],
                }
            })
            .collect();
        Self {
            mode: GameMode::Hunt,
            difficulty,
            waves,
            end_delay: 1.5,
        }
    }

    /// The mission for the player's settings.
    pub fn from_settings(settings: &GameSettings) -> Self {
        match settings.mode {
            GameMode::Hunt => Self::hunt(settings.difficulty),
        }
    }

    pub fn enemy_count(&self) -> u32 {
        self.waves.iter().map(WaveSpec::enemy_count).sum()
    }
}
