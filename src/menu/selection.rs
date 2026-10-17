//! What the player picked in the menu, how it becomes [`GameSettings`],
//! and how it is stored.

use bevy::prelude::*;

use super::rows::{Choice, Row};
use crate::{
    game_state::{Difficulty, DifficultyParams, GameMode, GameSettings},
    world::Lighting,
};

/// Game modes in the menu. A mode can be picked once the game offers it in
/// [`GameMode::ALL`]; the others show as "coming soon".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuMode {
    Hunt,
    Protect,
    Battle,
}

impl MenuMode {
    const ALL: [MenuMode; 3] = [MenuMode::Hunt, MenuMode::Protect, MenuMode::Battle];

    fn available(self) -> bool {
        self.game_mode()
            .is_some_and(|mode| GameMode::ALL.contains(&mode))
    }

    fn label(self) -> &'static str {
        match self {
            MenuMode::Hunt => "Hunt",
            MenuMode::Protect => "Protect",
            MenuMode::Battle => "Battle",
        }
    }

    fn game_mode(self) -> Option<GameMode> {
        match self {
            MenuMode::Hunt => Some(GameMode::Hunt),
            MenuMode::Protect => Some(GameMode::Protect),
            MenuMode::Battle => Some(GameMode::Battle),
        }
    }
}

/// The adjustable values of a custom difficulty, with the steps offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CustomField {
    Missiles,
    Enemies,
    FlightSize,
    EnemySpeed,
    EnemyAccuracy,
    Attackers,
    LockTime,
    UnlimitedGun,
    BomberWaves,
    BombersPerWave,
    WaveInterval,
    CityLoss,
}

impl CustomField {
    pub const ALL: [CustomField; 12] = [
        CustomField::Missiles,
        CustomField::Enemies,
        CustomField::FlightSize,
        CustomField::EnemySpeed,
        CustomField::EnemyAccuracy,
        CustomField::Attackers,
        CustomField::LockTime,
        CustomField::UnlimitedGun,
        CustomField::BomberWaves,
        CustomField::BombersPerWave,
        CustomField::WaveInterval,
        CustomField::CityLoss,
    ];

    /// Only meaningful with bombers (Protect, Battle).
    fn bombers_only(self) -> bool {
        matches!(
            self,
            CustomField::BomberWaves
                | CustomField::BombersPerWave
                | CustomField::WaveInterval
                | CustomField::CityLoss
        )
    }

