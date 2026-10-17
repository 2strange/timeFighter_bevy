//! What the player picked in the menu, how it becomes [`GameSettings`],
//! and how it is stored.

use bevy::prelude::*;

use super::rows::{Choice, Row};
use crate::{
    game_state::{
        Difficulty, DifficultyParams, GameMode, GameSettings, Mission, MissionConfig, PilotMix,
    },
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

/// Who flies the enemies, in menu order, with the menu's label.
const PILOT_MIXES: [(PilotMix, &str); 4] = [
    (PilotMix::Rookies, "Rookies"),
    (PilotMix::Mixed, "Mixed"),
    (PilotMix::RookiesThenVeterans, "Rookies first"),
    (PilotMix::Veterans, "Veterans"),
];

/// The adjustable values of a custom difficulty, with the steps offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CustomField {
    Missiles,
    LockTime,
    /// Gun ammunition, unlimited as the first step.
    Gun,
    EnemySpeed,
    EnemyAccuracy,
    Enemies,
    Pilots,
    FlightSize,
    Attackers,
    BomberWaves,
    BombersPerWave,
    WaveInterval,
    CityLoss,
    Zeppelin,
}

impl CustomField {
    /// In menu order, section by section.
    pub const ALL: [CustomField; 14] = [
        CustomField::Missiles,
        CustomField::LockTime,
        CustomField::Gun,
        CustomField::EnemySpeed,
        CustomField::EnemyAccuracy,
        CustomField::Enemies,
        CustomField::Pilots,
        CustomField::FlightSize,
        CustomField::Attackers,
        CustomField::BomberWaves,
        CustomField::BombersPerWave,
        CustomField::WaveInterval,
        CustomField::CityLoss,
        CustomField::Zeppelin,
    ];

    pub fn section(self) -> Section {
        match self {
            CustomField::Missiles | CustomField::LockTime | CustomField::Gun => Section::Weapons,
            CustomField::EnemySpeed | CustomField::EnemyAccuracy => Section::General,
            CustomField::Enemies
            | CustomField::Pilots
            | CustomField::FlightSize
            | CustomField::Attackers => Section::Tab(EnemyTab::Fighters),
            CustomField::BomberWaves
            | CustomField::BombersPerWave
            | CustomField::WaveInterval
            | CustomField::CityLoss
            | CustomField::Zeppelin => Section::Tab(EnemyTab::Bombers),
        }
    }

    /// Whether the field means anything in `mode` (its tab aside).
    fn used_in(self, mode: MenuMode) -> bool {
        match self {
            // Battle's total comes from its waves (bombers plus escort).
            CustomField::Enemies => mode == MenuMode::Hunt,
            // The end boss comes only in Battle.
            CustomField::Zeppelin => mode == MenuMode::Battle,
            _ => true,
        }
    }

