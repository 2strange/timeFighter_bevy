//! The mixer: from the game's state and the [`SoundCue`]s of a frame, the
//! loops that should run now (with volume and pitch) and the one-shots to
//! start. Headless; the player only follows [`SoundMix`].
//!
//! Nothing sounds before the first key press or click (browsers allow
//! audio only after one), while the sound is switched off, or in the
//! player's pause. Music plays only in the menu and on the result screen,
//! quietly; everything else only in flight.

use bevy::prelude::*;

use super::{Sound, SoundCue};
use crate::{
    ai::{Enemy, EnemyKind},
    combat::{GoingDown, LockOn, LockState, Missile},
    flight::{FlightConfig, FlightInput, FlightState, PlayerAircraft},
    game_state::{Pause, Round},
};

/// Switches the sound on and off.
pub const SOUND_TOGGLE_KEY: KeyCode = KeyCode::KeyN;

/// At most this many bombers are heard at once, the closest ...
pub const MAX_BOMBER_VOICES: usize = 4;
/// ... and this many zeppelins.
pub const MAX_ZEPPELIN_VOICES: usize = 2;

/// Music volume: very quiet, under the menu.
const MUSIC_VOLUME: f32 = 0.12;
/// Distance at which a sound is at half volume (m).
const HALF_VOLUME_DISTANCE: f32 = 400.0;
/// Quieter than this is not played at all.
const AUDIBLE: f32 = 0.02;
/// The gun loop runs this long after the last round (s).
const GUN_HOLD: f32 = 0.12;
/// Enemy gun bursts are heard at most this often (s).
const ENEMY_GUN_INTERVAL: f32 = 0.25;

/// Every sound file, one per slot. A missing file only stays silent, so a
/// slot can be filled in later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clip {
    Engine,
    Afterburner,
    Gun,
    Missile,
    LockSearch,
    LockTone,
    Warning,
    ExplosionSmall,
    ExplosionSmallLong,
    ExplosionBig,
    Bomber,
    Zeppelin,
    Bomb,
    Click,
    Music,
}

impl Clip {
    pub const ALL: [Clip; 15] = [
        Clip::Engine,
        Clip::Afterburner,
        Clip::Gun,
        Clip::Missile,
        Clip::LockSearch,
        Clip::LockTone,
        Clip::Warning,
        Clip::ExplosionSmall,
        Clip::ExplosionSmallLong,
        Clip::ExplosionBig,
        Clip::Bomber,
        Clip::Zeppelin,
        Clip::Bomb,
        Clip::Click,
        Clip::Music,
    ];

    /// Asset path. Short loops are WAV: compressed loops click at the seam.
    pub fn path(self) -> &'static str {
        match self {
            Clip::Engine => "audio/engine_turbine.wav",
            Clip::Afterburner => "audio/afterburner_kenney_thruster.wav",
            Clip::Gun => "audio/gun_fast.ogg",
            Clip::Missile => "audio/missile_whoosh.ogg",
            Clip::LockSearch => "audio/lock_search_double.wav",
            Clip::LockTone => "audio/lock_tone_warble.wav",
            Clip::Warning => "audio/warning_rapid.wav",
            Clip::ExplosionSmall => "audio/explosion_small_kenney_crunch.ogg",
            Clip::ExplosionSmallLong => "audio/explosion_small_kenney_crunch_long.ogg",
            Clip::ExplosionBig => "audio/explosion_big_boom.ogg",
            Clip::Bomber => "audio/bomber_rough.wav",
            Clip::Zeppelin => "audio/zeppelin_putter.wav",
            Clip::Bomb => "audio/bomb_kenney_low.ogg",
            Clip::Click => "audio/click_kenney_click_002.ogg",
            Clip::Music => "audio/music_drive.ogg",
        }
    }
}

/// The player's sound settings (the menu stores them).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SoundSettings {
    pub enabled: bool,
    /// Master volume, 0–1.
    pub volume: f32,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.8,
        }
    }
}

impl SoundSettings {
    /// The master volume as it applies (0 when off).
    pub fn master(&self) -> f32 {
        if self.enabled && self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Set by the first key press or click: from then on the browser lets
/// the page play sound.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioUnlocked(pub bool);

/// What a loop belongs to; the player keeps one playing loop per voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Voice {
    Engine,
    Afterburner,
    Gun,
    LockSearch,
    LockTone,
    Warning,
    Music,
    /// The engines of this bomber or zeppelin.
    Aircraft(Entity),
}

/// A loop that should run now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopMix {
    pub voice: Voice,
    pub clip: Clip,
    /// Final volume, master included.
    pub volume: f32,
    /// Playback speed (1 = as recorded; also the pitch).
    pub speed: f32,
}