    fn label(self) -> &'static str {
        match self {
            CustomField::Missiles => "Missiles",
            CustomField::Enemies => "Enemies",
            CustomField::FlightSize => "Flight size",
            CustomField::EnemySpeed => "Enemy speed",
            CustomField::EnemyAccuracy => "Enemy aim",
            CustomField::Attackers => "Attacking at once",
            CustomField::LockTime => "Missile lock time",
            CustomField::UnlimitedGun => "Unlimited gun",
            CustomField::BomberWaves => "Bomber waves",
            CustomField::BombersPerWave => "Bombers per wave",
            CustomField::WaveInterval => "Time between waves",
            CustomField::CityLoss => "City lost at",
        }
    }

    fn key(self) -> &'static str {
        match self {
            CustomField::Missiles => "missiles",
            CustomField::Enemies => "enemies",
            CustomField::FlightSize => "flight",
            CustomField::EnemySpeed => "speed",
            CustomField::EnemyAccuracy => "aim",
            CustomField::Attackers => "attackers",
            CustomField::LockTime => "lock",
            CustomField::UnlimitedGun => "gun",
            CustomField::BomberWaves => "waves",
            CustomField::BombersPerWave => "bombers",
            CustomField::WaveInterval => "interval",
            CustomField::CityLoss => "loss",
        }
    }

    /// Values offered, within the ranges the round accepts.
    fn steps(self) -> &'static [f32] {
        match self {
            // Includes every level's count in every mode (e.g. 14, 16, 24).
            CustomField::Missiles => &[0., 2., 4., 6., 8., 10., 12., 14., 16., 20., 24., 30., 40.],
            CustomField::Enemies => &[1., 2., 4., 6., 8., 10., 12., 16., 20., 24.],
            CustomField::FlightSize => &[1., 2., 3., 4., 5., 6.],
            CustomField::EnemySpeed => &[0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5],
            CustomField::EnemyAccuracy => &[0.25, 0.5, 0.7, 0.85, 1.0, 1.2, 1.5, 2.0],
            CustomField::Attackers => &[1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.],
            CustomField::LockTime => &[0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5],
            CustomField::UnlimitedGun => &[0., 1.],
            CustomField::BomberWaves => &[1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.],
            CustomField::BombersPerWave => &[1., 2., 3., 4., 5., 6., 7., 8.],
            CustomField::WaveInterval => &[
                60., 70., 80., 90., 100., 110., 120., 130., 140., 150., 160., 170., 180., 190.,
                200., 210., 220., 230., 240., 250., 260., 270., 280., 290., 300.,
            ],
            CustomField::CityLoss => &[0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6],
        }
    }

    fn step_label(self, value: f32) -> String {
        match self {
            CustomField::UnlimitedGun => if value > 0.5 { "On" } else { "Off" }.into(),
            CustomField::LockTime => format!("{value:.1} s"),
            CustomField::WaveInterval => format!("{value:.0} s"),
            CustomField::CityLoss => format!("{:.0} %", value * 100.0),
            CustomField::EnemySpeed | CustomField::EnemyAccuracy => {
                let number = format!("{value:.2}");
                format!("{}x", number.trim_end_matches('0').trim_end_matches('.'))
            }
            _ => format!("{value:.0}"),
        }
    }

    fn get(self, params: &DifficultyParams) -> f32 {
        match self {
            CustomField::Missiles => params.missiles as f32,
            CustomField::Enemies => params.enemies as f32,
            CustomField::FlightSize => params.flight_size as f32,
            CustomField::EnemySpeed => params.enemy_speed,
            CustomField::EnemyAccuracy => params.enemy_accuracy,
            CustomField::Attackers => params.attackers as f32,
            CustomField::LockTime => params.lock_time,
            CustomField::UnlimitedGun => f32::from(u8::from(params.unlimited_gun)),
            CustomField::BomberWaves => params.bomber_waves as f32,
            CustomField::BombersPerWave => params.bombers_per_wave as f32,
            CustomField::WaveInterval => params.wave_interval,
            CustomField::CityLoss => params.city_loss,
        }
    }

    fn set(self, params: &mut DifficultyParams, value: f32) {
        match self {
            CustomField::Missiles => params.missiles = value.round() as u32,
            CustomField::Enemies => params.enemies = value.round() as u32,
            CustomField::FlightSize => params.flight_size = value.round() as u32,
            CustomField::EnemySpeed => params.enemy_speed = value,
            CustomField::EnemyAccuracy => params.enemy_accuracy = value,
            CustomField::Attackers => params.attackers = value.round() as u32,
            CustomField::LockTime => params.lock_time = value,
            CustomField::UnlimitedGun => params.unlimited_gun = value > 0.5,
            CustomField::BomberWaves => params.bomber_waves = value.round() as u32,
            CustomField::BombersPerWave => params.bombers_per_wave = value.round() as u32,
            CustomField::WaveInterval => params.wave_interval = value,
            CustomField::CityLoss => params.city_loss = value,
        }
    }

    /// Index of the step nearest to the current value.
    fn nearest_step(self, params: &DifficultyParams) -> usize {
        let value = self.get(params);
        let steps = self.steps();
        (0..steps.len())
            .min_by(|&a, &b| {
                (steps[a] - value)
                    .abs()
                    .total_cmp(&(steps[b] - value).abs())
            })
            .unwrap_or(0)
    }
}

/// Rows of the menu, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowId {
    Mode,
    Difficulty,
    Custom(CustomField),
    Lighting,
    Start,
}

/// Number of difficulty choices: the presets plus Custom.
const LEVELS: usize = Difficulty::PRESETS.len() + 1;
const CUSTOM_LEVEL: usize = LEVELS - 1;

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct MenuSelection {
    pub mode: MenuMode,
    /// Index into the presets; the last one is Custom.
    pub level: usize,
    pub custom: DifficultyParams,
    /// The custom values were changed by the player (otherwise entering
    /// Custom starts from the level picked before).
    pub custom_edited: bool,
    pub lighting: Lighting,
}

impl Default for MenuSelection {
    fn default() -> Self {
        let normal = Difficulty::PRESETS
            .iter()
            .position(|d| *d == Difficulty::Normal)
            .unwrap_or(0);
        Self {
            mode: MenuMode::Hunt,
            level: normal,
            custom: Difficulty::Normal.params(),
            custom_edited: false,
            lighting: Lighting::Day,
        }
    }
}

impl MenuSelection {
    pub fn is_custom(&self) -> bool {
        self.level == CUSTOM_LEVEL
    }

    pub fn difficulty(&self) -> Difficulty {
        Difficulty::PRESETS
            .get(self.level)
            .copied()
            .unwrap_or(Difficulty::Custom(self.custom))
    }

