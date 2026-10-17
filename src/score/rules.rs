//! The scoring rules, without ECS. The leaderboard worker
//! (`leaderboard/src/score.js`) recomputes every submitted score with the
//! same rules; `leaderboard/test/vectors.json` and `leaderboard/limits.json`
//! are written from here and checked by tests on both sides.

use crate::{
    ai::EnemyKind,
    game_state::{Difficulty, GameMode, GameSettings, Mission},
};

/// Points for shooting down an enemy with a missile (or by making it
/// crash); the gun pays [`GUN_FACTOR`] times as much.
pub fn kill_points(kind: EnemyKind) -> u32 {
    match kind {
        EnemyKind::Bf109K4 => 100,
        EnemyKind::Me309 => 150,
        EnemyKind::He177 => 200,
        EnemyKind::Ju390 => 400,
        EnemyKind::Zeppelin => 1_500,
    }
}

/// A gun kill is worth 3/2 of a missile kill (kill points are even, so
/// this stays whole).
pub const GUN_FACTOR: (u32, u32) = (3, 2);
/// A zeppelin whose wreck comes down outside the built-up city.
pub const ZEPPELIN_OUTSIDE: u32 = 500;
// Only with a victory:
pub const PER_MISSILE_LEFT: u32 = 75;
/// Per percent of the city centre still standing (Protect, Battle).
pub const PER_CITY_PERCENT: u32 = 20;
pub const VICTORY: u32 = 1_000;
/// Per whole second faster than the par time.
pub const PER_SECOND_UNDER_PAR: u32 = 5;
/// Par time per enemy (s): a fair pace for an experienced pilot.
pub const PAR_PER_FIGHTER: u32 = 15;
pub const PAR_PER_BOMBER: u32 = 20;
pub const PAR_PER_ZEPPELIN: u32 = 90;

/// How the enemy went down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KillWeapon {
    Gun,
    Missile,
    /// Flew into the ground, a building or the player.
    Crash,
}

impl KillWeapon {
    pub const ALL: [KillWeapon; 3] = [KillWeapon::Gun, KillWeapon::Missile, KillWeapon::Crash];

    pub fn key(self) -> &'static str {
        match self {
            KillWeapon::Gun => "gun",
            KillWeapon::Missile => "missile",
            KillWeapon::Crash => "crash",
        }
    }
}

/// Enemy kinds in the order of the leaderboard's keys.
pub const KINDS: [EnemyKind; 5] = [
    EnemyKind::Bf109K4,
    EnemyKind::Me309,
    EnemyKind::He177,
    EnemyKind::Ju390,
    EnemyKind::Zeppelin,
];

pub fn kind_key(kind: EnemyKind) -> &'static str {
    match kind {
        EnemyKind::Bf109K4 => "bf109k4",
        EnemyKind::Me309 => "me309",
        EnemyKind::He177 => "he177",
        EnemyKind::Ju390 => "ju390",
        EnemyKind::Zeppelin => "zeppelin",
    }
}

/// The enemy classes the leaderboard limits by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Fighter,
    Bomber,
    Zeppelin,
}

pub fn class(kind: EnemyKind) -> Class {
    match kind {
        EnemyKind::Zeppelin => Class::Zeppelin,
        kind if kind.is_bomber() => Class::Bomber,
        _ => Class::Fighter,
    }
}

/// What a round brings in, filled in while it is flown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScoreSheet {
    /// Kills by kind and weapon: `[kind][weapon]`, indexed like [`KINDS`]
    /// and [`KillWeapon::ALL`].
    pub kills: [[u32; 3]; 5],
    pub zeppelins_outside: u32,
    pub missiles_left: u32,
    /// Percent of the city centre still standing, where it counts.
    pub city_percent: Option<u32>,
    pub victory: bool,
    /// Whole seconds of the round.
    pub duration_s: u32,
}

