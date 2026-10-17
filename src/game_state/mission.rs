//! What a round consists of: the game mode and its enemy waves, as data.
//!
//! Modes in build order: Hunt (fighters only), Protect (bombers attack the
//! city), Battle (bombers with fighter escort). Hunt and Protect are
//! built; Battle combines both on the same model.

use bevy::prelude::*;

use super::{Difficulty, GameSettings, MissionConfig, PilotMix};
use crate::ai::{EnemyKind, EnemyTuning, PilotSkill};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// Shoot down every fighter.
    #[default]
    Hunt,
    /// Bomber flights attack the city centre; shoot them all down before
    /// too much of it is destroyed.
    Protect,
    /// Both: every bomber flight comes with a fighter escort.
    Battle,
}

impl GameMode {
    /// The modes that can be played (for menus).
    pub const ALL: [GameMode; 3] = [GameMode::Hunt, GameMode::Protect, GameMode::Battle];

    pub fn display_name(self) -> &'static str {
        match self {
            GameMode::Hunt => "HUNT",
            GameMode::Protect => "PROTECT",
            GameMode::Battle => "BATTLE",
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
    /// This many seconds into the round, or earlier once no enemy is left.
    AfterSecondsOrCleared(f32),
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

/// Enemies flying together, possibly of different kinds.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnGroup {
    /// The aircraft in formation order; the first leads.
    pub members: Vec<EnemyKind>,
    /// Where the lead appears (m).
    pub lead: Vec3,
    /// Initial flight direction (horizontal is fine; normalised on spawn).
    pub heading: Vec3,
    pub formation: Formation,
    pub skill: PilotSkill,
    pub tuning: EnemyTuning,
    /// Fighters escorting the bomber group at this index of the same wave:
    /// while waiting for an attack slot they circle the bombers instead of
    /// a fixed holding point.
    pub escorts: Option<usize>,
}

impl SpawnGroup {
    /// Transforms of all aircraft, noses along `heading`.
    pub fn transforms(&self) -> impl Iterator<Item = Transform> + '_ {
        let heading = self.heading.normalize_or(Vec3::NEG_Z);
        let frame = Transform::from_translation(self.lead).looking_to(heading, Vec3::Y);
        (0..self.count()).map(move |i| {
            let position = frame.transform_point(self.formation.slot(i));
            Transform::from_translation(position).with_rotation(frame.rotation)
        })
    }