    fn label(self) -> &'static str {
        match self {
            CustomField::Missiles => "Missiles",
            CustomField::Enemies => "Enemies",
            CustomField::FlightSize => "Flight size",
            CustomField::EnemySpeed => "Enemy speed",
            CustomField::EnemyAccuracy => "Enemy aim",
            CustomField::Pilots => "Pilots",
            CustomField::Attackers => "Attacking at once",
            CustomField::LockTime => "Lock time",
            CustomField::Gun => "Gun",
            CustomField::BomberWaves => "Waves",
            CustomField::BombersPerWave => "Bombers per wave",
            CustomField::WaveInterval => "Interval",
            CustomField::CityLoss => "City loss",
            CustomField::Zeppelin => "Zeppelin at the end",
        }
    }

    fn key(self) -> &'static str {
        match self {
            CustomField::Missiles => "missiles",
            CustomField::Enemies => "enemies",
            CustomField::FlightSize => "flight",
            CustomField::EnemySpeed => "speed",
            CustomField::EnemyAccuracy => "aim",
            CustomField::Pilots => "pilots",
            CustomField::Attackers => "attackers",
            CustomField::LockTime => "lock",
            // A new key: the old "gun" (unlimited 0/1) would read wrongly.
            CustomField::Gun => "mg",
            CustomField::BomberWaves => "waves",
            CustomField::BombersPerWave => "bombers",
            CustomField::WaveInterval => "interval",
            CustomField::CityLoss => "loss",
            CustomField::Zeppelin => "zeppelin",
        }
    }

    /// Values offered, within the ranges the round accepts.
    fn steps(self) -> &'static [f32] {
        match self {
            // Includes every level's count in every mode (e.g. 8, 16, 24).
            CustomField::Missiles => &[
                0., 2., 4., 6., 8., 10., 12., 14., 16., 20., 24., 28., 32., 36., 40.,
            ],
            CustomField::Enemies => &[1., 2., 4., 6., 8., 10., 12., 16., 20., 24.],
            CustomField::FlightSize => &[1., 2., 3., 4., 5., 6.],
            CustomField::EnemySpeed => &[
                0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15, 1.2,
                1.25, 1.3, 1.35, 1.4, 1.45, 1.5,
            ],
            CustomField::EnemyAccuracy => &[0.25, 0.5, 0.7, 0.85, 1.0, 1.2, 1.5, 2.0],
            // Index into PILOT_MIXES.
            CustomField::Pilots => &[0., 1., 2., 3.],
            CustomField::Attackers => &[1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.],
            CustomField::LockTime => &[0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5],
            // 0 stands for unlimited.
            CustomField::Gun => &[0., 500., 1000., 1500., 2000., 3000.],
            CustomField::BomberWaves => &[1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12.],
            CustomField::BombersPerWave => &[1., 2., 3., 4., 5., 6., 7., 8.],
            CustomField::WaveInterval => &[
                60., 70., 80., 90., 100., 110., 120., 130., 140., 150., 160., 170., 180., 190.,
                200., 210., 220., 230., 240., 250., 260., 270., 280., 290., 300.,
            ],
            CustomField::CityLoss => &[0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6],
            CustomField::Zeppelin => &[0., 1.],
        }
    }

    fn step_label(self, value: f32) -> String {
        match self {
            CustomField::Zeppelin => if value > 0.5 { "On" } else { "Off" }.into(),
            CustomField::Gun if value < 0.5 => INFINITY.into(),
            CustomField::Pilots => PILOT_MIXES
                .get(value.round() as usize)
                .map_or("", |(_, label)| label)
                .into(),
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
            CustomField::Pilots => PILOT_MIXES
                .iter()
                .position(|(mix, _)| *mix == params.pilots)
                .unwrap_or(0) as f32,
            CustomField::Attackers => params.attackers as f32,
            CustomField::LockTime => params.lock_time,
            CustomField::Gun if params.unlimited_gun => 0.0,
            CustomField::Gun => params.gun_ammo as f32,
            CustomField::BomberWaves => params.bomber_waves as f32,
            CustomField::BombersPerWave => params.bombers_per_wave as f32,
            CustomField::WaveInterval => params.wave_interval,
            CustomField::CityLoss => params.city_loss,
            CustomField::Zeppelin => f32::from(u8::from(params.zeppelin)),
        }
    }

    fn set(self, params: &mut DifficultyParams, value: f32) {
        match self {
            CustomField::Missiles => params.missiles = value.round() as u32,
            CustomField::Enemies => params.enemies = value.round() as u32,
            CustomField::FlightSize => params.flight_size = value.round() as u32,
            CustomField::EnemySpeed => params.enemy_speed = value,
            CustomField::EnemyAccuracy => params.enemy_accuracy = value,
            CustomField::Pilots => {
                if let Some((mix, _)) = PILOT_MIXES.get(value.round() as usize) {
                    params.pilots = *mix;
                }
            }
            CustomField::Attackers => params.attackers = value.round() as u32,
            CustomField::LockTime => params.lock_time = value,
            CustomField::Gun => {
                params.unlimited_gun = value < 0.5;
                // Unlimited keeps the default count behind it, so a stored
                // line reads back the same.
                params.gun_ammo = if params.unlimited_gun {
                    DifficultyParams::default().gun_ammo
                } else {
                    value.round() as u32
                };
            }
            CustomField::BomberWaves => params.bomber_waves = value.round() as u32,
            CustomField::BombersPerWave => params.bombers_per_wave = value.round() as u32,
            CustomField::WaveInterval => params.wave_interval = value,
            CustomField::CityLoss => params.city_loss = value,
            CustomField::Zeppelin => params.zeppelin = value > 0.5,
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

/// The label of unlimited gun ammunition; the menu draws it as a symbol
/// (the built-in font has only ASCII).
pub const INFINITY: &str = "∞";

/// Rows of the menu, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowId {
    Mode,
    Difficulty,
    Custom(CustomField),
    Lighting,
    /// The tab bar over the enemy values.
    Tabs,
    Start,
}

impl RowId {
    /// Where the row sits in the panel.
    pub fn section(self) -> Section {
        match self {
            RowId::Mode | RowId::Difficulty => Section::Top,
            RowId::Custom(field) => field.section(),
            RowId::Lighting => Section::General,
            RowId::Tabs => Section::TabBar,
            RowId::Start => Section::Bottom,
        }
    }
}

/// Areas of the menu panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Top,
    Weapons,
    General,
    TabBar,
    Tab(EnemyTab),
    Bottom,
}

