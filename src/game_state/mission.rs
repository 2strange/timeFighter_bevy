//! What a round consists of: the game mode and its enemy waves, as data.
//!
//! Modes in build order: Hunt (fighters only), Protect (bombers attack the
//! city), Battle (bombers with fighter escort). Hunt and Protect are
//! built; Battle combines both on the same model.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use super::{Difficulty, GameSettings, MAX_ZEPPELIN_FIGHTERS, PilotMix};
use crate::{
    ai::{CarrierSpec, EnemyKind, EnemyTuning, PilotSkill},
    combat::CombatRng,
    flight::PLAYER_SPAWN,
};

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

impl WaveTrigger {
    /// The time a wave is planned for (s); triggers on the enemy count
    /// sort as they are listed.
    fn time(self) -> f32 {
        match self {
            WaveTrigger::AtStart | WaveTrigger::WhenEnemiesAtMost(_) => 0.0,
            WaveTrigger::AfterSeconds(t) | WaveTrigger::AfterSecondsOrCleared(t) => t,
        }
    }
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
    /// The (single) member launches fighters as a carrier.
    pub carrier: Option<CarrierSpec>,
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
        Self::hunt(Difficulty::default())
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
    /// Mode 1, Hunt: fighter flights arriving one after another, each as
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
                        members: fighter_flight(count, params.pilots, i as usize, flights as usize),
                        lead,
                        heading,
                        formation: Formation::Vic { spacing: 45.0 },
                        skill,
                        tuning,
                        escorts: None,
                        carrier: None,
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
    pub fn from_settings(settings: &GameSettings) -> Self {
        match settings.mode {
            GameMode::Hunt => Self::hunt(settings.difficulty),
            GameMode::Protect => Self::protect(settings.difficulty),
            GameMode::Battle => Self::battle(settings.difficulty),
        }
    }