    pub fn count(&self) -> u32 {
        self.members.len() as u32
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaveSpec {
    pub trigger: WaveTrigger,
    pub groups: Vec<SpawnGroup>,
}

impl WaveSpec {
    pub fn enemy_count(&self) -> u32 {
        self.groups.iter().map(SpawnGroup::count).sum()
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
    /// Protect: the round is lost once this share of the city centre is
    /// destroyed. `None` when the city does not count.
    pub city_loss: Option<f32>,
}

impl Default for Mission {
    fn default() -> Self {
        Self::hunt(Difficulty::default(), &MissionConfig::default())
    }
}

/// Where flights enter, in turn: lead position and heading. The player
/// starts at z = +800 m flying along −Z, so the first comes head-on from
/// the city, the next ones from the flanks and from deep behind it.
const ENTRY_POINTS: [(Vec3, Vec3); 6] = [
    (Vec3::new(0.0, 700.0, -1_500.0), Vec3::Z),
    (Vec3::new(-3_500.0, 900.0, 1_500.0), Vec3::X),
    (Vec3::new(3_500.0, 800.0, 500.0), Vec3::NEG_X),
    (Vec3::new(0.0, 1_100.0, -3_500.0), Vec3::Z),
    (
        Vec3::new(-2_500.0, 750.0, -2_000.0),
        Vec3::new(0.6, 0.0, 0.8),
    ),
    (
        Vec3::new(2_500.0, 1_000.0, -2_500.0),
        Vec3::new(-0.6, 0.0, 0.8),
    ),
];

/// Where bomber flights enter: far out, on a straight line to the city
/// centre, from the north and the flanks (the player starts 4 km south).
const BOMBER_ENTRY_POINTS: [(Vec3, Vec3); 5] = [
    (Vec3::new(0.0, 900.0, -7_000.0), Vec3::Z),
    (
        Vec3::new(-6_000.0, 950.0, -3_000.0),
        Vec3::new(0.9, 0.0, 0.45),
    ),
    (
        Vec3::new(6_000.0, 850.0, -3_500.0),
        Vec3::new(-0.86, 0.0, 0.5),
    ),
    (Vec3::new(-7_000.0, 1_000.0, 0.0), Vec3::X),
    (Vec3::new(7_000.0, 900.0, 0.0), Vec3::NEG_X),
];

impl Mission {
    /// Mode 1, Hunt: Spitfire flights arriving one after another, each as
    /// the previous one is (nearly) gone. Size, count, pilots and tuning
    /// come from the difficulty.
    pub fn hunt(difficulty: Difficulty, config: &MissionConfig) -> Self {
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
                        members: config.set.fighters(count),
                        lead,
                        heading,
                        formation: Formation::Vic { spacing: 45.0 },
                        skill,
                        tuning,
                        escorts: None,
                    }],
                }
            })
            .collect();
        Self {
            mode: GameMode::Hunt,
            difficulty,
            waves,
            end_delay: 1.5,
            city_loss: None,
        }
    }

    /// The mission for the player's settings.
    pub fn from_settings(settings: &GameSettings, config: &MissionConfig) -> Self {
        match settings.mode {
            GameMode::Hunt => Self::hunt(settings.difficulty, config),
            GameMode::Protect => Self::protect(settings.difficulty, config),
            GameMode::Battle => Self::battle(settings.difficulty, config),
        }
    }

    /// Mode 2, Protect: flights of four bombers, one after another (a fixed
    /// interval apart, or as soon as the sky is clear), each flying at the
    /// city centre from a different side and bombing it until shot down.
    pub fn protect(difficulty: Difficulty, config: &MissionConfig) -> Self {
        let params = difficulty.params().clamped();
        let tuning = EnemyTuning {
            speed: params.enemy_speed,
            accuracy: params.enemy_accuracy,
        };
        let waves = (0..params.bomber_waves)
            .map(|i| {
                let (lead, heading) = BOMBER_ENTRY_POINTS[i as usize % BOMBER_ENTRY_POINTS.len()];
                WaveSpec {
                    trigger: if i == 0 {
                        WaveTrigger::AtStart
                    } else {
                        WaveTrigger::AfterSecondsOrCleared(i as f32 * params.wave_interval)
                    },
                    groups: vec![SpawnGroup {
                        members: config.set.bombers(params.bombers_per_wave, config.bomber),
                        lead,
                        heading,
                        formation: Formation::Vic { spacing: 60.0 },
                        skill: PilotSkill::VETERAN,
                        tuning,
                        escorts: None,
                    }],
                }
            })
            .collect();
        Self {
            mode: GameMode::Protect,
            difficulty,
            waves,
            end_delay: 2.0,
            city_loss: Some(params.city_loss),
        }
    }

    /// Mode 3, Battle: Protect's bomber waves, each with an escort of
    /// `flight_size` fighters that fight the player as in Hunt (attack
    /// slots, sight). Won when everything is down; lost when the city
    /// centre falls or the player is shot down.
    pub fn battle(difficulty: Difficulty, config: &MissionConfig) -> Self {
        let params = difficulty.params().clamped();
        let mut mission = Self::protect(difficulty, config);
        let waves = mission.waves.len() as u32;
        for (i, wave) in mission.waves.iter_mut().enumerate() {
            let bombers = wave.groups[0].clone();
            let heading = bombers.heading.normalize_or(Vec3::NEG_Z);
            let skill = match params.pilots {
                PilotMix::Rookies => PilotSkill::ROOKIE,
                PilotMix::Veterans => PilotSkill::VETERAN,
                PilotMix::Mixed if i % 2 == 0 => PilotSkill::ROOKIE,
                PilotMix::Mixed => PilotSkill::VETERAN,
                PilotMix::RookiesThenVeterans if 2 * i < waves as usize => PilotSkill::ROOKIE,
                PilotMix::RookiesThenVeterans => PilotSkill::VETERAN,
            };
            // The escort trails the bombers, a little higher.
            wave.groups.push(SpawnGroup {
                members: config.set.fighters(params.flight_size),
                lead: bombers.lead - heading * ESCORT_TRAIL + Vec3::Y * ESCORT_ABOVE,
                heading,
                formation: Formation::Vic { spacing: 45.0 },
                skill,
                tuning: bombers.tuning,
                escorts: Some(0),
            });
        }
        // Normal and up: the zeppelin closes the battle, with a double
        // escort, on a straight course over the centre from the north.
        if params.zeppelin {
            let (lead, heading) = ZEPPELIN_ENTRY;
            let i = mission.waves.len();
            let tuning = mission.waves[0].groups[0].tuning;
            let escort_heading = heading.normalize_or(Vec3::NEG_Z);
            mission.waves.push(WaveSpec {
                trigger: WaveTrigger::AfterSecondsOrCleared(i as f32 * params.wave_interval),
                groups: vec![
                    SpawnGroup {
                        members: vec![EnemyKind::Zeppelin],
                        lead,
                        heading,
                        formation: Formation::Vic { spacing: 300.0 },
                        skill: PilotSkill::VETERAN,
                        tuning,
                        escorts: None,
                    },
                    SpawnGroup {
                        members: config.set.fighters(2 * params.flight_size),
                        lead: lead - escort_heading * ESCORT_TRAIL + Vec3::Y * ESCORT_ABOVE,
                        heading,
                        formation: Formation::Vic { spacing: 45.0 },
                        skill: PilotSkill::VETERAN,
                        tuning,
                        escorts: Some(0),
                    },
                ],
            });
        }
        mission.mode = GameMode::Battle;
        mission
    }

    pub fn enemy_count(&self) -> u32 {
        self.waves.iter().map(WaveSpec::enemy_count).sum()
    }
}

/// Where the zeppelin enters: north of the centre, close enough that at
/// 30 m/s it is over the city in under two minutes, heading south.
const ZEPPELIN_ENTRY: (Vec3, Vec3) = (Vec3::new(0.0, 700.0, -3_200.0), Vec3::Z);

/// Where an escort enters relative to its bombers: this far behind (m) ...
const ESCORT_TRAIL: f32 = 400.0;
/// ... and this much higher (m).
const ESCORT_ABOVE: f32 = 200.0;
