//! Combat: health, weapons (gun, guided missiles), lock-on and hits.
//!
//! [`CombatPlugin`] is pure simulation and runs headless (`MinimalPlugins`).
//! [`CombatVisualsPlugin`] adds meshes for tracers, missiles and explosions.
//!
//! Hit detection sits behind a narrow interface: [`hits`] translates the
//! physics layer's [`crate::physics::ProjectileHit`] and
//! [`crate::physics::PlayerCrashed`] (plus the missiles' proximity fuses)
//! into [`Hit`] and [`Destroyed`] messages in [`CombatSystems::DetectHits`];
//! everything after that (damage, kills, statistics, sound, HUD feedback)
//! only reads those two.

mod bombs;
mod config;
mod effects;
pub mod hits;
mod lock;
mod rng;
#[cfg(test)]
mod tests;
mod turrets;
mod visuals;
mod weapons;
mod wreck;

use bevy::prelude::*;

pub use bombs::{Bomb, BombImpact, BombSpec, GRAVITY, bomb_bundle, fall_time};
pub use config::{CombatConfig, CombatKeys, GunSpec, LockSpec, MissileSpec, key_label};
pub use effects::{Effect, EffectKind};
pub use lock::{LockOn, LockState, in_seeker_cone};
pub use rng::CombatRng;
pub use turrets::{TurretMount, Turrets};
pub use visuals::CombatVisualsPlugin;
pub use weapons::{Gun, Missile, MissileRack, Munition, Trigger, Weapon, gun_lead};

use crate::{
    flight::{FlightState, FlightSystems, KeyHelpAppExt, KeyHelpSection, PlayerAircraft},
    physics::{CrashDetection, EnemyCrashed, PlayerCrashed, ProjectileHit},
    world::damage::DamageArea,
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

/// Spheres around an aircraft's origin (m) for hits the physics colliders
/// would judge too strictly.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Hitbox {
    /// Missile proximity fuses go off within this plus their fuse radius.
    pub radius: f32,
    /// Gun rounds passing within this count as hits (0 = only the physics
    /// collider counts). For enemies about half the span: the colliders'
    /// wings are thin plates, so a round through the visible silhouette,
    /// wings included, often missed them.
    pub gun_radius: f32,
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
    /// Flew into something.
    Crash(CrashInto),
}

/// What an aircraft crashed into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashInto {
    Ground,
    Water,
    Building,
    Bridge,
    /// Another aircraft (ramming), by its type name.
    Aircraft(&'static str),
    /// A world collider of no known kind.
    Obstacle,
}

/// Type name of an aircraft ("Spitfire"), for messages such as "rammed by".
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AircraftType(pub &'static str);

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
    /// Arming the player, velocity sync.
    Input,
    /// Lock-on.
    Aim,
    /// Fire buttons, guns and missile launches. Gated by the round (no
    /// firing while paused or after the round is decided).
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
        let keys = app.world_mut().get_resource_or_init::<CombatKeys>().clone();
        app.add_key_help(
            KeyHelpSection::Weapons,
            &format!("{} / left mouse", key_labels(&keys.gun)),
            "machine gun",
        )
        .add_key_help(
            KeyHelpSection::Weapons,
            &format!("{} / right mouse", key_labels(&keys.missile)),
            "missile (homes on a locked target)",
        )
        .add_key_help(
            KeyHelpSection::Weapons,
            &key_labels(&keys.next_target),
            "next target (also automatic after a missile launch)",
        );
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
            .add_message::<BombImpact>()
            // The city's damage plugin registers it too.
            .add_message::<DamageArea>()
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
                    (arm_new_player, sync_player_velocity)
                        .chain()
                        .in_set(CombatSystems::Input),
                    lock::update_lock.in_set(CombatSystems::Aim),
                    // The player's buttons are read only when weapons may
                    // fire, so nothing pressed while paused acts later.
                    (
                        weapons::player_trigger,
                        lock::next_target,
                        (
                            weapons::fire_guns,
                            weapons::launch_missiles,
                            turrets::fire_turrets,
                        ),
                    )
                        .chain()
                        .in_set(CombatSystems::Fire),
                    (weapons::guide_missiles, bombs::fall_bombs).in_set(CombatSystems::Move),
                    (
                        hits::projectile_hits,
                        hits::proximity_hits,
                        hits::remember_positions,
                    )
                        .chain()
                        .in_set(CombatSystems::DetectHits),
                    hits::apply_hits.in_set(CombatSystems::Resolve),
                    // Shot down: hidden and exploding in this frame.
                    (wreck::wreck_player, wreck::restore_player)
                        .chain()
                        .after(CombatSystems::Resolve)
                        .in_set(wreck::WreckSystems),
                    (effects::spawn_effects, effects::age_effects)
                        .chain()
                        .after(CombatSystems::Resolve),
                ),
            )
            // Crashes are handled right where the physics reports them, so
            // the frame that shows the impact shows the explosion, not a
            // jet half inside a building (and enemies vanish just as fast).
            .add_systems(
                PostUpdate,
                (
                    hits::player_crashes,
                    hits::enemy_crashes,
                    wreck::wreck_player,
                )
                    .chain()
                    .after(CrashDetection)
                    .in_set(wreck::WreckSystems),
            );
    }
}

/// Combat components of a freshly armed player jet. Inserting them again
/// resets health, ammunition and lock (used when a round restarts).
pub fn player_loadout(cfg: &CombatConfig) -> impl Bundle {
    (
        Team::Player,
        Health::full(cfg.player_health),
        // Enemy rounds hit the jet's own collider only.
        Hitbox {
            radius: cfg.player_hitbox_radius,
            gun_radius: 0.0,
        },
        AircraftVelocity::default(),
        AircraftType("jet"),
        Gun::new(
            cfg.player_gun,
            (!cfg.player_gun_unlimited).then_some(cfg.player_gun_ammo),
        ),
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

/// "Space / Tab"-style labels of a key list, for help texts and the HUD.
pub fn key_labels(keys: &[KeyCode]) -> String {
    keys.iter()
        .map(|k| key_label(*k))
        .collect::<Vec<_>>()
        .join(" / ")
}
