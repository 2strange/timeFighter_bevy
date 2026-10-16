//! What a round consists of: the game mode and its enemy waves, as data.
//!
//! Modes in build order: Hunt (fighters only), Protect (bombers attack the
//! city), Battle (bombers with fighter escort). Hunt is built; the others
//! add enemy kinds, roles and their own victory rule on the same model.

use bevy::prelude::*;

use crate::ai::{EnemyKind, PilotSkill};

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

/// The round to play: mode, waves and pacing.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Mission {
    pub mode: GameMode,
    pub waves: Vec<WaveSpec>,
    /// Seconds between the deciding moment (last kill, own crash) and the
    /// result screen, so the end can be seen.
    pub end_delay: f32,
}

impl Default for Mission {
    fn default() -> Self {
        Self::hunt()
    }
}

impl Mission {
    /// Mode 1: two flights of four Spitfires. The first, rookies, comes
    /// head-on from the city (the player starts at z = +800 m flying along
    /// −Z); the second, veterans, from the flank once the first is nearly
    /// gone. The fight gets harder as it goes on.
    pub fn hunt() -> Self {
        let spitfires = |count, lead: Vec3, heading: Vec3, skill| SpawnGroup {
            kind: EnemyKind::Spitfire,
            count,
            lead,
            heading,
            formation: Formation::Vic { spacing: 45.0 },
            skill,
        };
        Self {
            mode: GameMode::Hunt,
            waves: vec![
                WaveSpec {
                    trigger: WaveTrigger::AtStart,
                    groups: vec![spitfires(
                        4,
                        Vec3::new(0.0, 700.0, -2_800.0),
                        Vec3::Z,
                        PilotSkill::ROOKIE,
                    )],
                },
                WaveSpec {
                    trigger: WaveTrigger::WhenEnemiesAtMost(1),
                    groups: vec![spitfires(
                        4,
                        Vec3::new(-3_200.0, 900.0, -600.0),
                        Vec3::X,
                        PilotSkill::VETERAN,
                    )],
                },
            ],
            end_delay: 1.5,
        }
    }

    pub fn enemy_count(&self) -> u32 {
        self.waves.iter().map(WaveSpec::enemy_count).sum()
    }
}
