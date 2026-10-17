//! Plays the [`SoundMix`] with Bevy's audio: one looping player per voice,
//! following its volume and pitch, and a despawning player per one-shot.

use bevy::{audio::Volume, platform::collections::HashMap, prelude::*};

use super::mix::{Clip, SoundMix, SoundMixSystems, Voice};

/// At most this many one-shots sound at once; more are dropped.
const MAX_ONE_SHOTS: usize = 12;

pub struct SoundPlaybackPlugin;

impl Plugin for SoundPlaybackPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_clips)
            .add_systems(PostUpdate, play.after(SoundMixSystems));
    }
}

#[derive(Resource)]
struct SoundBank(HashMap<Clip, Handle<AudioSource>>);

/// A running loop.
#[derive(Component)]
struct LoopVoice(Voice);

/// A running one-shot.
#[derive(Component)]
struct OneShotSound;

fn load_clips(mut commands: Commands, assets: Res<AssetServer>) {
    let clips = Clip::ALL
        .into_iter()
        .map(|clip| (clip, assets.load(clip.path())))
        .collect();
    commands.insert_resource(SoundBank(clips));
}

fn play(
    mut commands: Commands,
    mix: Res<SoundMix>,
    bank: Option<Res<SoundBank>>,
    mut loops: Query<(Entity, &LoopVoice, Option<&mut AudioSink>)>,
    shots: Query<(), With<OneShotSound>>,
) {
    let Some(bank) = bank else {
        return;
    };
    let mut running = Vec::new();
    for (entity, voice, sink) in &mut loops {
        match mix.voice(voice.0) {
            Some(wanted) => {
                running.push(voice.0);
                if let Some(mut sink) = sink {
                    sink.set_volume(Volume::Linear(wanted.volume));
                    sink.set_speed(wanted.speed);
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for wanted in mix.loops.iter().filter(|l| !running.contains(&l.voice)) {
        let Some(handle) = bank.0.get(&wanted.clip) else {
            continue;
        };
        commands.spawn((
            LoopVoice(wanted.voice),
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::LOOP
                .with_volume(Volume::Linear(wanted.volume))
                .with_speed(wanted.speed),
        ));
    }
    let room = MAX_ONE_SHOTS.saturating_sub(shots.iter().count());
    for shot in mix.shots.iter().take(room) {
        let Some(handle) = bank.0.get(&shot.clip) else {
            continue;
        };
        commands.spawn((
            OneShotSound,
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(shot.volume)),
        ));
    }
}
