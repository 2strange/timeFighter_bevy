//! Destructible city: area damage, ruins and the city's health.
//!
//! Gameplay sends [`DamageArea`] (a bomb, a crash, ...). Every building that
//! any collision box of comes within `radius` of `point` takes `strength`
//! damage; at [`BUILDING_TOUGHNESS`] accumulated damage it becomes a ruin: a
//! low dark stump on its base footprint. Ruins keep a collider (the stump),
//! the chunk mesh is rebuilt by the render side, [`BuildingDestroyed`] tells
//! everybody else, and [`CityHealth`] tracks how much of the centre stands.
//! [`RepairCity`] restores everything for a new round.
//!
//! Data and colliders only; works headless.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use super::{
    CityLayout,
    colliders::CityBuilding,
    layout::{Building, Facade, Part, Roof, Zone},
};

/// Damage that turns a building into a ruin.
pub const BUILDING_TOUGHNESS: f32 = 1.0;

/// Ruin stump: this fraction of the original height, within the limits.
const RUIN_HEIGHT_FRACTION: f32 = 0.12;
const RUIN_HEIGHT_MIN: f32 = 3.0;
const RUIN_HEIGHT_MAX: f32 = 18.0;
const RUIN_WALL: [f32; 3] = [0.26, 0.25, 0.24];
const RUIN_TOP: [f32; 3] = [0.16, 0.15, 0.15];

/// Damage everything within `radius` metres of `point` by `strength`
/// (1.0 = destroys any building it reaches).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct DamageArea {
    pub point: Vec3,
    pub radius: f32,
    pub strength: f32,
}

/// A building just became a ruin.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct BuildingDestroyed {
    /// Index in [`CityLayout::buildings`].
    pub index: usize,
    /// Centre of the building's footprint at street level.
    pub position: Vec3,
    pub zone: Zone,
    /// Volume it had (m³), for scoring.
    pub volume: f32,
}

/// Restore every building (new round).
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct RepairCity;

/// Accumulated damage per building.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CityDamage {
    pub damage: Vec<f32>,
    pub destroyed: Vec<bool>,
}

impl CityDamage {
    pub fn new(layout: &CityLayout) -> Self {
        let n = layout.buildings.len();
        Self {
            damage: vec![0.0; n],
            destroyed: vec![false; n],
        }
    }

    /// Applies `area` and returns the buildings it destroyed, in index order.
    /// Pure and deterministic.
    pub fn apply(&mut self, layout: &CityLayout, area: &DamageArea) -> Vec<usize> {
        let mut newly = Vec::new();
        if !(area.radius >= 0.0 && area.strength > 0.0 && area.point.is_finite()) {
            return newly;
        }
        for (i, building) in layout.buildings.iter().enumerate() {
            if self.destroyed[i] || !in_reach(building, area) {
                continue;
            }
            self.damage[i] += area.strength;
            if self.damage[i] >= BUILDING_TOUGHNESS {
                self.destroyed[i] = true;
                newly.push(i);
            }
        }
        newly
    }

    pub fn destroyed_count(&self) -> usize {
        self.destroyed.iter().filter(|&&d| d).count()
    }
}

/// Whether any collision box of `building` comes within the area's radius.
pub fn in_reach(building: &Building, area: &DamageArea) -> bool {
    let (lo, hi) = building.bounds();
    // Cheap reject on the bounds first.
    if distance_to_box(area.point, (lo + hi) / 2.0, (hi - lo) / 2.0) > area.radius {
        return false;
    }
    building
        .collision_boxes()
        .any(|(c, h)| distance_to_box(area.point, c, h) <= area.radius)
}

fn distance_to_box(p: Vec3, centre: Vec3, half: Vec3) -> f32 {
    ((p - centre).abs() - half).max(Vec3::ZERO).length()
}

/// The stump a destroyed building leaves: its base footprint, low and dark.
pub fn ruin_part(building: &Building) -> Part {
    let base = building.parts[0];
    let height = (building.height() * RUIN_HEIGHT_FRACTION).clamp(RUIN_HEIGHT_MIN, RUIN_HEIGHT_MAX);
    let height = height.min(building.height());
    Part {
        center: Vec3::new(base.center.x, height / 2.0, base.center.z),
        half_size: Vec3::new(base.half_size.x, height / 2.0, base.half_size.z),
        facade: Facade::Plain,
        roof: Roof::Flat,
        wall_color: RUIN_WALL,
        roof_color: RUIN_TOP,
        underside: false,
    }
}