impl ScoreSheet {
    pub fn add_kill(&mut self, kind: EnemyKind, weapon: KillWeapon) {
        let k = KINDS.iter().position(|k| *k == kind).unwrap_or(0);
        let w = KillWeapon::ALL
            .iter()
            .position(|w| *w == weapon)
            .unwrap_or(0);
        self.kills[k][w] += 1;
    }

    pub fn kill_count(&self) -> u32 {
        self.kills.iter().flatten().sum()
    }

    /// Kills of one class, for the leaderboard's limits.
    pub fn class_count(&self, of: Class) -> u32 {
        KINDS
            .iter()
            .zip(&self.kills)
            .filter(|(kind, _)| class(**kind) == of)
            .map(|(_, by_weapon)| by_weapon.iter().sum::<u32>())
            .sum()
    }

    /// Points for the kills alone, before the level factor.
    pub fn kill_points(&self) -> u32 {
        KINDS
            .iter()
            .zip(&self.kills)
            .map(|(kind, by_weapon)| {
                let base = kill_points(*kind);
                let gun = base * GUN_FACTOR.0 / GUN_FACTOR.1;
                by_weapon[0] * gun + (by_weapon[1] + by_weapon[2]) * base
            })
            .sum()
    }
}

/// The level's side of the rules: the factor and the par time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub factor: f32,
    pub par_s: u32,
}

impl Level {
    pub fn of(settings: &GameSettings) -> Self {
        let mission = Mission::from_settings(settings);
        let size = MissionSize::of(&mission);
        Self {
            factor: level_factor(settings.difficulty),
            par_s: size.par_s(),
        }
    }
}

/// The final multiplier of a level. Custom counts as Normal (its scores
/// stay on the local list).
pub fn level_factor(difficulty: Difficulty) -> f32 {
    match difficulty {
        Difficulty::SuperEasy => 0.5,
        Difficulty::Easy => 0.75,
        Difficulty::Normal | Difficulty::Custom(_) => 1.0,
        Difficulty::Hard => 1.5,
        Difficulty::Insane => 2.5,
    }
}

/// Enemies in a mission by class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionSize {
    /// In the waves.
    pub fighters: u32,
    pub bombers: u32,
    pub zeppelins: u32,
    /// Fighters the zeppelins may launch on top, at most.
    pub launched_max: u32,
}

impl MissionSize {
    pub fn of(mission: &Mission) -> Self {
        let mut size = Self {
            launched_max: mission.max_enemy_count() - mission.enemy_count(),
            ..Self::default()
        };
        for kind in mission
            .waves
            .iter()
            .flat_map(|w| &w.groups)
            .flat_map(|g| &g.members)
        {
            match class(*kind) {
                Class::Fighter => size.fighters += 1,
                Class::Bomber => size.bombers += 1,
                Class::Zeppelin => size.zeppelins += 1,
            }
        }
        size
    }

    pub fn par_s(&self) -> u32 {
        self.fighters * PAR_PER_FIGHTER
            + self.bombers * PAR_PER_BOMBER
            + self.zeppelins * PAR_PER_ZEPPELIN
    }
}

/// The score as the result screen lists it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Breakdown {
    pub kills: u32,
    pub zeppelins_outside: u32,
    pub missiles: u32,
    pub city: u32,
    pub victory: u32,
    /// Seconds under par and their points.
    pub seconds_under_par: u32,
    pub time: u32,
    pub subtotal: u32,
    pub factor: f32,
    pub total: u32,
}

/// Scores a round. The end bonuses only come with a victory, otherwise
/// dying early would pay.
pub fn score(sheet: &ScoreSheet, level: Level) -> Breakdown {
    let kills = sheet.kill_points();
    let zeppelins_outside = sheet.zeppelins_outside * ZEPPELIN_OUTSIDE;
    let (missiles, city, victory, seconds_under_par) = if sheet.victory {
        (
            sheet.missiles_left * PER_MISSILE_LEFT,
            sheet.city_percent.unwrap_or(0).min(100) * PER_CITY_PERCENT,
            VICTORY,
            level.par_s.saturating_sub(sheet.duration_s),
        )
    } else {
        (0, 0, 0, 0)
    };
    let time = seconds_under_par * PER_SECOND_UNDER_PAR;
    let subtotal = kills + zeppelins_outside + missiles + city + victory + time;
    Breakdown {
        kills,
        zeppelins_outside,
        missiles,
        city,
        victory,
        seconds_under_par,
        time,
        subtotal,
        factor: level.factor,
        total: apply_factor(subtotal, level.factor),
    }
}

