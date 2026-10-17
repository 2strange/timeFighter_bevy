//! Headless tests of the mixer: what should sound when.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy};

use super::{
    Clip, MAX_BOMBER_VOICES, MAX_ZEPPELIN_VOICES, OneShot, SOUND_TOGGLE_KEY, Sound, SoundCue,
    SoundMix, SoundMixPlugin, SoundSettings, Voice, mix::attenuation,
};
use crate::{
    ai::{Enemy, EnemyKind},
    combat::{CombatConfig, LockOn, LockState, Missile, MissileRack},
    flight::{FlightConfig, FlightInput, FlightState, PlayerAircraft},
    game_state::{Pause, Round, RoundPhase},
};

const FRAME: Duration = Duration::from_nanos(16_666_667);

/// A round in flight, the player's jet at the origin; sound not yet
/// unlocked.
fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SoundMixPlugin))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .insert_resource(FlightConfig::default())
        .insert_resource(Pause::default())
        .insert_resource(Round {
            phase: RoundPhase::Running,
            ..default()
        });
    let jet = app
        .world_mut()
        .spawn((
            PlayerAircraft,
            Transform::default(),
            FlightState {
                speed: 50.0,
                ..default()
            },
            FlightInput::default(),
            LockOn::default(),
        ))
        .id();
    app.update();
    (app, jet)
}