/// A sound to start once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OneShot {
    pub clip: Clip,
    /// Final volume, master included.
    pub volume: f32,
}

/// This frame's mix.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SoundMix {
    pub loops: Vec<LoopMix>,
    /// Started this frame; the player takes them.
    pub shots: Vec<OneShot>,
}

impl SoundMix {
    pub fn voice(&self, voice: Voice) -> Option<&LoopMix> {
        self.loops.iter().find(|l| l.voice == voice)
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SoundMixSystems;

/// The mixer, headless. Reads the cues of [`super::SoundHooksPlugin`].
pub struct SoundMixPlugin;

impl Plugin for SoundMixPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundSettings>()
            .init_resource::<AudioUnlocked>()
            .init_resource::<SoundMix>()
            .add_message::<SoundCue>()
            .add_systems(
                PostUpdate,
                (unlock_and_toggle, mix).chain().in_set(SoundMixSystems),
            );
    }
}

/// Any key or click unlocks; the toggle key switches the sound.
fn unlock_and_toggle(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut unlocked: ResMut<AudioUnlocked>,
    mut settings: ResMut<SoundSettings>,
) {
    let key = keys
        .as_ref()
        .is_some_and(|k| k.get_just_pressed().next().is_some());
    let click = mouse.is_some_and(|m| m.get_just_pressed().next().is_some());
    if (key || click) && !unlocked.0 {
        unlocked.0 = true;
    }
    if keys.is_some_and(|k| k.just_pressed(SOUND_TOGGLE_KEY)) {
        settings.enabled = !settings.enabled;
    }
}

/// Volume of a sound `distance` metres away (1 close by).
pub fn attenuation(distance: f32) -> f32 {
    let d = distance / HALF_VOLUME_DISTANCE;
    1.0 / (1.0 + d * d)
}

