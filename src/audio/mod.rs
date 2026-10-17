//! Sound. Gameplay messages become [`SoundCue`]s (headless hooks); the
//! mixer ([`mix`]) turns cues and the game's state into the loops and
//! one-shots that should sound now; the player ([`playback`]) plays them.

pub mod mix;
#[cfg(target_arch = "wasm32")]
mod playback;
#[cfg(test)]
mod tests;

use bevy::{platform::collections::HashMap, prelude::*};

pub use mix::{
    AudioUnlocked, Clip, LoopMix, MAX_BOMBER_VOICES, MAX_ZEPPELIN_VOICES, OneShot,
    SOUND_TOGGLE_KEY, SoundMix, SoundMixPlugin, SoundSettings, Voice,
};

use crate::{
    ai::{Enemy, EnemyKind},
    combat::{
        BombImpact, CombatSystems, Destroyed, DryFire, GoingDown, Hit, LockOn, LockState,
        ShotFired, Team, Weapon,
    },
    flight::{KeyHelpAppExt, KeyHelpSection, PlayerAircraft},
    game_state::{Outcome, Round, RoundPhase, RoundSystems},
};

/// Something worth a sound. `position` is the world position for spatial
/// sounds, `None` for cockpit and interface sounds.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct SoundCue {
    pub sound: Sound,
    pub position: Option<Vec3>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    PlayerGun,
    EnemyGun,
    MissileLaunch,
    /// The seeker starts tracking a target (lock tone, low).
    LockTracking,
    /// Target locked (lock tone, high).
    LockAcquired,
    /// Player's shot struck an enemy.
    HitEnemy,
    /// Player was hit.
    HitPlayer,
    /// The player's missile struck an enemy that survived.
    MissileHit,
    /// A fighter blows up.
    Explosion,
    /// A bomber, a zeppelin or the player's jet blows up.
    BigExplosion,
    /// A burning wreck hits the ground.
    WreckImpact,
    BombImpact,
    /// The player pulled the trigger on an empty gun ...
    GunEmpty,
    /// ... or asked for a missile with none left.
    MissileEmpty,
    Victory,
    Defeat,
}

pub struct SoundHooksPlugin;

/// Sound for the game: hooks, mixer, and playback where the build has it.
pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_key_help(KeyHelpSection::Game, "N", "sound on / off")
            .add_plugins((SoundHooksPlugin, SoundMixPlugin));
        #[cfg(target_arch = "wasm32")]
        app.add_plugins(playback::SoundPlaybackPlugin);
    }
}

impl Plugin for SoundHooksPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SoundCue>()
            .init_resource::<KnownEnemies>()
            // The combat plugin registers them too.
            .add_message::<BombImpact>()
            .add_message::<DryFire>()
            .add_systems(
                Update,
                (
                    // After the round spawned its waves, so an enemy downed in
                    // its first frame is known too.
                    (track_enemies, combat_cues)
                        .chain()
                        .after(CombatSystems::Resolve)
                        .after(RoundSystems::Track),
                    round_cues.after(RoundSystems::Track),
                ),
            );
    }
}

/// What each enemy is and where it was last, so its explosion still
/// knows its size when the entity is already gone.
#[derive(Resource, Default)]
struct KnownEnemies {
    kinds: HashMap<Entity, EnemyKind>,
    /// Wrecks on their way down, at their last position.
    falling: HashMap<Entity, Vec3>,
}

fn track_enemies(
    mut known: ResMut<KnownEnemies>,
    added: Query<(Entity, &Enemy), Added<Enemy>>,
    falling: Query<(Entity, &Transform), With<GoingDown>>,
) {
    for (entity, enemy) in &added {
        known.kinds.insert(entity, enemy.kind);
    }
    for (entity, transform) in &falling {
        known.falling.insert(entity, transform.translation);
    }
}