    pub fn settings(&self) -> GameSettings {
        GameSettings {
            mode: self.mode.game_mode().unwrap_or_default(),
            difficulty: self.difficulty(),
            lighting: self.lighting,
        }
    }

    /// The menu rows for the current selection (custom values only when
    /// Custom is picked).
    pub fn rows(&self) -> Vec<Row<RowId>> {
        let mut rows = vec![
            Row {
                id: RowId::Mode,
                label: "Mode".into(),
                choices: MenuMode::ALL
                    .iter()
                    .map(|m| {
                        if m.available() {
                            Choice::new(m.label())
                        } else {
                            Choice::disabled(format!("{} (coming soon)", m.label()))
                        }
                    })
                    .collect(),
                selected: MenuMode::ALL
                    .iter()
                    .position(|m| *m == self.mode)
                    .unwrap_or(0),
            },
            Row {
                id: RowId::Difficulty,
                label: "Difficulty".into(),
                choices: Difficulty::PRESETS
                    .iter()
                    .map(|d| Choice::new(d.display_name()))
                    .chain([Choice::new("Custom")])
                    .collect(),
                selected: self.level,
            },
        ];
        if self.is_custom() {
            let bombers = matches!(self.mode, MenuMode::Protect | MenuMode::Battle);
            let fields = CustomField::ALL
                .iter()
                .filter(|f| bombers || !f.bombers_only())
                // Battle's total comes from its waves (bombers plus escort).
                .filter(|f| !(self.mode == MenuMode::Battle && **f == CustomField::Enemies));
            rows.extend(fields.map(|&field| {
                Row {
                    id: RowId::Custom(field),
                    label: field.label().into(),
                    choices: field
                        .steps()
                        .iter()
                        .map(|&v| Choice::new(field.step_label(v)))
                        .collect(),
                    selected: field.nearest_step(&self.custom),
                }
            }));
        }
        rows.push(Row {
            id: RowId::Lighting,
            label: "Light".into(),
            choices: Lighting::ALL
                .iter()
                .map(|l| Choice::new(l.label()))
                .collect(),
            selected: Lighting::ALL
                .iter()
                .position(|l| *l == self.lighting)
                .unwrap_or(0),
        });
        rows.push(Row {
            id: RowId::Start,
            label: "Start".into(),
            choices: vec![],
            selected: 0,
        });
        rows
    }

    /// Picks choice `index` in row `id`. Returns whether anything changed;
    /// disabled choices are refused.
    pub fn select(&mut self, id: RowId, index: usize) -> bool {
        let before = self.clone();
        match id {
            RowId::Mode => {
                if let Some(mode) = MenuMode::ALL.get(index).filter(|m| m.available()) {
                    self.mode = *mode;
                }
            }
            RowId::Difficulty => {
                if index < LEVELS {
                    if index == CUSTOM_LEVEL && !self.is_custom() && !self.custom_edited {
                        // The values of the level for this mode (missiles
                        // differ between modes).
                        let mode = self.mode.game_mode().unwrap_or_default();
                        self.custom = self.difficulty().params_for(mode);
                    }
                    self.level = index;
                }
            }
            RowId::Custom(field) => {
                if let Some(&value) = field.steps().get(index) {
                    field.set(&mut self.custom, value);
                    self.custom_edited = true;
                }
            }
            RowId::Lighting => {
                if let Some(lighting) = Lighting::ALL.get(index) {
                    self.lighting = *lighting;
                }
            }
            RowId::Start => {}
        }
        *self != before
    }

    /// Applies a start request from the start page (`mode`, `level` and
    /// `light`, by name like `protect` / `hard` / `evening`, levels also by
    /// index); unknown values are ignored.
    pub fn apply_request(&mut self, request: &str) {
        for (key, value) in super::storage::parse(request) {
            let value = value.to_lowercase();
            match key {
                "mode" => {
                    if let Some(mode) = MenuMode::ALL
                        .into_iter()
                        .find(|m| m.available() && m.label().eq_ignore_ascii_case(&value))
                    {
                        self.mode = mode;
                    }
                }
                "light" => {
                    if let Some(lighting) = Lighting::ALL
                        .into_iter()
                        .find(|l| l.label().eq_ignore_ascii_case(&value))
                    {
                        self.lighting = lighting;
                    }
                }
                "level" => {
                    let by_name = |name: &str| name.to_lowercase().replace(' ', "-") == value;
                    let level = value.parse::<usize>().ok().or_else(|| {
                        if value == "custom" {
                            Some(CUSTOM_LEVEL)
                        } else {
                            Difficulty::PRESETS
                                .iter()
                                .position(|d| by_name(d.display_name()))
                        }
                    });
                    if let Some(level) = level.filter(|l| *l < LEVELS) {
                        self.select(RowId::Difficulty, level);
                    }
                }
                _ => {}
            }
        }
    }