/// Remembered between frames.
#[derive(Default)]
struct MixState {
    gun_hold: f32,
    enemy_gun_rest: f32,
    /// Alternates the two small explosions.
    explosions: u32,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn mix(
    time: Res<Time<Real>>,
    settings: Res<SoundSettings>,
    unlocked: Res<AudioUnlocked>,
    pause: Option<Res<Pause>>,
    round: Option<Res<Round>>,
    flight: Option<Res<FlightConfig>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut cues: MessageReader<SoundCue>,
    player: Query<
        (
            Entity,
            &Transform,
            Option<&FlightState>,
            Option<&FlightInput>,
            Option<&LockOn>,
        ),
        With<PlayerAircraft>,
    >,
    engines: Query<(Entity, &Enemy, &Transform), Without<GoingDown>>,
    missiles: Query<&Missile>,
    mut state: Local<MixState>,
    mut out: ResMut<SoundMix>,
) {
    let dt = time.delta_secs();
    out.loops.clear();
    out.shots.clear();
    let cues: Vec<SoundCue> = cues.read().copied().collect();
    let master = if unlocked.0 { settings.master() } else { 0.0 };
    let pause = pause.map(|p| *p).unwrap_or_default();
    let menu = pause.menu || pause.round_over;
    let flying =
        !pause.is_paused() && round.is_some_and(|r| r.is_running() || r.outcome().is_some());
    state.gun_hold -= dt;
    state.enemy_gun_rest -= dt;
    if master <= 0.0 {
        return;
    }

    if menu {
        out.loops.push(LoopMix {
            voice: Voice::Music,
            clip: Clip::Music,
            volume: master * MUSIC_VOLUME,
            speed: 1.0,
        });
        // A click for every key or button press in the menu.
        if pause.menu && keys.is_some_and(|k| k.get_just_pressed().next().is_some()) {
            out.shots.push(OneShot {
                clip: Clip::Click,
                volume: master * 0.6,
            });
        }
    }
    if !flying {
        return;
    }

    let Ok((jet, transform, state_now, input, lock)) = player.single() else {
        return;
    };
    let listener = transform.translation;
    let heard = |at: Option<Vec3>| at.map_or(1.0, |p| attenuation(p.distance(listener)));

    // The engine sings higher with speed; the afterburner joins while the
    // throttle is pushed.
    let speed = match (state_now, &flight) {
        (Some(s), Some(cfg)) => {
            ((s.speed - cfg.speed_min) / (cfg.speed_max - cfg.speed_min).max(1.0)).clamp(0.0, 1.0)
        }
        _ => 0.5,
    };
    let push = input.map_or(0.0, |i| i.throttle.clamp(0.0, 1.0));
    out.loops.push(LoopMix {
        voice: Voice::Engine,
        clip: Clip::Engine,
        volume: master * (0.25 + 0.2 * speed + 0.1 * push),
        speed: 0.8 + 0.6 * speed,
    });
    if push > 0.0 {
        out.loops.push(LoopMix {
            voice: Voice::Afterburner,
            clip: Clip::Afterburner,
            volume: master * 0.5 * push,
            speed: 1.0,
        });
    }

    // Seeker: a search tone while a target is in the cone, the lock tone
    // once locked.
    match lock.map(|l| l.state) {
        Some(LockState::Tracking { .. }) => out.loops.push(LoopMix {
            voice: Voice::LockSearch,
            clip: Clip::LockSearch,
            volume: master * 0.35,
            speed: 1.0,
        }),
        Some(LockState::Locked { .. }) => out.loops.push(LoopMix {
            voice: Voice::LockTone,
            clip: Clip::LockTone,
            volume: master * 0.4,
            speed: 1.0,
        }),
        _ => {}
    }
    if missiles.iter().any(|m| m.target == Some(jet)) {
        out.loops.push(LoopMix {
            voice: Voice::Warning,
            clip: Clip::Warning,
            volume: master * 0.5,
            speed: 1.0,
        });
    }

    // The closest bombers and zeppelins drone.
    let mut drones: Vec<(f32, Entity, EnemyKind)> = engines
        .iter()
        .filter(|(_, e, _)| e.kind.is_bomber())
        .map(|(entity, e, tf)| (tf.translation.distance(listener), entity, e.kind))
        .collect();
    drones.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut bombers, mut zeppelins) = (0, 0);
    for (distance, entity, kind) in drones {
        let (count, max, clip, loudness) = if kind == EnemyKind::Zeppelin {
            (&mut zeppelins, MAX_ZEPPELIN_VOICES, Clip::Zeppelin, 0.9)
        } else {
            (&mut bombers, MAX_BOMBER_VOICES, Clip::Bomber, 0.6)
        };
        let volume = master * loudness * attenuation(distance);
        if *count >= max || volume < AUDIBLE {
            continue;
        }
        *count += 1;
        out.loops.push(LoopMix {
            voice: Voice::Aircraft(entity),
            clip,
            volume,
            speed: 1.0,
        });
    }

    // One-shots.
    let mut enemy_gun: Option<f32> = None;
    for cue in &cues {
        let shot = |clip, loudness: f32| OneShot {
            clip,
            volume: master * loudness * heard(cue.position),
        };
        match cue.sound {
            Sound::PlayerGun => state.gun_hold = GUN_HOLD,
            Sound::EnemyGun => {
                let v = heard(cue.position);
                enemy_gun = Some(enemy_gun.map_or(v, |e: f32| e.max(v)));
            }
            Sound::MissileLaunch => out.shots.push(shot(Clip::Missile, 0.7)),
            Sound::Explosion => {
                state.explosions += 1;
                let clip = if state.explosions.is_multiple_of(2) {
                    Clip::ExplosionSmall
                } else {
                    Clip::ExplosionSmallLong
                };
                out.shots.push(shot(clip, 0.9));
            }
            Sound::BigExplosion | Sound::WreckImpact => {
                out.shots.push(shot(Clip::ExplosionBig, 1.0));
            }
            Sound::BombImpact => out.shots.push(shot(Clip::Bomb, 0.8)),
            // Tones and hits are loops or have no sound of their own.
            Sound::LockTracking
            | Sound::LockAcquired
            | Sound::HitEnemy
            | Sound::HitPlayer
            | Sound::Victory
            | Sound::Defeat => {}
        }
    }
    if let Some(v) = enemy_gun
        && state.enemy_gun_rest <= 0.0
    {
        state.enemy_gun_rest = ENEMY_GUN_INTERVAL;
        out.shots.push(OneShot {
            clip: Clip::Gun,
            volume: master * 0.5 * v,
        });
    }
    if state.gun_hold > 0.0 {
        out.loops.push(LoopMix {
            voice: Voice::Gun,
            clip: Clip::Gun,
            volume: master * 0.55,
            speed: 1.0,
        });
    }
    out.shots.retain(|s| s.volume >= AUDIBLE);
}