#[allow(clippy::too_many_arguments)]
fn combat_cues(
    mut shots: MessageReader<ShotFired>,
    mut hits: MessageReader<Hit>,
    mut destroyed: MessageReader<Destroyed>,
    mut bombs: MessageReader<BombImpact>,
    mut dry: MessageReader<DryFire>,
    jets: Query<(), With<PlayerAircraft>>,
    mut known: ResMut<KnownEnemies>,
    alive: Query<(), With<Enemy>>,
    lock: Query<&LockOn, (With<PlayerAircraft>, Changed<LockOn>)>,
    mut last_lock: Local<Option<std::mem::Discriminant<LockState>>>,
    mut cues: MessageWriter<SoundCue>,
) {
    for shot in shots.read() {
        let sound = match (shot.weapon, shot.team) {
            (Weapon::Missile, _) => Sound::MissileLaunch,
            (Weapon::Gun, Team::Player) => Sound::PlayerGun,
            (Weapon::Gun, Team::Enemy) => Sound::EnemyGun,
        };
        cues.write(SoundCue {
            sound,
            position: Some(shot.position),
        });
    }
    let destroyed: Vec<Destroyed> = destroyed.read().copied().collect();
    for hit in hits.read() {
        let sound = match hit.team {
            Team::Player => Sound::HitEnemy,
            Team::Enemy => Sound::HitPlayer,
        };
        cues.write(SoundCue {
            sound,
            position: Some(hit.point),
        });
        // A missile that did not bring its target down still bursts.
        let survived = !destroyed.iter().any(|d| d.entity == hit.target);
        if hit.team == Team::Player && hit.weapon == Weapon::Missile && survived {
            cues.write(SoundCue {
                sound: Sound::MissileHit,
                position: Some(hit.point),
            });
        }
    }
    for event in &destroyed {
        let big = match event.team {
            Team::Player => true,
            Team::Enemy => known
                .kinds
                .get(&event.entity)
                .is_some_and(|kind| kind.is_bomber()),
        };
        cues.write(SoundCue {
            sound: if big {
                Sound::BigExplosion
            } else {
                Sound::Explosion
            },
            position: Some(event.position),
        });
    }
    for event in dry.read().filter(|d| jets.contains(d.shooter)) {
        cues.write(SoundCue {
            sound: match event.weapon {
                Weapon::Gun => Sound::GunEmpty,
                Weapon::Missile => Sound::MissileEmpty,
            },
            position: None,
        });
    }
    for bomb in bombs.read() {
        cues.write(SoundCue {
            sound: Sound::BombImpact,
            position: Some(bomb.point),
        });
    }
    // A wreck that is gone has hit the ground.
    let landed: Vec<(Entity, Vec3)> = known
        .falling
        .iter()
        .filter(|(entity, _)| !alive.contains(**entity))
        .map(|(entity, at)| (*entity, *at))
        .collect();
    for (entity, at) in landed {
        known.falling.remove(&entity);
        cues.write(SoundCue {
            sound: Sound::WreckImpact,
            position: Some(at),
        });
    }
    known.kinds.retain(|entity, _| alive.contains(*entity));
    // Lock tones only when the kind of lock state changes, not every
    // frame of tracking progress.
    if let Ok(lock) = lock.single() {
        let kind = std::mem::discriminant(&lock.state);
        if *last_lock != Some(kind) {
            *last_lock = Some(kind);
            let sound = match lock.state {
                LockState::Searching => None,
                LockState::Tracking { .. } => Some(Sound::LockTracking),
                LockState::Locked { .. } => Some(Sound::LockAcquired),
            };
            if let Some(sound) = sound {
                cues.write(SoundCue {
                    sound,
                    position: None,
                });
            }
        }
    }
}

fn round_cues(
    round: Res<Round>,
    mut last: Local<Option<RoundPhase>>,
    mut cues: MessageWriter<SoundCue>,
) {
    if !round.is_changed() {
        return;
    }
    let ending = |phase: Option<RoundPhase>| matches!(phase, Some(RoundPhase::Ending { .. }));
    if ending(Some(round.phase)) && !ending(*last) {
        let sound = match round.outcome() {
            Some(Outcome::Victory) => Sound::Victory,
            _ => Sound::Defeat,
        };
        cues.write(SoundCue {
            sound,
            position: None,
        });
    }
    *last = Some(round.phase);
}