/// `points × factor`, rounded half up (the worker does the same; the
/// factors are exact in binary floating point, so both agree).
pub fn apply_factor(points: u32, factor: f32) -> u32 {
    (points as f64 * factor as f64).round() as u32
}

/// Key of a mode on the leaderboard.
pub fn mode_key(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Hunt => "hunt",
        GameMode::Protect => "protect",
        GameMode::Battle => "battle",
    }
}

/// Key of a level on the leaderboard.
pub fn level_key(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::SuperEasy => "super-easy",
        Difficulty::Easy => "easy",
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
        Difficulty::Insane => "insane",
        Difficulty::Custom(_) => "custom",
    }
}

/// Whether scores of these settings may go online (not Custom).
pub fn online(settings: &GameSettings) -> bool {
    !matches!(settings.difficulty, Difficulty::Custom(_))
}

/// The submitted form of a sheet: the breakdown the worker recomputes.
pub fn sheet_json(sheet: &ScoreSheet) -> String {
    let kills: Vec<String> = KINDS
        .iter()
        .zip(&sheet.kills)
        .filter(|(_, by_weapon)| by_weapon.iter().any(|n| *n > 0))
        .map(|(kind, by_weapon)| {
            let weapons: Vec<String> = KillWeapon::ALL
                .iter()
                .zip(by_weapon)
                .filter(|(_, n)| **n > 0)
                .map(|(weapon, n)| format!("\"{}\":{n}", weapon.key()))
                .collect();
            format!("\"{}\":{{{}}}", kind_key(*kind), weapons.join(","))
        })
        .collect();
    format!(
        "{{\"kills\":{{{}}},\"zeppelins_outside\":{},\"missiles_left\":{},\"city_percent\":{},\"victory\":{}}}",
        kills.join(","),
        sheet.zeppelins_outside,
        sheet.missiles_left,
        sheet
            .city_percent
            .map_or("null".to_owned(), |p| p.to_string()),
        sheet.victory,
    )
}

/// `leaderboard/limits.json`: per mode and level the factor, the par time,
/// the missiles on board and the enemies by class (`fighters` in the
/// waves, `fighters_max` with what the zeppelins can launch). The worker
/// checks submissions against it.
pub fn limits_json() -> String {
    let mut modes = Vec::new();
    for mode in GameMode::ALL {
        let mut levels = Vec::new();
        for difficulty in Difficulty::PRESETS {
            let settings = GameSettings {
                mode,
                difficulty,
                ..Default::default()
            };
            let size = MissionSize::of(&Mission::from_settings(&settings));
            let level = Level::of(&settings);
            levels.push(format!(
                "    \"{}\": {{ \"factor\": {}, \"par_s\": {}, \"missiles\": {}, \"fighters\": {}, \"fighters_max\": {}, \"bombers\": {}, \"zeppelins\": {}, \"city\": {} }}",
                level_key(difficulty),
                level.factor,
                level.par_s,
                difficulty.params_for(mode).missiles,
                size.fighters,
                size.fighters + size.launched_max,
                size.bombers,
                size.zeppelins,
                mode != GameMode::Hunt,
            ));
        }
        modes.push(format!(
            "  \"{}\": {{\n{}\n  }}",
            mode_key(mode),
            levels.join(",\n")
        ));
    }
    format!("{{\n{}\n}}\n", modes.join(",\n"))
}

/// Number with thousands separated by commas.
pub fn grouped(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