    /// Mode 2, Protect: flights of four bombers, one after another (a fixed
    /// interval apart, or as soon as the sky is clear), each flying at the
    /// city centre from a different side and bombing it until shot down.
    pub fn protect(difficulty: Difficulty) -> Self {
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
                        members: bomber_flight(params.bombers_per_wave),
                        lead,
                        heading,
                        formation: Formation::Vic { spacing: 60.0 },
                        skill: PilotSkill::VETERAN,
                        tuning,
                        escorts: None,
                        carrier: None,
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
    pub fn battle(difficulty: Difficulty) -> Self {
        let params = difficulty.params().clamped();
        let mut mission = Self::protect(difficulty);
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
                members: fighter_flight(params.flight_size, params.pilots, i, waves as usize),
                lead: bombers.lead - heading * ESCORT_TRAIL + Vec3::Y * ESCORT_ABOVE,
                heading,
                formation: Formation::Vic { spacing: 45.0 },
                skill,
                tuning: bombers.tuning,
                escorts: Some(0),
                carrier: None,
            });
        }
        // Zeppelins come alone, each between two bomber waves and spread
        // over the round. They bring no escort: once they spot the player
        // they launch their own fighters.
        let zeppelins = params.zeppelins as usize;
        let tuning = mission.waves[0].groups[0].tuning;
        for z in 0..zeppelins {
            let (lead, heading) = ZEPPELIN_ENTRIES[z % ZEPPELIN_ENTRIES.len()];
            let i = mission.waves.len();
            mission.waves.push(WaveSpec {
                trigger: WaveTrigger::AfterSecondsOrCleared(
                    zeppelin_slot(z, zeppelins, waves as usize) * params.wave_interval,
                ),
                groups: vec![SpawnGroup {
                    members: vec![EnemyKind::Zeppelin],
                    lead,
                    heading,
                    formation: Formation::Vic { spacing: 300.0 },
                    skill: PilotSkill::VETERAN,
                    tuning,
                    escorts: None,
                    carrier: Some(CarrierSpec {
                        fighters: fighter_flight(MAX_ZEPPELIN_FIGHTERS, params.pilots, i, i + 1),
                        max_alive: params.zeppelin_fighters,
                    }),
                }],
            });
        }
        // Waves enter in order: sort the zeppelins in between the bombers.
        mission
            .waves
            .sort_by(|a, b| a.trigger.time().total_cmp(&b.trigger.time()));
        mission.mode = GameMode::Battle;
        mission
    }

    pub fn enemy_count(&self) -> u32 {
        self.waves.iter().map(WaveSpec::enemy_count).sum()
    }

    /// The same waves entering at random places, different for every
    /// seed: fighter flights all round the city (never straight down the
    /// player's nose, never on top of the player), bombers far out on a
    /// course for the centre, zeppelins from another side than the bombers
    /// before them. Successive waves come from different sectors; escorts
    /// keep their place behind their bombers.
    pub fn scattered(mut self, seed: u64) -> Self {
        let mut rng = CombatRng::seeded(seed);
        let mut fighters = Sectors::new(&mut rng);
        let mut bombers = Sectors::new(&mut rng);
        let mut last_bombers: Option<f32> = None;
        let mut zeppelins: Vec<f32> = Vec::new();
        for wave in &mut self.waves {
            let before: Vec<(Vec3, Vec3)> =
                wave.groups.iter().map(|g| (g.lead, g.heading)).collect();
            for group in wave.groups.iter_mut().filter(|g| g.escorts.is_none()) {
                let kind = group.members[0];
                let (_, lead, heading) = if kind == EnemyKind::Zeppelin {
                    // Away from the bombers before it first, then from the
                    // other zeppelins.
                    let entry = place(&mut rng, &mut bombers, ZEPPELIN_SCATTER, |b| {
                        let near_bombers =
                            last_bombers.is_some_and(|l| bearing_gap(b, l) < ZEPPELIN_GAP);
                        let near_zeppelin = zeppelins
                            .iter()
                            .any(|&z| bearing_gap(b, z) < ZEPPELIN_SPREAD);
                        2 * u32::from(near_bombers) + u32::from(near_zeppelin)
                    });
                    zeppelins.push(entry.0);
                    entry
                } else if kind.is_bomber() {
                    let entry = place(&mut rng, &mut bombers, BOMBER_SCATTER, |_| 0);
                    last_bombers = Some(entry.0);
                    entry
                } else {
                    let (bearing, lead, _) = place(&mut rng, &mut fighters, FIGHTER_SCATTER, |_| 0);
                    (bearing, lead, fighter_heading(&mut rng, lead))
                };
                group.lead = lead;
                group.heading = heading;
            }
            // Escorts follow their bombers, in the same place behind them.
            for g in 0..wave.groups.len() {
                let Some(b) = wave.groups[g].escorts else {
                    continue;
                };
                let (old_lead, old_heading) = before[b];
                // About the vertical only, also for a full turn round.
                let (from, to) = (flat(old_heading), flat(wave.groups[b].heading));
                let turn = Quat::from_rotation_y(from.cross(to).y.atan2(from.dot(to)));
                let behind = wave.groups[g].lead - old_lead;
                wave.groups[g].lead = wave.groups[b].lead + turn * behind;
                wave.groups[g].heading = turn * wave.groups[g].heading;
            }
        }
        self
    }
}

/// How far out (m, from the city centre) and how high (m) a kind of
/// enemy enters when scattered.
#[derive(Clone, Copy)]
struct Scatter {
    distance: (f32, f32),
    altitude: (f32, f32),
}

const FIGHTER_SCATTER: Scatter = Scatter {
    distance: (1_800.0, 3_400.0),
    altitude: (600.0, 1_200.0),
};

const BOMBER_SCATTER: Scatter = Scatter {
    distance: (6_000.0, 7_500.0),
    altitude: (850.0, 1_050.0),
};

/// Zeppelins: at 30 m/s over the city in under two minutes.
const ZEPPELIN_SCATTER: Scatter = Scatter {
    distance: (3_000.0, 3_500.0),
    altitude: (650.0, 750.0),
};