    /// One line for storage.
    pub fn to_line(&self) -> String {
        let mut parts = vec![
            "v=1".to_owned(),
            format!("mode={}", self.mode.label().to_lowercase()),
            format!("level={}", self.level),
            format!("light={}", self.lighting.label().to_lowercase()),
            format!("edited={}", u8::from(self.custom_edited)),
        ];
        for field in CustomField::ALL {
            parts.push(format!("{}={}", field.key(), field.get(&self.custom)));
        }
        parts.join(";")
    }

    /// Reads a stored line; values that are missing, unknown or out of
    /// range keep their defaults.
    pub fn from_line(line: &str) -> Self {
        let mut selection = Self::default();
        for (key, value) in super::storage::parse(line) {
            match key {
                "mode" => {
                    if let Some(mode) = MenuMode::ALL
                        .into_iter()
                        .find(|m| m.available() && m.label().eq_ignore_ascii_case(value))
                    {
                        selection.mode = mode;
                    }
                }
                "level" => {
                    if let Some(level) = value.parse().ok().filter(|l| *l < LEVELS) {
                        selection.level = level;
                    }
                }
                "light" => {
                    if let Some(lighting) = Lighting::ALL
                        .into_iter()
                        .find(|l| l.label().eq_ignore_ascii_case(value))
                    {
                        selection.lighting = lighting;
                    }
                }
                "edited" => selection.custom_edited = value == "1",
                _ => {
                    let field = CustomField::ALL.into_iter().find(|f| f.key() == key);
                    if let (Some(field), Ok(value)) = (field, value.parse::<f32>()) {
                        // Snap to an offered step, so the menu can show it.
                        let mut probe = selection.custom;
                        field.set(&mut probe, value);
                        let step = field.steps()[field.nearest_step(&probe)];
                        field.set(&mut selection.custom, step);
                    }
                }
            }
        }
        selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_normal_hunt_by_day() {
        let selection = MenuSelection::default();
        assert_eq!(
            selection.settings(),
            GameSettings {
                mode: GameMode::Hunt,
                difficulty: Difficulty::Normal,
                lighting: Lighting::Day,
            }
        );
    }

    #[test]
    fn modes_follow_what_the_game_offers() {
        let mut selection = MenuSelection::default();
        // All three are in GameMode::ALL now.
        assert!(selection.select(RowId::Mode, 1));
        assert_eq!(selection.settings().mode, GameMode::Protect);
        assert!(selection.select(RowId::Mode, 2));
        assert_eq!(selection.settings().mode, GameMode::Battle);
        let rows = selection.rows();
        assert!(rows[0].choices.iter().all(|c| c.enabled));
        assert!(
            !rows[0]
                .choices
                .iter()
                .any(|c| c.label.contains("coming soon"))
        );
        for mode in GameMode::ALL {
            assert!(
                MenuMode::ALL.iter().any(|m| m.game_mode() == Some(mode)),
                "{mode:?} missing in the menu"
            );
        }
    }

    #[test]
    fn battle_shows_bomber_and_fighter_values_and_its_own_missiles() {
        let mut selection = MenuSelection::default();
        selection.select(RowId::Mode, 2);
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        let has = |s: &MenuSelection, f| s.rows().iter().any(|r| r.id == RowId::Custom(f));
        assert!(has(&selection, CustomField::BomberWaves));
        assert!(has(&selection, CustomField::FlightSize));
        // The enemy total follows from the waves in Battle.
        assert!(!has(&selection, CustomField::Enemies));
        // Custom starts from Normal's values for Battle.
        assert_eq!(
            selection.custom.missiles,
            Difficulty::Normal.params_for(GameMode::Battle).missiles
        );
        assert_eq!(MenuSelection::from_line(&selection.to_line()), selection);
    }

    #[test]
    fn every_level_value_is_an_offered_step() {
        // Otherwise Custom would start from a rounded value.
        for mode in GameMode::ALL {
            for level in Difficulty::PRESETS {
                let params = level.params_for(mode);
                for field in CustomField::ALL {
                    let value = field.get(&params);
                    assert!(
                        field.steps().iter().any(|s| (s - value).abs() < 1e-4),
                        "{mode:?} {level:?} {field:?} = {value} not offered"
                    );
                }
            }
        }
    }

    #[test]
    fn bomber_values_show_only_in_protect() {
        let mut selection = MenuSelection::default();
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        let has = |s: &MenuSelection, f| s.rows().iter().any(|r| r.id == RowId::Custom(f));
        assert!(!has(&selection, CustomField::BomberWaves));
        assert!(has(&selection, CustomField::Missiles));

        selection.select(RowId::Mode, 1);
        for field in [
            CustomField::BomberWaves,
            CustomField::BombersPerWave,
            CustomField::WaveInterval,
            CustomField::CityLoss,
        ] {
            assert!(has(&selection, field), "{field:?}");
        }
        let interval = CustomField::WaveInterval
            .steps()
            .iter()
            .position(|v| *v == 200.0)
            .unwrap();
        selection.select(RowId::Custom(CustomField::WaveInterval), interval);
        let loss = CustomField::CityLoss
            .steps()
            .iter()
            .position(|v| *v == 0.25)
            .unwrap();
        selection.select(RowId::Custom(CustomField::CityLoss), loss);
        let Difficulty::Custom(params) = selection.settings().difficulty else {
            panic!("custom expected");
        };
        assert_eq!((params.wave_interval, params.city_loss), (200.0, 0.25));
        // Stored and read back.
        assert_eq!(MenuSelection::from_line(&selection.to_line()), selection);
    }

    #[test]
    fn custom_starts_from_the_level_picked_before_and_keeps_edits() {
        let mut selection = MenuSelection::default();
        selection.select(RowId::Difficulty, 0); // Super Easy
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        assert_eq!(selection.custom, Difficulty::SuperEasy.params());
        assert!(
            selection
                .rows()
                .iter()
                .any(|r| r.id == RowId::Custom(CustomField::Missiles))
        );

        let five_enemies = CustomField::Enemies
            .steps()
            .iter()
            .position(|v| *v == 6.0)
            .unwrap();
        selection.select(RowId::Custom(CustomField::Enemies), five_enemies);
        // Away and back: the edit stays.
        selection.select(RowId::Difficulty, 3);
        assert!(
            !selection
                .rows()
                .iter()
                .any(|r| matches!(r.id, RowId::Custom(_)))
        );
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        let Difficulty::Custom(params) = selection.settings().difficulty else {
            panic!("custom expected");
        };
        assert_eq!(params.enemies, 6);
        assert_eq!(params.missiles, Difficulty::SuperEasy.params().missiles);
    }

    #[test]
    fn custom_steps_stay_within_the_rounds_ranges() {
        for field in CustomField::ALL {
            let steps = field.steps();
            let (lo, hi) = (steps[0], steps[steps.len() - 1]);
            let (min, max) = match field {
                CustomField::Missiles => (0.0, 40.0),
                CustomField::Enemies => (1.0, 24.0),
                CustomField::FlightSize => (1.0, 6.0),
                CustomField::EnemySpeed => (0.5, 1.5),
                CustomField::EnemyAccuracy => (0.25, 2.0),
                CustomField::Attackers => (1.0, 12.0),
                CustomField::LockTime => (0.2, 3.0),
                CustomField::UnlimitedGun => (0.0, 1.0),
                CustomField::BomberWaves => (1.0, 12.0),
                CustomField::BombersPerWave => (1.0, 8.0),
                CustomField::WaveInterval => (60.0, 300.0),
                CustomField::CityLoss => (0.1, 0.6),
            };
            assert!(lo >= min && hi <= max, "{field:?}");
            assert!(
                steps.windows(2).all(|w| w[0] < w[1]),
                "{field:?} not ascending"
            );
        }
    }

    #[test]
    fn stored_line_round_trips() {
        let mut selection = MenuSelection::default();
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        selection.select(RowId::Custom(CustomField::Missiles), 9);
        selection.select(RowId::Custom(CustomField::UnlimitedGun), 1);
        selection.select(RowId::Custom(CustomField::EnemySpeed), 0);
        selection.select(RowId::Lighting, 1);
        let line = selection.to_line();
        assert_eq!(MenuSelection::from_line(&line), selection, "{line}");
    }

    #[test]
    fn damaged_or_foreign_lines_fall_back_to_defaults() {
        assert_eq!(MenuSelection::from_line(""), MenuSelection::default());
        assert_eq!(
            MenuSelection::from_line("garbage;;=;level=99;mode=dogfight;light=purple"),
            MenuSelection::default()
        );
        // Values between steps snap to the nearest step.
        let snapped = MenuSelection::from_line("missiles=7;speed=0.93");
        assert_eq!(snapped.custom.missiles, 6);
        assert_eq!(snapped.custom.enemy_speed, 0.9);
    }
}
