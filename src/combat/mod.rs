//! Combat: health, weapons (gun, guided missiles), lock-on and hits.
//!
//! [`CombatPlugin`] is pure simulation and runs headless (`MinimalPlugins`).
//! [`CombatVisualsPlugin`] adds meshes for tracers and missiles.
//!
//! Hit detection sits behind a narrow interface: whatever detects a hit
//! writes a [`Hit`] message in [`CombatSystems::DetectHits`]; everything
//! after that (damage, kills, statistics, sound, HUD feedback) only reads
//! [`Hit`] and [`Destroyed`]. For now [`hits`] detects hits with swept
//! spheres; the physics layer replaces it later without touching the rest.

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
pub use weapons::{Gun, Missile, MissileRack, Projectile, Trigger, Weapon};

use crate::flight::{FlightState, FlightSystems, PlayerAircraft};

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

/// Sphere around the aircraft's origin that projectiles can hit (m).
/// Used by the interim hit detection; the physics layer brings its own
/// collision shapes later.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Hitbox {
    pub radius: f32,
}

/// World-space velocity (m/s) of anything that can shoot or be shot at.
/// Weapons inherit it, missiles lead with it. The player's copy is taken
/// from [`FlightState`] every frame; enemies write their own.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Velocity(pub Vec3);

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
    /// Guidance and projectile motion.
    Move,
    /// Writes [`Hit`] messages. The only stage that knows how hits are found.
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
                    (weapons::guide_missiles, weapons::move_projectiles)
                        .chain()
                        .in_set(CombatSystems::Move),
                    (
                        hits::detect_projectile_hits,
                        hits::detect_player_ground_crash,
                    )
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
        Velocity::default(),
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

fn sync_player_velocity(mut player: Query<(&FlightState, &mut Velocity), With<PlayerAircraft>>) {
    for (state, mut velocity) in &mut player {
        velocity.0 = state.velocity;
    }
}