/// Nobody enters closer than this to the player's start (m) ...
pub const MIN_PLAYER_DISTANCE: f32 = 2_000.0;
/// ... or ahead of it closer than this to its starting course (m).
pub const MIN_AXIS_OFFSET: f32 = 800.0;
/// Fighters fly for a point this close to the centre (m) ...
const FIGHTER_AIM: f32 = 1_500.0;
/// ... but not within this angle of straight at the player's start (rad).
pub const MIN_HEAD_ON_ANGLE: f32 = 35.0 * PI / 180.0;
/// A zeppelin enters at least this far round from the last bombers (rad)
/// ...
pub const ZEPPELIN_GAP: f32 = 100.0 * PI / 180.0;
/// ... and, where that leaves room, this far from other zeppelins (rad).
const ZEPPELIN_SPREAD: f32 = 50.0 * PI / 180.0;
/// Sectors round the city; successive waves draw different ones.
pub const SECTORS: usize = 8;
/// Tries to find an entry that keeps all the rules; then the best goes.
const PLACE_TRIES: usize = 32;

/// Compass bearings in turn from shuffled sectors, jittered within them;
/// never the same sector twice in a row.
struct Sectors {
    order: Vec<usize>,
    last: Option<usize>,
    /// The sector of the last entry placed.
    placed: Option<usize>,
}

impl Sectors {
    fn new(rng: &mut CombatRng) -> Self {
        let mut sectors = Self {
            order: Vec::new(),
            last: None,
            placed: None,
        };
        sectors.shuffle(rng);
        sectors
    }

    fn shuffle(&mut self, rng: &mut CombatRng) {
        self.order = (0..SECTORS).collect();
        for i in (1..SECTORS).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            self.order.swap(i, j);
        }
        // Drawn from the back: the next one differs from the last one.
        if self.order.last() == self.last.as_ref() {
            self.order.swap(0, SECTORS - 1);
        }
    }

    fn next(&mut self, rng: &mut CombatRng) -> f32 {
        let Some(sector) = self.order.pop() else {
            self.shuffle(rng);
            return self.next(rng);
        };
        self.last = Some(sector);
        let width = TAU / SECTORS as f32;
        (sector as f32 + rng.range(0.1, 0.9)) * width
    }

    fn sector_of(bearing: f32) -> usize {
        (bearing.rem_euclid(TAU) / (TAU / SECTORS as f32)) as usize % SECTORS
    }
}

/// A random entry `(bearing, lead, heading for the centre)` for `scatter`
/// that keeps clear of the player's start, comes from another sector than
/// the last one placed, and has the lowest `penalty` found (0: fine).
fn place(
    rng: &mut CombatRng,
    sectors: &mut Sectors,
    scatter: Scatter,
    penalty: impl Fn(f32) -> u32,
) -> (f32, Vec3, Vec3) {
    let mut best: Option<(u32, (f32, Vec3, Vec3))> = None;
    for _ in 0..PLACE_TRIES {
        let bearing = sectors.next(rng);
        let lead = on_bearing(bearing) * rng.range(scatter.distance.0, scatter.distance.1)
            + Vec3::Y * rng.range(scatter.altitude.0, scatter.altitude.1);
        let entry = (bearing, lead, flat(-lead));
        // Clear of the player always; the rest as far as possible.
        let same_side = sectors.placed == Some(Sectors::sector_of(bearing));
        let score = if clear_of_player(lead) {
            2 * penalty(bearing) + u32::from(same_side)
        } else {
            u32::MAX
        };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, entry));
        }
        if score == 0 {
            break;
        }
    }
    let (_, entry) = best.expect("at least one try");
    sectors.placed = Some(Sectors::sector_of(entry.0));
    entry
}

/// Far enough from the player's start, and not ahead on its course (the
/// player starts flying north, along −Z).
pub fn clear_of_player(lead: Vec3) -> bool {
    let distance = Vec2::new(lead.x - PLAYER_SPAWN.x, lead.z - PLAYER_SPAWN.z).length();
    let ahead = lead.z < PLAYER_SPAWN.z;
    let off_course = (lead.x - PLAYER_SPAWN.x).abs() >= MIN_AXIS_OFFSET;
    distance >= MIN_PLAYER_DISTANCE && (off_course || !ahead)
}