/// Presses and releases `key` within one frame.
fn tap(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

fn unlocked_app() -> (App, Entity) {
    let (mut app, jet) = app();
    tap(&mut app, KeyCode::Enter);
    (app, jet)
}

fn mix(app: &App) -> SoundMix {
    app.world().resource::<SoundMix>().clone()
}

fn cue(app: &mut App, sound: Sound, position: Option<Vec3>) {
    app.world_mut().write_message(SoundCue { sound, position });
}

#[test]
fn nothing_sounds_before_the_first_key_or_click() {
    let (mut app, _) = app();
    app.update();
    assert_eq!(mix(&app), SoundMix::default());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(mix(&app).voice(Voice::Engine).is_some(), "a click unlocks");
}

#[test]
fn the_toggle_key_switches_everything_off_and_on() {
    let (mut app, _) = unlocked_app();
    assert!(mix(&app).voice(Voice::Engine).is_some());
    tap(&mut app, SOUND_TOGGLE_KEY);
    assert!(!app.world().resource::<SoundSettings>().enabled);
    assert!(mix(&app).loops.is_empty());
    cue(&mut app, Sound::BigExplosion, None);
    app.update();
    assert!(mix(&app).shots.is_empty(), "no one-shots either");
    tap(&mut app, SOUND_TOGGLE_KEY);
    assert!(mix(&app).voice(Voice::Engine).is_some());

    // The volume scales everything.
    let loud = mix(&app).voice(Voice::Engine).unwrap().volume;
    app.world_mut().resource_mut::<SoundSettings>().volume = 0.4;
    app.update();
    let quiet = mix(&app).voice(Voice::Engine).unwrap().volume;
    assert!((quiet / loud - 0.5).abs() < 1e-3, "{loud} {quiet}");
}

#[test]
fn the_engine_follows_speed_and_the_afterburner_the_throttle() {
    let (mut app, jet) = unlocked_app();
    let slow = *mix(&app).voice(Voice::Engine).unwrap();
    assert!(mix(&app).voice(Voice::Afterburner).is_none());
    app.world_mut().get_mut::<FlightState>(jet).unwrap().speed = 450.0;
    app.world_mut()
        .get_mut::<FlightInput>(jet)
        .unwrap()
        .throttle = 1.0;
    app.update();
    let fast = *mix(&app).voice(Voice::Engine).unwrap();
    assert!(fast.speed > slow.speed + 0.3, "higher pitch");
    assert!(fast.volume > slow.volume, "louder");
    assert_eq!(
        mix(&app).voice(Voice::Afterburner).map(|l| l.clip),
        Some(Clip::Afterburner)
    );
}

#[test]
fn seeker_tones_and_the_missile_warning() {
    let (mut app, jet) = unlocked_app();
    let target = app.world_mut().spawn_empty().id();
    let tones = |app: &App| {
        let m = mix(app);
        (
            m.voice(Voice::LockSearch).is_some(),
            m.voice(Voice::LockTone).is_some(),
        )
    };
    assert_eq!(tones(&app), (false, false));
    app.world_mut().get_mut::<LockOn>(jet).unwrap().state = LockState::Tracking {
        target,
        progress: 0.3,
    };
    app.update();
    assert_eq!(tones(&app), (true, false), "searching tone");
    app.world_mut().get_mut::<LockOn>(jet).unwrap().state = LockState::Locked { target };
    app.update();
    assert_eq!(tones(&app), (false, true), "lock tone");

    assert!(mix(&app).voice(Voice::Warning).is_none());
    let missile = app
        .world_mut()
        .spawn(Missile {
            spec: CombatConfig::default().missile,
            target: Some(jet),
            speed: 300.0,
        })
        .id();
    app.update();
    assert!(mix(&app).voice(Voice::Warning).is_some(), "missile on us");
    app.world_mut().despawn(missile);
    app.update();
    assert!(mix(&app).voice(Voice::Warning).is_none());
}

#[test]
fn only_the_closest_bombers_and_zeppelins_are_heard() {
    let (mut app, _) = unlocked_app();
    let mut bombers = Vec::new();
    for i in 0..16 {
        let at = Vec3::new(0.0, 500.0, -(200.0 + 150.0 * i as f32));
        bombers.push(
            app.world_mut()
                .spawn((
                    Enemy {
                        kind: EnemyKind::He177,
                    },
                    Transform::from_translation(at),
                ))
                .id(),
        );
    }
    for i in 0..3 {
        app.world_mut().spawn((
            Enemy {
                kind: EnemyKind::Zeppelin,
            },
            Transform::from_xyz(300.0 * (i + 1) as f32, 600.0, 0.0),
        ));
    }
    // Fighters have no drone of their own.
    app.world_mut().spawn((
        Enemy {
            kind: EnemyKind::Bf109K4,
        },
        Transform::from_xyz(0.0, 0.0, -50.0),
    ));
    app.update();
    let m = mix(&app);
    let heard = |clip| m.loops.iter().filter(|l| l.clip == clip).count();
    assert_eq!(heard(Clip::Bomber), MAX_BOMBER_VOICES);
    assert_eq!(heard(Clip::Zeppelin), MAX_ZEPPELIN_VOICES);
    for close in &bombers[..MAX_BOMBER_VOICES] {
        assert!(m.voice(Voice::Aircraft(*close)).is_some(), "the closest");
    }
    let near = m.voice(Voice::Aircraft(bombers[0])).unwrap().volume;
    let farther = m.voice(Voice::Aircraft(bombers[3])).unwrap().volume;
    assert!(near > farther, "fades with distance");
}

#[test]
fn pause_is_silent_and_the_menu_plays_quiet_music() {
    let (mut app, _) = unlocked_app();
    app.world_mut().resource_mut::<Pause>().player = true;
    app.update();
    assert_eq!(mix(&app), SoundMix::default(), "the pause is silent");

    app.world_mut().resource_mut::<Pause>().player = false;
    app.world_mut().resource_mut::<Pause>().menu = true;
    app.update();
    let m = mix(&app);
    assert_eq!(m.loops.len(), 1, "only the music: {m:?}");
    let music = m.voice(Voice::Music).unwrap();
    assert!(music.volume < 0.15, "very quiet: {}", music.volume);
    // A click as the menu's choice changes (not when it appears).
    app.insert_resource(crate::menu::MenuSelection::default());
    app.update();
    assert!(mix(&app).shots.is_empty());
    app.world_mut()
        .resource_mut::<crate::menu::MenuSelection>()
        .set_changed();
    app.update();
    assert_eq!(
        mix(&app).shots.iter().map(|s| s.clip).collect::<Vec<_>>(),
        vec![Clip::Click]
    );

    // The result screen keeps the music; flying again stops it.
    app.world_mut().resource_mut::<Pause>().menu = false;
    app.world_mut().resource_mut::<Pause>().round_over = true;
    app.update();
    assert!(mix(&app).voice(Voice::Music).is_some());
    assert!(mix(&app).voice(Voice::Engine).is_none());
    app.world_mut().resource_mut::<Pause>().round_over = false;
    app.update();
    assert!(mix(&app).voice(Voice::Music).is_none());
    assert!(mix(&app).voice(Voice::Engine).is_some());
}

#[test]
fn explosions_bombs_and_guns() {
    let (mut app, _) = unlocked_app();
    let shots = |app: &App| -> Vec<OneShot> { mix(app).shots };
    // Small explosions take turns between the two sounds.
    let mut small = Vec::new();
    for _ in 0..2 {
        cue(&mut app, Sound::Explosion, Some(Vec3::ZERO));
        app.update();
        small.push(shots(&app)[0].clip);
    }
    small.sort_by_key(|c| *c == Clip::ExplosionSmall);
    assert_eq!(small, vec![Clip::ExplosionSmallLong, Clip::ExplosionSmall]);

    for (sound, clip) in [
        (Sound::BigExplosion, Clip::ExplosionBig),
        (Sound::WreckImpact, Clip::ExplosionBig),
        (Sound::BombImpact, Clip::Bomb),
        (Sound::MissileLaunch, Clip::Missile),
    ] {
        cue(&mut app, sound, Some(Vec3::new(0.0, 0.0, -100.0)));
        app.update();
        assert_eq!(
            shots(&app).iter().map(|s| s.clip).collect::<Vec<_>>(),
            vec![clip]
        );
    }
    // Far away is quieter, very far not played at all.
    cue(
        &mut app,
        Sound::BombImpact,
        Some(Vec3::new(0.0, 0.0, -1_000.0)),
    );
    app.update();
    let far = shots(&app)[0].volume;
    assert!(far < 0.8 * 0.8 * attenuation(500.0));
    cue(
        &mut app,
        Sound::BombImpact,
        Some(Vec3::new(0.0, 0.0, -20_000.0)),
    );
    app.update();
    assert!(shots(&app).is_empty());

    // Hits on the jet rattle, a few per second at most.
    let mut hits = 0;
    let mut pitches: Vec<f32> = Vec::new();
    for _ in 0..30 {
        cue(&mut app, Sound::HitPlayer, Some(Vec3::new(0.0, 0.0, -5.0)));
        cue(&mut app, Sound::HitPlayer, Some(Vec3::new(0.0, 0.0, -5.0)));
        app.update();
        for hit in shots(&app).iter().filter(|s| s.clip == Clip::PlayerHit) {
            hits += 1;
            pitches.push(hit.speed);
        }
    }
    let most = (30.0 * FRAME.as_secs_f32() / super::mix::PLAYER_HIT_INTERVAL).ceil() as usize;
    assert!(hits >= 2 && hits <= most, "{hits} of at most {most}");
    assert!(pitches.windows(2).all(|p| p[0] != p[1]), "{pitches:?}");
    assert!(pitches.iter().all(|p| (p - 1.0).abs() <= 0.08));

    // A volley of enemy rounds: one gun sound.
    for _ in 0..30 {
        cue(&mut app, Sound::EnemyGun, Some(Vec3::new(0.0, 0.0, -200.0)));
    }
    app.update();
    assert_eq!(shots(&app).len(), 1);

    // The player's gun runs as a loop while it fires, and stops after.
    for _ in 0..10 {
        cue(&mut app, Sound::PlayerGun, None);
        app.update();
        assert!(mix(&app).voice(Voice::Gun).is_some());
    }
    for _ in 0..12 {
        app.update();
    }
    assert!(mix(&app).voice(Voice::Gun).is_none());
}

#[test]
fn without_missiles_a_new_target_gets_one_beep_and_then_quiet() {
    let (mut app, jet) = unlocked_app();
    let (first, second) = (
        app.world_mut().spawn_empty().id(),
        app.world_mut().spawn_empty().id(),
    );
    app.world_mut()
        .entity_mut(jet)
        .insert(MissileRack::new(CombatConfig::default().missile, 0));
    let seek = |app: &mut App, state| {
        app.world_mut().get_mut::<LockOn>(jet).unwrap().state = state;
        app.update();
        let m = mix(app);
        let beeps = m
            .shots
            .iter()
            .filter(|s| s.clip == Clip::SearchBeep)
            .count();
        let tones = m.voice(Voice::LockSearch).is_some() || m.voice(Voice::LockTone).is_some();
        (beeps, tones)
    };
    let tracking = |target| LockState::Tracking {
        target,
        progress: 0.5,
    };
    assert_eq!(seek(&mut app, tracking(first)), (1, false), "one beep");
    assert_eq!(seek(&mut app, tracking(first)), (0, false), "then quiet");
    assert_eq!(
        seek(&mut app, LockState::Locked { target: first }),
        (0, false),
        "no lock tone"
    );
    assert_eq!(seek(&mut app, tracking(second)), (1, false), "a new one");
    assert_eq!(seek(&mut app, LockState::Searching), (0, false));
    assert_eq!(seek(&mut app, tracking(second)), (1, false), "back in");
    assert_eq!(Clip::SearchBeep.max_length(), Some(0.1), "short");

    // With missiles: the tones as ever, no beep.
    app.world_mut().get_mut::<MissileRack>(jet).unwrap().count = 2;
    assert_eq!(seek(&mut app, tracking(first)), (0, true));
    assert!(mix(&app).voice(Voice::LockSearch).is_some());
    seek(&mut app, LockState::Locked { target: first });
    assert!(mix(&app).voice(Voice::LockTone).is_some());
}

#[test]
fn empty_weapons_answer_with_a_refusal_and_a_dry_click() {
    let (mut app, _) = unlocked_app();
    cue(&mut app, Sound::MissileEmpty, None);
    app.update();
    let clips = |app: &App| mix(app).shots.iter().map(|s| s.clip).collect::<Vec<_>>();
    assert_eq!(clips(&app), vec![Clip::MissileEmpty]);
    // The held trigger clicks a few times a second, not every frame.
    let mut clicks = 0;
    for _ in 0..60 {
        cue(&mut app, Sound::GunEmpty, None);
        app.update();
        clicks += clips(&app).iter().filter(|c| **c == Clip::GunEmpty).count();
    }
    assert!((3..=5).contains(&clicks), "{clicks} clicks in a second");
}
