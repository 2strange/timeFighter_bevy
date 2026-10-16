use bevy::prelude::*;

/// A gun's ballistics. Arcade: rounds fly straight (no drop), inherit the
/// shooter's velocity and scatter in a small cone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GunSpec {
    /// Rounds per second while the trigger is held.
    pub rate: f32,
    /// Muzzle velocity relative to the shooter (m/s).
    pub muzzle_speed: f32,
    /// Half-angle of the scatter cone (rad).
    pub spread: f32,
    pub damage: f32,
    /// Seconds before a round vanishes; with the muzzle speed this sets
    /// the range.
    pub lifetime: f32,
    /// Rounds leave this far ahead of the aircraft's origin (m), clear of
    /// its own nose.
    pub muzzle_offset: f32,
    /// Collision radius of a round (m). A little larger than a real one:
    /// arcade forgiveness.
    pub radius: f32,
}

impl GunSpec {
    /// The jet's cannon: fast and tight, 1.3 s of flight (about 1.6 km).
    /// Three hits bring down a Spitfire. Faster rounds need less lead, so
    /// fast WWII fighters stay hittable.
    pub const JET_CANNON: GunSpec = GunSpec {
        rate: 20.0,
        muzzle_speed: 1_200.0,
        spread: 0.3_f32 * (std::f32::consts::PI / 180.0),
        damage: 17.0,
        lifetime: 1.3,
        muzzle_offset: 12.0,
        radius: 0.5,
    };

    /// WWII fighter guns: slower rounds, wider scatter, little damage each.
    pub const WWII_GUNS: GunSpec = GunSpec {
        rate: 10.0,
        muzzle_speed: 750.0,
        spread: 1.2_f32 * (std::f32::consts::PI / 180.0),
        damage: 3.0,
        lifetime: 1.4,
        muzzle_offset: 6.0,
        radius: 0.4,
    };

    pub fn interval(&self) -> f32 {
        1.0 / self.rate
    }
}

/// Guided missile: boosts to top speed, steers by proportional navigation
/// with a turn limit, detonates by proximity fuse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MissileSpec {
    /// Speed added at launch on top of the jet's speed (m/s).
    pub launch_kick: f32,
    pub max_speed: f32,
    /// Motor acceleration until `max_speed` (m/s²).
    pub boost: f32,
    /// Largest sideways acceleration (m/s²); limits how tight it turns.
    pub max_lateral_accel: f32,
    /// Proportional navigation constant (3–5 is usual).
    pub nav_gain: f32,
    /// Seconds before it self-destructs.
    pub lifetime: f32,
    /// Detonates when an enemy's hitbox comes this close (m).
    pub fuse_radius: f32,
    /// Collision radius of the missile body (m).
    pub radius: f32,
    pub damage: f32,
    /// Leaves this far below and ahead of the jet's origin (m).
    pub launch_offset: Vec3,
    /// Seconds between two launches.
    pub reload: f32,
}

/// When a target gets locked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockSpec {
    /// Half-angle of the cone around the nose a target must be in (rad).
    pub cone: f32,
    /// Longest lock distance (m).
    pub range: f32,
    /// Seconds a target must stay in the cone until it is locked.
    pub time: f32,
    /// A held lock tolerates this much more angle and range, so it does
    /// not flicker at the edge.
    pub hold_factor: f32,
}

/// Tunables of combat, all in metres, seconds, radians.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CombatConfig {
    pub player_health: f32,
    pub player_hitbox_radius: f32,
    pub player_gun: GunSpec,
    pub player_gun_ammo: u32,
    /// The gun never runs dry (a difficulty option).
    pub player_gun_unlimited: bool,
    pub player_missiles: u32,
    pub missile: MissileSpec,
    pub lock: LockSpec,
}

impl Default for CombatConfig {
    fn default() -> Self {
        Self {
            player_health: 100.0,
            player_hitbox_radius: 6.0,
            player_gun: GunSpec::JET_CANNON,
            // 30 s of continuous fire.
            player_gun_ammo: 600,
            player_gun_unlimited: false,
            player_missiles: 6,
            missile: MissileSpec {
                launch_kick: 40.0,
                max_speed: 750.0,
                boost: 350.0,
                // About 25 g: twice what a turning Spitfire pulls, so a
                // well-aimed missile mostly hits, a late break can still
                // shake it.
                max_lateral_accel: 250.0,
                nav_gain: 4.0,
                lifetime: 8.0,
                fuse_radius: 8.0,
                radius: 0.3,
                damage: 100.0,
                launch_offset: Vec3::new(0.0, -2.0, -6.0),
                reload: 0.6,
            },
            lock: LockSpec {
                cone: 20_f32.to_radians(),
                range: 2_500.0,
                time: 1.0,
                hold_factor: 1.25,
            },
        }
    }
}

/// Weapon buttons for two-handed flying without moving a hand: left hand
/// on WASD/QE, right hand on the arrow keys.
/// - Gun: Space under the left thumb.
/// - Missile: Right Shift under the right pinky, next to the arrow keys.
/// - Next target: Tab under the left pinky (next to Q), or Right Alt/Option
///   under the right thumb, so either hand can switch targets. The page
///   suppresses the browser's own Tab handling.
///
/// Mouse buttons work for gun and missile as well.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CombatKeys {
    pub gun: Vec<KeyCode>,
    pub missile: Vec<KeyCode>,
    pub next_target: Vec<KeyCode>,
    pub gun_mouse: Vec<MouseButton>,
    pub missile_mouse: Vec<MouseButton>,
}

impl Default for CombatKeys {
    fn default() -> Self {
        Self {
            gun: vec![KeyCode::Space],
            missile: vec![KeyCode::ShiftRight],
            next_target: vec![KeyCode::Tab, KeyCode::AltRight],
            gun_mouse: vec![MouseButton::Left],
            missile_mouse: vec![MouseButton::Right],
        }
    }
}

/// Short label of a key as printed on Mac and PC keyboards.
pub fn key_label(key: KeyCode) -> String {
    match key {
        KeyCode::Space => "Space".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::ShiftRight => "R-Shift".into(),
        KeyCode::ShiftLeft => "L-Shift".into(),
        KeyCode::AltRight => "R-Alt".into(),
        KeyCode::AltLeft => "L-Alt".into(),
        KeyCode::ControlRight => "R-Ctrl".into(),
        KeyCode::ControlLeft => "L-Ctrl".into(),
        other => {
            let name = format!("{other:?}");
            name.strip_prefix("Key")
                .or_else(|| name.strip_prefix("Digit"))
                .unwrap_or(&name)
                .to_string()
        }
    }
}