impl Section {
    /// Heading shown above the area, if any.
    pub fn heading(self) -> Option<&'static str> {
        match self {
            Section::Weapons => Some("WEAPONS"),
            Section::General => Some("GENERAL"),
            _ => None,
        }
    }
}

/// Tabs over the enemy values; more may follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EnemyTab {
    #[default]
    Fighters,
    Bombers,
}

impl EnemyTab {
    pub const ALL: [EnemyTab; 2] = [EnemyTab::Fighters, EnemyTab::Bombers];

    fn label(self) -> &'static str {
        match self {
            EnemyTab::Fighters => "Fighters",
            EnemyTab::Bombers => "Bombers",
        }
    }

    fn key(self) -> &'static str {
        match self {
            EnemyTab::Fighters => "fighters",
            EnemyTab::Bombers => "bombers",
        }
    }

    /// Whether `mode` has enemies of this kind.
    fn used_in(self, mode: MenuMode) -> bool {
        match self {
            EnemyTab::Fighters => mode != MenuMode::Protect,
            EnemyTab::Bombers => mode != MenuMode::Hunt,
        }
    }
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
    /// The enemy tab last picked (see [`MenuSelection::active_tab`]).
    pub tab: EnemyTab,
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
            tab: EnemyTab::default(),
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

    /// The enemy tab shown: the one last picked if the mode has such
    /// enemies, otherwise the first one it has.
    pub fn active_tab(&self) -> EnemyTab {
        if self.tab.used_in(self.mode) {
            return self.tab;
        }
        EnemyTab::ALL
            .into_iter()
            .find(|t| t.used_in(self.mode))
            .unwrap_or_default()
    }

    /// Fighters and bombers in the round these settings make.
    pub fn enemy_counts(&self) -> (u32, u32) {
        let mission = Mission::from_settings(&self.settings(), &MissionConfig::default());
        mission
            .waves
            .iter()
            .flat_map(|wave| &wave.groups)
            .flat_map(|group| &group.members)
            .fold((0, 0), |(fighters, bombers), kind| {
                if kind.is_bomber() {
                    (fighters, bombers + 1)
                } else {
                    (fighters + 1, bombers)
                }
            })
    }

    /// The menu rows for the current selection, top to bottom: mode and
    /// level; with Custom the weapons, the general values with the light,
    /// the tab bar and the values of the open tab; then Start.
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
                label: "Level".into(),
                choices: Difficulty::PRESETS
                    .iter()
                    .map(|d| Choice::new(d.display_name()))
                    .chain([Choice::new("Custom")])
                    .collect(),
                selected: self.level,
            },
        ];
        if self.is_custom() {
            let fields = |section: Section| {
                CustomField::ALL
                    .into_iter()
                    .filter(move |f| f.section() == section && f.used_in(self.mode))
                    .map(|f| self.field_row(f))
            };
            rows.extend(fields(Section::Weapons));
            rows.extend(fields(Section::General));
            rows.push(self.lighting_row());
            rows.push(self.tab_row());
            rows.extend(fields(Section::Tab(self.active_tab())));
        } else {
            rows.push(self.lighting_row());
        }
        rows.push(Row {
            id: RowId::Start,
            label: "Start".into(),
            choices: vec![],
            selected: 0,
        });
        rows
    }

    fn field_row(&self, field: CustomField) -> Row<RowId> {
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
    }

    fn lighting_row(&self) -> Row<RowId> {
        Row {
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
        }
    }

    /// The tabs with the number of such enemies in the round; a tab the
    /// mode has no enemies for is greyed out.
    fn tab_row(&self) -> Row<RowId> {
        let (fighters, bombers) = self.enemy_counts();
        let active = self.active_tab();
        Row {
            id: RowId::Tabs,
            label: "Enemies".into(),
            choices: EnemyTab::ALL
                .iter()
                .map(|tab| {
                    let count = match tab {
                        EnemyTab::Fighters => fighters,
                        EnemyTab::Bombers => bombers,
                    };
                    let label = format!("{} ({count})", tab.label());
                    if tab.used_in(self.mode) {
                        Choice::new(label)
                    } else {
                        Choice::disabled(label)
                    }
                })
                .collect(),
            selected: EnemyTab::ALL.iter().position(|t| *t == active).unwrap_or(0),
        }
    }

    /// The next or previous usable tab (`step` = -1 or 1), if there is one.
    pub fn switch_tab(&mut self, step: isize) -> bool {
        let row = self.tab_row();
        match row.step(step) {
            Some(index) if self.is_custom() => self.select(RowId::Tabs, index),
            _ => false,
        }
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
            RowId::Tabs => {
                if let Some(tab) = EnemyTab::ALL.get(index).filter(|t| t.used_in(self.mode)) {
                    self.tab = *tab;
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
            format!("tab={}", self.tab.key()),
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
                "tab" => {
                    if let Some(tab) = EnemyTab::ALL.into_iter().find(|t| t.key() == value) {
                        selection.tab = tab;
                    }
                }
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
        // Fighters first, then the bombers' tab.
        assert!(has(&selection, CustomField::FlightSize));
        // The enemy total follows from the waves in Battle.
        assert!(!has(&selection, CustomField::Enemies));
        assert!(selection.switch_tab(1));
        assert!(has(&selection, CustomField::BomberWaves));
        assert!(has(&selection, CustomField::Zeppelin));
        assert!(!has(&selection, CustomField::FlightSize));
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
        assert!(!has(&selection, CustomField::Zeppelin));

        selection.select(RowId::Mode, 1);
        assert!(!has(&selection, CustomField::Zeppelin), "Battle only");
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
                CustomField::Pilots => (0.0, (PILOT_MIXES.len() - 1) as f32),
                CustomField::Attackers => (1.0, 12.0),
                CustomField::LockTime => (0.2, 3.0),
                // 0 is unlimited, the counts within 50..9999.
                CustomField::Gun => (0.0, 9_999.0),
                CustomField::BomberWaves => (1.0, 12.0),
                CustomField::BombersPerWave => (1.0, 8.0),
                CustomField::WaveInterval => (60.0, 300.0),
                CustomField::CityLoss => (0.1, 0.6),
                CustomField::Zeppelin => (0.0, 1.0),
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
        selection.select(RowId::Custom(CustomField::Gun), 0);
        selection.select(RowId::Custom(CustomField::EnemySpeed), 0);
        selection.select(RowId::Lighting, 1);
        selection.select(RowId::Mode, 2);
        selection.select(RowId::Tabs, 1);
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
        let snapped = MenuSelection::from_line("missiles=7;speed=0.91;mg=1200");
        assert_eq!(snapped.custom.missiles, 6);
        assert_eq!(snapped.custom.enemy_speed, 0.9);
        assert_eq!(snapped.custom.gun_ammo, 1_000);
        // The old gun switch is not read as an ammunition count.
        assert_eq!(
            MenuSelection::from_line("gun=0;ammo=500"),
            MenuSelection::default()
        );
    }

    #[test]
    fn zeppelin_can_be_switched_on_in_an_easy_battle() {
        let mut selection = MenuSelection::default();
        selection.select(RowId::Mode, 2);
        selection.select(RowId::Difficulty, 1);
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        // Easy has no end boss; Custom starts from that.
        assert!(!selection.custom.zeppelin);
        assert_eq!(MenuSelection::from_line(&selection.to_line()), selection);

        selection.select(RowId::Custom(CustomField::Zeppelin), 1);
        let Difficulty::Custom(params) = selection.settings().difficulty else {
            panic!("custom expected");
        };
        assert!(params.zeppelin);
        assert_eq!(MenuSelection::from_line(&selection.to_line()), selection);
    }

    #[test]
    fn gun_offers_unlimited_first_then_counts() {
        let mut selection = MenuSelection::default();
        // Easy shoots without limit.
        selection.select(RowId::Difficulty, 1);
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        let gun_row = |s: &MenuSelection| {
            s.rows()
                .into_iter()
                .find(|r| r.id == RowId::Custom(CustomField::Gun))
                .unwrap()
        };
        let row = gun_row(&selection);
        assert_eq!(row.choices[0].label, INFINITY);
        assert_eq!(row.selected, 0);

        let most = row.choices.iter().position(|c| c.label == "3000").unwrap();
        selection.select(RowId::Custom(CustomField::Gun), most);
        let Difficulty::Custom(params) = selection.settings().difficulty else {
            panic!("custom expected");
        };
        assert!(!params.unlimited_gun);
        assert_eq!(params.gun_ammo, 3_000);
        assert_eq!(MenuSelection::from_line(&selection.to_line()), selection);

        selection.select(RowId::Custom(CustomField::Gun), 0);
        assert!(selection.custom.unlimited_gun);
        assert_eq!(MenuSelection::from_line(&selection.to_line()), selection);
    }

    #[test]
    fn tabs_follow_the_mode_and_count_its_enemies() {
        let mut selection = MenuSelection::default();
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        let tabs = |s: &MenuSelection| s.rows().into_iter().find(|r| r.id == RowId::Tabs).unwrap();
        let enabled = |s: &MenuSelection| {
            tabs(s)
                .choices
                .iter()
                .map(|c| c.enabled)
                .collect::<Vec<_>>()
        };

        // Hunt: fighters only, with the round's enemy count.
        assert_eq!(enabled(&selection), [true, false]);
        let hunt = Difficulty::Normal.params_for(GameMode::Hunt).enemies;
        assert_eq!(
            tabs(&selection).choices[0].label,
            format!("Fighters ({hunt})")
        );
        assert!(!selection.switch_tab(1), "bombers greyed out");
        assert!(!selection.select(RowId::Tabs, 1));

        // Protect: bombers only, open even though fighters were picked.
        selection.select(RowId::Mode, 1);
        assert_eq!(enabled(&selection), [false, true]);
        assert_eq!(selection.active_tab(), EnemyTab::Bombers);
        assert_eq!(tabs(&selection).selected, 1);

        // Battle: both; the counts add up to the round's enemies.
        selection.select(RowId::Mode, 2);
        assert_eq!(enabled(&selection), [true, true]);
        let (fighters, bombers) = selection.enemy_counts();
        let mission = Mission::from_settings(&selection.settings(), &MissionConfig::default());
        assert_eq!(fighters + bombers, mission.enemy_count());
        assert!(fighters > 0 && bombers > 0);
        assert!(selection.switch_tab(1));
        assert_eq!(selection.active_tab(), EnemyTab::Bombers);
        assert!(!selection.switch_tab(1), "no tab after the last");
    }

    #[test]
    fn rows_come_in_sections_with_only_the_open_tab() {
        let mut selection = MenuSelection::default();
        // Without Custom: mode, level, light, start.
        let ids = |s: &MenuSelection| s.rows().iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(
            ids(&selection),
            [
                RowId::Mode,
                RowId::Difficulty,
                RowId::Lighting,
                RowId::Start
            ]
        );

        selection.select(RowId::Mode, 2);
        selection.select(RowId::Difficulty, CUSTOM_LEVEL);
        for tab in EnemyTab::ALL {
            selection.select(RowId::Tabs, tab as usize);
            let sections: Vec<_> = ids(&selection).iter().map(|id| id.section()).collect();
            assert_eq!(
                sections,
                [
                    vec![Section::Top; 2],
                    vec![Section::Weapons; 3],
                    vec![Section::General; 3],
                    vec![Section::TabBar],
                    vec![Section::Tab(tab); if tab == EnemyTab::Fighters { 3 } else { 5 }],
                    vec![Section::Bottom],
                ]
                .concat(),
                "{tab:?}"
            );
        }
    }
}