/// Volume of a building's boxes (m³).
pub fn volume(building: &Building) -> f32 {
    building
        .parts
        .iter()
        .map(|p| 8.0 * p.half_size.x * p.half_size.y * p.half_size.z)
        .sum()
}

/// How much of the city still stands, by volume (1.0 = untouched).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CityHealth {
    /// Downtown buildings, the bombers' target.
    pub center_intact_ratio: f32,
    /// All buildings.
    pub intact_ratio: f32,
}

impl Default for CityHealth {
    fn default() -> Self {
        Self {
            center_intact_ratio: 1.0,
            intact_ratio: 1.0,
        }
    }
}

impl CityHealth {
    pub fn measure(layout: &CityLayout, damage: &CityDamage) -> Self {
        let (mut center, mut center_total, mut all, mut all_total) = (0.0, 0.0, 0.0, 0.0);
        for (i, b) in layout.buildings.iter().enumerate() {
            let v = f64::from(volume(b));
            let standing = if damage.destroyed.get(i).copied().unwrap_or(false) {
                0.0
            } else {
                v
            };
            all += standing;
            all_total += v;
            if b.zone == Zone::Downtown {
                center += standing;
                center_total += v;
            }
        }
        let ratio = |a: f64, t: f64| if t > 0.0 { (a / t) as f32 } else { 1.0 };
        Self {
            center_intact_ratio: ratio(center, center_total),
            intact_ratio: ratio(all, all_total),
        }
    }
}

/// Damage handling and health. Part of [`super::WorldPlugin`]; also usable
/// headless next to [`super::CityPlugin`].
pub struct CityDamagePlugin;

/// Damage is applied here (in `Update`); systems reacting to
/// [`BuildingDestroyed`] in the same frame run after it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CityDamageSystems;

impl Plugin for CityDamagePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DamageArea>()
            .add_message::<BuildingDestroyed>()
            .add_message::<RepairCity>()
            .init_resource::<CityHealth>()
            .add_systems(Startup, init_damage)
            .add_systems(
                Update,
                (repair_city, apply_damage)
                    .chain()
                    .in_set(CityDamageSystems),
            );
    }
}

fn init_damage(mut commands: Commands, layout: Res<CityLayout>) {
    commands.insert_resource(CityDamage::new(&layout));
    commands.insert_resource(CityHealth::default());
}

fn apply_damage(
    mut commands: Commands,
    layout: Res<CityLayout>,
    mut damage: ResMut<CityDamage>,
    mut health: ResMut<CityHealth>,
    mut areas: MessageReader<DamageArea>,
    mut destroyed: MessageWriter<BuildingDestroyed>,
    colliders: Query<(Entity, &CityBuilding)>,
) {
    let mut newly = Vec::new();
    for area in areas.read() {
        newly.extend(damage.apply(&layout, area));
    }
    if newly.is_empty() {
        return;
    }
    newly.sort_unstable();
    for &index in &newly {
        let b = &layout.buildings[index];
        let base = b.parts[0];
        destroyed.write(BuildingDestroyed {
            index,
            position: Vec3::new(base.center.x, 0.0, base.center.z),
            zone: b.zone,
            volume: volume(b),
        });
    }
    // Ruins keep a collider: the stump.
    for (entity, building) in &colliders {
        if newly.binary_search(&building.0).is_ok() {
            let stump = ruin_part(&layout.buildings[building.0]);
            let h = stump.half_size;
            commands.entity(entity).insert((
                Collider::cuboid(h.x, h.y, h.z),
                Transform::from_translation(stump.center),
            ));
        }
    }
    *health = CityHealth::measure(&layout, &damage);
}

fn repair_city(
    mut commands: Commands,
    layout: Res<CityLayout>,
    mut repairs: MessageReader<RepairCity>,
    mut damage: ResMut<CityDamage>,
    mut health: ResMut<CityHealth>,
    colliders: Query<(Entity, &CityBuilding)>,
) {
    if repairs.read().count() == 0 {
        return;
    }
    for (entity, building) in &colliders {
        if damage.destroyed[building.0] {
            let boxes: Vec<_> = layout.buildings[building.0].collision_boxes().collect();
            commands
                .entity(entity)
                .insert(super::colliders::static_body(&boxes));
        }
    }
    *damage = CityDamage::new(&layout);
    *health = CityHealth::default();
}