/// A fighter flight's course: for a random point near the centre, turned
/// off if that would be head-on at the player's start.
fn fighter_heading(rng: &mut CombatRng, lead: Vec3) -> Vec3 {
    let angle = rng.range(0.0, TAU);
    let aim = Vec3::new(angle.sin(), 0.0, angle.cos()) * FIGHTER_AIM * rng.unit().sqrt();
    let heading = flat(aim - lead);
    let to_player = flat(PLAYER_SPAWN - lead);
    if heading.angle_between(to_player) >= MIN_HEAD_ON_ANGLE {
        return heading;
    }
    // Turn away from the player, to the side the course already leans.
    let side = if heading.cross(to_player).y > 0.0 {
        -1.0
    } else {
        1.0
    };
    Quat::from_rotation_y(side * 2.0 * MIN_HEAD_ON_ANGLE) * heading
}

/// Horizontal unit vector towards compass `bearing` (0 north = −Z,
/// clockwise seen from above: π/2 east = +X).
fn on_bearing(bearing: f32) -> Vec3 {
    Vec3::new(bearing.sin(), 0.0, -bearing.cos())
}

/// Angle between two bearings (rad, 0..=π).
pub fn bearing_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d)
}

/// Bearing of a point seen from the centre, as [`on_bearing`] counts.
pub fn bearing_of(point: Vec3) -> f32 {
    point.x.atan2(-point.z).rem_euclid(TAU)
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z).normalize_or(Vec3::NEG_Z)
}

/// When zeppelin `z` of `zeppelins` enters, in bomber wave intervals: half
/// an interval after the bomber wave that starts its share of the round,
/// so bombers and zeppelin arrive one after the other.
pub fn zeppelin_slot(z: usize, zeppelins: usize, bomber_waves: usize) -> f32 {
    let share = (z as f32 + 0.5) * bomber_waves as f32 / zeppelins.max(1) as f32;
    share.floor() + 0.5
}

/// Where zeppelins enter before scattering, in turn: north, west and east
/// of the centre, close enough that at 30 m/s they are over the city in
/// under two minutes, each heading for it.
const ZEPPELIN_ENTRIES: [(Vec3, Vec3); 3] = [
    (Vec3::new(0.0, 700.0, -3_200.0), Vec3::Z),
    (Vec3::new(-3_200.0, 750.0, -600.0), Vec3::X),
    (Vec3::new(3_200.0, 650.0, -1_200.0), Vec3::NEG_X),
];

/// Where an escort enters relative to its bombers: this far behind (m) ...
const ESCORT_TRAIL: f32 = 400.0;
/// ... and this much higher (m).
const ESCORT_ABOVE: f32 = 200.0;

/// Fighter flight number `flight` of `flights`, `count` aircraft in
/// formation order. The pilots pick the type: rookies fly the Bf 109,
/// veterans the Me 309. Mixed flights alternate with the slower Bf 109
/// leading; rookies-then-veterans sends Bf 109 flights first, Me 309
/// flights after.
pub fn fighter_flight(
    count: u32,
    pilots: PilotMix,
    flight: usize,
    flights: usize,
) -> Vec<EnemyKind> {
    (0..count)
        .map(|i| match pilots {
            PilotMix::Rookies => EnemyKind::Bf109K4,
            PilotMix::Veterans => EnemyKind::Me309,
            PilotMix::Mixed if i % 2 == 0 => EnemyKind::Bf109K4,
            PilotMix::Mixed => EnemyKind::Me309,
            PilotMix::RookiesThenVeterans if 2 * flight < flights => EnemyKind::Bf109K4,
            PilotMix::RookiesThenVeterans => EnemyKind::Me309,
        })
        .collect()
}

/// A bomber flight of `count` in formation order: a Ju 390 leading He 177s.
pub fn bomber_flight(count: u32) -> Vec<EnemyKind> {
    (0..count)
        .map(|i| {
            if i == 0 {
                EnemyKind::Ju390
            } else {
                EnemyKind::He177
            }
        })
        .collect()
}
