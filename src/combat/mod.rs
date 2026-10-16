//! Combat: health, weapons (gun, guided missiles), lock-on and hits.
//!
//! [`CombatPlugin`] is pure simulation and runs headless (`MinimalPlugins`).
//! [`CombatVisualsPlugin`] adds meshes for tracers and missiles.
//!
//! Hit detection sits behind a narrow interface: [`hits`] translates the
//! physics layer's [`crate::physics::ProjectileHit`] and
//! [`crate::physics::PlayerCrashed`] (plus the missiles' proximity fuses)
//! into [`Hit`] and [`Destroyed`] messages in [`CombatSystems::DetectHits`];
//! everything after that (damage, kills, statistics, sound, HUD feedback)
//! only reads those two.

mod config;
pub mod hits;
mod lock;
mod rng;
#[cfg(test)]
mod tests;
mod visuals;
mod weapons;

use bevy::prelude::*;

pub use config::{CombatConfig, CombatKeys, GunSpec, LockSpec, MissileSpec};
pub use lock::{LockOn, LockState, in_seeker_cone};
pub use rng::CombatRng;
pub use visuals::CombatVisualsPlugin;
pub use weapons::{Gun, Missile, MissileRack, Munition, Trigger, Weapon};

use crate::{
    flight::{FlightState, FlightSystems, PlayerAircraft},
    physics::{EnemyCrashed, PlayerCrashed, ProjectileHit},
};

/// Side an aircraft or projectile fights on. Projectiles never hit their
/// own side.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Team {
    Player,
    Enemy,
}

/// Hit points. At zero the aircraft is destroyed.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn full(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }

    pub fn fraction(&self) -> f32 {
        (self.current / self.max).clamp(0.0, 1.0)
    }
}

/// Rough size of an aircraft: a sphere around its origin (m). Missile
/// proximity fuses go off within this plus their fuse radius. Direct hits
/// come from the physics colliders.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Hitbox {
    pub radius: f32,
}

/// World-space velocity (m/s) of an aircraft. Weapons inherit it, missiles
/// lead with it. The player's copy is taken from [`FlightState`] every
/// frame; enemies write their own. (Projectiles use Rapier's `Velocity`.)
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct AircraftVelocity(pub Vec3);

/// A projectile struck an aircraft.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub projectile: Entity,
    pub target: Entity,
    pub point: Vec3,
    /// Side that fired the projectile.
    pub team: Team,
    pub weapon: Weapon,
    pub damage: f32,
}

/// What brought an aircraft down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestroyCause {
    Gun,
    Missile,
    /// Flew into the ground or a building.
    Crash,
}

/// An aircraft was destroyed. Enemies are despawned in the same frame;
/// the player's entity stays (camera and HUD hang on it).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Destroyed {
    pub entity: Entity,
    pub team: Team,
    pub position: Vec3,
    pub cause: DestroyCause,
}

/// A weapon fired (one message per round or missile). Used for statistics
/// and as a sound hook.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ShotFired {
    pub shooter: Entity,
    pub team: Team,
    pub weapon: Weapon,
    pub position: Vec3,
}

/// Combat stages within a frame, all after the flight simulation.
/// AI runs before [`CombatSystems::Aim`] so locks and shots see the
/// enemies' new positions.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CombatSystems {
    /// Player trigger input, velocity sync.
    Input,
    /// Lock-on.
    Aim,
    /// Guns and missile launches. Gated by the round (no firing on the
    /// result screen).
    Fire,
    /// Missile guidance (the physics moves all projectiles).
    Move,
    /// Writes [`Hit`] and crash [`Destroyed`] messages. The only stage that
    /// knows how hits are found.
    DetectHits,
    /// Applies damage, writes [`Destroyed`], removes the dead.
    Resolve,
}

/// Headless combat simulation.
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatConfig>()
            .init_resource::<CombatKeys>()
            .init_resource::<CombatRng>()
            .add_message::<Hit>()
            .add_message::<Destroyed>()
            .add_message::<ShotFired>()
            // Registered by the physics plugin too; here so the combat
            // systems also run without it.
            .add_message::<ProjectileHit>()
            .add_message::<PlayerCrashed>()
            .add_message::<EnemyCrashed>()
            .configure_sets(
                Update,
                (
                    CombatSystems::Input,
                    CombatSystems::Aim,
                    CombatSystems::Fire,
                    CombatSystems::Move,
                    CombatSystems::DetectHits,
                    CombatSystems::Resolve,
                )
                    .chain()
                    .after(FlightSystems::Simulate),
            )
            .add_systems(
                Update,
                (
                    (
                        arm_new_player,
                        sync_player_velocity,
                        weapons::player_trigger,
                    )
                        .chain()
                        .in_set(CombatSystems::Input),
                    lock::update_lock.in_set(CombatSystems::Aim),
                    (weapons::fire_guns, weapons::launch_missiles).in_set(CombatSystems::Fire),
                    weapons::guide_missiles.in_set(CombatSystems::Move),
                    (
                        hits::projectile_hits,
                        hits::missile_fuses,
                        hits::player_crashes,
                        hits::enemy_crashes,
                        hits::remember_positions,
                    )
                        .chain()
                        .in_set(CombatSystems::DetectHits),
                    hits::apply_hits.in_set(CombatSystems::Resolve),
                ),
            );
    }
}

/// Combat components of a freshly armed player jet. Inserting them again
/// resets health, ammunition and lock (used when a round restarts).
pub fn player_loadout(cfg: &CombatConfig) -> impl Bundle {
    (
        Team::Player,
        Health::full(cfg.player_health),
        Hitbox {
            radius: cfg.player_hitbox_radius,
        },
        AircraftVelocity::default(),
        Gun::new(cfg.player_gun, Some(cfg.player_gun_ammo)),
        MissileRack::new(cfg.missile, cfg.player_missiles),
        LockOn::default(),
        Trigger::default(),
    )
}

/// Gives the player jet its weapons as soon as it exists.
fn arm_new_player(
    mut commands: Commands,
    cfg: Res<CombatConfig>,
    player: Query<Entity, (With<PlayerAircraft>, Without<Health>)>,
) {
    for jet in &player {
        commands.entity(jet).insert(player_loadout(&cfg));
    }
}

fn sync_player_velocity(
    mut player: Query<(&FlightState, &mut AircraftVelocity), With<PlayerAircraft>>,
) {
    for (state, mut velocity) in &mut player {
        velocity.0 = state.velocity;
    }
}
