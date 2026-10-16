//! Sound hooks. No playback yet: gameplay messages are translated into
//! [`SoundCue`]s, so the later audio player only has to read those.

use bevy::prelude::*;

use crate::{
    combat::{CombatSystems, Destroyed, Hit, LockOn, LockState, ShotFired, Team, Weapon},
    flight::PlayerAircraft,
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
    Explosion,
    Victory,
    Defeat,
}

pub struct SoundHooksPlugin;

impl Plugin for SoundHooksPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SoundCue>().add_systems(
            Update,
            (
                combat_cues.after(CombatSystems::Resolve),
                round_cues.after(RoundSystems),
            ),
        );
    }
}

fn combat_cues(
    mut shots: MessageReader<ShotFired>,
    mut hits: MessageReader<Hit>,
    mut destroyed: MessageReader<Destroyed>,
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
    for hit in hits.read() {
        let sound = match hit.team {
            Team::Player => Sound::HitEnemy,
            Team::Enemy => Sound::HitPlayer,
        };
        cues.write(SoundCue {
            sound,
            position: Some(hit.point),
        });
    }
    for event in destroyed.read() {
        cues.write(SoundCue {
            sound: Sound::Explosion,
            position: Some(event.position),
        });
    }
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
