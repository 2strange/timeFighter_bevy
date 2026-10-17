//! Headless tests of the round flow (mode Hunt).

use std::time::Duration;

use bevy::{
    prelude::*,
    time::TimeUpdateStrategy,
    transform::TransformPlugin,
    window::{WindowFocused, WindowOccluded},
};
use bevy_rapier3d::prelude::{Collider, RigidBody};

use super::{
    Difficulty, GameMode, GameSettings, Mission, Outcome, Pause, PausePlugin, RestartRound, Round,
    RoundPhase, RoundPlugin, SpawnSeed, WaveTrigger,
};
use crate::{
    ai::{AiPlugin, Enemy},
    audio::{Sound, SoundCue, SoundHooksPlugin},
    combat::{
        CombatConfig, CombatPlugin, CrashInto, Gun, Health, Hit, MissileRack, Munition, Team,
        Trigger, Weapon,
    },
    flight::{FlightConfig, FlightInput, FlightPlugin, FlightState, PlayerAircraft},
    physics::{Ground, Layer, PhysicsPlugin, PlayerCrashed, ProjectileHit, aircraft_body},
};

const FRAME: Duration = Duration::from_nanos(16_666_667);

/// Where the waves enter in these tests ([`Mission::scattered`]).
const TEST_SEED: u64 = 7;

fn app() -> (App, Entity) {
    app_with_jet_at(FlightConfig::default().spawn_transform())
}

/// The round app with the player's jet placed at `jet_at` before the first
/// update (the round start moves it to the spawn point).
fn app_with_jet_at(jet_at: Transform) -> (App, Entity) {
    let settings = GameSettings {
        difficulty: classic(),
        ..default()
    };
    custom_app(jet_at, settings, false)
}

/// The layout most tests below assume: two flights of four fighters (a
/// Bf 109 flight, then a Me 309 flight: rookies then veterans), at designed
/// speed.
fn classic() -> Difficulty {
    Difficulty::Custom(crate::game_state::DifficultyParams {
        missiles: 6,
        enemies: 8,
        flight_size: 4,
        pilots: crate::game_state::PilotMix::RookiesThenVeterans,
        enemy_speed: 1.0,
        enemy_accuracy: 1.0,
        attackers: 3,
        lock_time: 1.0,
        ..default()
    })
}

/// The round app for `settings`, optionally with the generated city and
/// its damage model (headless, no colliders: the flat ground stays).
fn custom_app(jet_at: Transform, settings: GameSettings, city: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugin,
        FlightPlugin,
        CombatPlugin,
        AiPlugin,
        RoundPlugin,
        PausePlugin,
        SoundHooksPlugin,
    ))
    .init_resource::<ButtonInput<KeyCode>>()
    .add_message::<WindowFocused>()
    .add_message::<WindowOccluded>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app.insert_resource(settings);
    // The same entry points every run.
    app.insert_resource(SpawnSeed::fixed(TEST_SEED));
    if city {
        app.add_plugins((
            crate::world::CityPlugin,
            crate::world::damage::CityDamagePlugin,
        ));
    }
    // Flat ground with its top at y = 0 (the city builds its own).
    app.world_mut().spawn((
        Ground,
        RigidBody::Fixed,
        Collider::cuboid(20_000.0, 50.0, 20_000.0),
        Layer::World.groups(),
        Transform::from_xyz(0.0, -50.0, 0.0),
    ));
    let cfg = FlightConfig::default();
    let jet = app
        .world_mut()
        .spawn((
            PlayerAircraft,
            FlightInput::default(),
            FlightState::new(&cfg),
            aircraft_body(Layer::Player),
            jet_at,
        ))
        .id();
    app.finish();
    app.update();
    (app, jet)
}

fn round(app: &App) -> Round {
    app.world().resource::<Round>().clone()
}

fn enemies(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, With<Enemy>>()
        .iter(app.world())
        .collect()
}

/// Shoots down every enemy currently in the air with one big hit each.
fn shoot_down_all(app: &mut App) {
    for enemy in enemies(app) {
        app.world_mut().write_message(Hit {
            projectile: Entity::PLACEHOLDER,
            target: enemy,
            point: Vec3::ZERO,
            team: Team::Player,
            weapon: Weapon::Missile,
            damage: 1_000.0,
        });
    }
    app.update();
}

fn run_for(app: &mut App, seconds: f32) {
    for _ in 0..(seconds / FRAME.as_secs_f32()).ceil() as u32 {
        app.update();
    }
}

fn hold(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
}

fn release(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
}

fn press(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

#[test]
fn hunt_starts_with_the_first_wave_and_counts_all_enemies() {
    let (mut app, _) = app();
    let mission = app.world().resource::<Mission>().clone();
    app.world_mut().write_message(RestartRound);
    app.update();
    assert_eq!(round(&app).phase, RoundPhase::Running);
    assert_eq!(
        enemies(&mut app).len(),
        mission.waves[0].enemy_count() as usize
    );
    assert_eq!(round(&app).enemies_left(&mission), mission.enemy_count());
    assert!((6..=8).contains(&mission.enemy_count()));
}

#[test]
fn victory_when_every_wave_is_shot_down() {
    let (mut app, _) = app();
    let mission = app.world().resource::<Mission>().clone();

    shoot_down_all(&mut app);
    // The second wave arrives once the first is (nearly) gone.
    app.update();
    assert_eq!(
        enemies(&mut app).len(),
        mission.waves[1].enemy_count() as usize
    );
    assert_eq!(round(&app).phase, RoundPhase::Running);

    shoot_down_all(&mut app);
    assert!(matches!(
        round(&app).phase,
        RoundPhase::Ending {
            outcome: Outcome::Victory,
            ..
        }
    ));
    run_for(&mut app, mission.end_delay + 0.1);
    let round = round(&app);
    assert_eq!(round.phase, RoundPhase::Over(Outcome::Victory));
    assert_eq!(round.stats.kills, mission.enemy_count());
    assert_eq!(round.stats.missile_hits, mission.enemy_count());
    assert_eq!(round.enemies_left(&mission), 0);

    // Result screen: game time stands still.
    assert!(app.world().resource::<Pause>().round_over);
    app.update();
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
}

#[test]
fn crashing_into_the_ground_loses_the_round() {
    let (mut app, jet) = app();
    // Nose down near the ground: the physics reports the impact.
    *app.world_mut().get_mut::<Transform>(jet).unwrap() =
        Transform::from_xyz(0.0, 20.0, 0.0).with_rotation(Quat::from_rotation_x(-0.8));
    run_for(&mut app, 2.0);
    assert_eq!(
        round(&app).phase,
        RoundPhase::Over(Outcome::Crashed(CrashInto::Ground))
    );
}

#[test]
fn being_shot_down_loses_the_round() {
    let (mut app, jet) = app();
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: jet,
        point: Vec3::ZERO,
        team: Team::Enemy,
        weapon: Weapon::Gun,
        damage: 1_000.0,
    });
    app.update();
    let defeat = app
        .world()
        .resource::<Messages<SoundCue>>()
        .iter_current_update_messages()
        .filter(|c| c.sound == Sound::Defeat)
        .count();
    assert_eq!(defeat, 1, "defeat sound hook");
    run_for(&mut app, 2.0);
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::ShotDown));
}

#[test]
fn no_firing_once_the_round_is_decided() {
    let (mut app, jet) = app();
    // Shot down ...
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: jet,
        point: Vec3::ZERO,
        team: Team::Enemy,
        weapon: Weapon::Gun,
        damage: 1_000.0,
    });
    app.update();
    // ... the trigger does nothing any more.
    hold(&mut app, KeyCode::Space);
    let ammo = app.world().get::<Gun>(jet).unwrap().ammo;
    run_for(&mut app, 0.3);
    assert_eq!(app.world().get::<Gun>(jet).unwrap().ammo, ammo);
}

#[test]
fn restart_resets_round_player_and_enemies() {
    let (mut app, jet) = app();
    let combat = app.world().resource::<CombatConfig>().clone();
    let flight = app.world().resource::<FlightConfig>().clone();
    let first_wave = enemies(&mut app);

    // Fly, shoot, get hurt, then crash.
    hold(&mut app, KeyCode::Space);
    app.world_mut().get_mut::<Trigger>(jet).unwrap().missile = true;
    run_for(&mut app, 0.5);
    release(&mut app, KeyCode::Space);
    app.world_mut().get_mut::<Health>(jet).unwrap().current = 40.0;
    *app.world_mut().get_mut::<Transform>(jet).unwrap() = Transform::from_xyz(0.0, 0.5, 0.0);
    run_for(&mut app, 2.0);
    assert_eq!(
        round(&app).phase,
        RoundPhase::Over(Outcome::Crashed(CrashInto::Ground))
    );
    assert!(round(&app).stats.gun_rounds > 0);

    // Restart keys only work on the result screen; Enter is one of them.
    // The restart happens in a frame where game time still stands (result
    // screen); the jet must stay where the reset put it.
    press(&mut app, KeyCode::Enter);
    let at_restart = app.world().get::<Transform>(jet).unwrap().translation;
    assert!(
        at_restart.distance(flight.spawn_position) < 1.0,
        "reset in the paused frame: {at_restart:?}"
    );
    app.update();

    let round = round(&app);
    assert_eq!(round.phase, RoundPhase::Running);
    assert_eq!(round.stats, Default::default());
    assert!(round.elapsed < 0.1);
    assert!(!app.world().resource::<Pause>().is_paused());

    let world = app.world();
    assert_eq!(
        world.get::<Health>(jet).unwrap().current,
        combat.player_health
    );
    assert_eq!(
        world.get::<Gun>(jet).unwrap().ammo,
        Some(combat.player_gun_ammo)
    );
    assert_eq!(
        world.get::<MissileRack>(jet).unwrap().count,
        combat.player_missiles
    );
    assert_eq!(*world.get::<Trigger>(jet).unwrap(), Trigger::default());
    let position = world.get::<Transform>(jet).unwrap().translation;
    assert!(
        position.distance(flight.spawn_position) < 5.0,
        "jet back at spawn: {position:?}"
    );

    let new_wave = enemies(&mut app);
    assert_eq!(new_wave.len(), first_wave.len());
    assert!(
        new_wave.iter().all(|e| !first_wave.contains(e)),
        "fresh enemies"
    );
    let leftovers = app
        .world_mut()
        .query::<&Munition>()
        .iter(app.world())
        .count();
    assert_eq!(leftovers, 0, "old rounds and missiles removed");

    // Crash detection is armed again: the next crash ends this round too.
    *app.world_mut().get_mut::<Transform>(jet).unwrap() = Transform::from_xyz(0.0, 0.5, 0.0);
    run_for(&mut app, 2.0);
    assert_eq!(
        self::round(&app).phase,
        RoundPhase::Over(Outcome::Crashed(CrashInto::Ground))
    );
}

#[test]
fn pause_still_freezes_the_running_round() {
    let (mut app, _) = app();
    run_for(&mut app, 0.5);
    press(&mut app, KeyCode::KeyP);
    let elapsed = round(&app).elapsed;
    let positions: Vec<Vec3> = {
        let mut q = app.world_mut().query_filtered::<&Transform, With<Enemy>>();
        q.iter(app.world()).map(|t| t.translation).collect()
    };
    run_for(&mut app, 1.0);
    assert_eq!(round(&app).elapsed, elapsed);
    let after: Vec<Vec3> = {
        let mut q = app.world_mut().query_filtered::<&Transform, With<Enemy>>();
        q.iter(app.world()).map(|t| t.translation).collect()
    };
    assert_eq!(positions, after, "enemies froze");

    press(&mut app, KeyCode::KeyP);
    run_for(&mut app, 0.2);
    assert!(round(&app).elapsed > elapsed);
}

#[test]
fn r_does_not_reset_the_jet_during_a_round() {
    let mut app = crate::test_support::headless_app(crate::GamePlugin);
    for _ in 0..30 {
        app.update();
    }
    let mut jets = app
        .world_mut()
        .query_filtered::<&Transform, With<PlayerAircraft>>();
    let before = jets.single(app.world()).unwrap().translation;
    let spawn = app.world().resource::<FlightConfig>().spawn_position;
    assert!(before.distance(spawn) > 1.0, "jet has moved");

    crate::test_support::tap(&mut app, KeyCode::KeyR);
    let after = jets.single(app.world()).unwrap().translation;
    assert!(after.distance(spawn) > 1.0, "R reset the jet mid-round");
    assert_eq!(round(&app).phase, RoundPhase::Running);
}

#[test]
fn ramming_a_fighter_loses_the_round_but_counts_the_kill() {
    let (mut app, jet) = app();
    let mission = app.world().resource::<Mission>().clone();
    let enemy = enemies(&mut app)[0];
    let rammed = app.world().get::<Enemy>(enemy).unwrap().kind;
    // Put one fighter right in the jet's path.
    let jet_tf = *app.world().get::<Transform>(jet).unwrap();
    let in_path = Transform::from_translation(jet_tf.translation + jet_tf.forward() * 4.0)
        .with_rotation(jet_tf.rotation);
    *app.world_mut().get_mut::<Transform>(enemy).unwrap() = in_path;
    run_for(&mut app, mission.end_delay + 0.5);

    let round = round(&app);
    assert_eq!(
        round.phase,
        RoundPhase::Over(Outcome::Crashed(CrashInto::Aircraft(rammed.display_name())))
    );
    assert_eq!(round.stats.kills, 1);
    assert!(
        app.world().get_entity(enemy).is_err(),
        "rammed enemy removed"
    );
}

#[test]
fn the_round_starts_in_the_very_first_frame() {
    // The first frame has no time step; the start, which moves the jet to
    // its spawn point, must still hold.
    let (app, jet) = app_with_jet_at(Transform::from_xyz(300.0, 900.0, -2_000.0));
    let flight = app.world().resource::<FlightConfig>().clone();
    assert_eq!(round(&app).phase, RoundPhase::Running);
    let position = app.world().get::<Transform>(jet).unwrap().translation;
    assert!(
        position.distance(flight.spawn_position) < 1e-3,
        "{position:?}"
    );
}

#[test]
fn a_restart_ignores_hits_and_crashes_still_queued_from_the_old_round() {
    let (mut app, jet) = app();
    let old_enemy = enemies(&mut app)[0];
    // Fire, so old rounds are in the air.
    hold(&mut app, KeyCode::Space);
    run_for(&mut app, 0.3);
    release(&mut app, KeyCode::Space);
    let old_round = app
        .world_mut()
        .query_filtered::<Entity, With<Munition>>()
        .iter(app.world())
        .next()
        .expect("rounds in flight");

    // What the old round's last physics step left behind, then a restart.
    app.world_mut().write_message(ProjectileHit {
        projectile: old_round,
        target: old_enemy,
        point: Vec3::ZERO,
    });
    app.world_mut().write_message(Hit {
        projectile: old_round,
        target: old_enemy,
        point: Vec3::ZERO,
        team: Team::Player,
        weapon: Weapon::Gun,
        damage: 1_000.0,
    });
    app.world_mut().write_message(PlayerCrashed {
        player: jet,
        obstacle: old_enemy,
        point: Vec3::ZERO,
    });
    app.world_mut().write_message(RestartRound);
    run_for(&mut app, 0.5);

    let round = round(&app);
    let mission = app.world().resource::<Mission>().clone();
    assert_eq!(round.phase, RoundPhase::Running);
    assert_eq!(round.stats.kills, 0);
    assert_eq!(round.stats.gun_hits, 0);
    assert_eq!(round.stats.gun_rounds, 0);
    assert_eq!(round.enemies_left(&mission), mission.enemy_count());
    assert!(!app.world().get::<Health>(jet).unwrap().is_dead());
    assert_eq!(
        enemies(&mut app).len(),
        mission.waves[0].enemy_count() as usize
    );
}

#[test]
fn paused_weapons_neither_fire_nor_fire_later() {
    let (mut app, jet) = app();
    let combat = app.world().resource::<CombatConfig>().clone();
    press(&mut app, KeyCode::KeyP);
    assert!(app.world().resource::<Pause>().is_paused());

    // Gun held and a missile pressed while paused.
    hold(&mut app, KeyCode::Space);
    press(&mut app, KeyCode::ShiftRight);
    run_for(&mut app, 0.5);
    let world = app.world();
    assert_eq!(
        world.get::<Gun>(jet).unwrap().ammo,
        Some(combat.player_gun_ammo)
    );
    assert_eq!(
        world.get::<MissileRack>(jet).unwrap().count,
        combat.player_missiles
    );
    let in_flight = app
        .world_mut()
        .query::<&Munition>()
        .iter(app.world())
        .count();
    assert_eq!(in_flight, 0);

    // Resumed: the held gun fires, the missile pressed during the pause
    // does not launch on its own.
    press(&mut app, KeyCode::KeyP);
    run_for(&mut app, 0.5);
    let world = app.world();
    assert!(world.get::<Gun>(jet).unwrap().ammo.unwrap() < combat.player_gun_ammo);
    assert_eq!(
        world.get::<MissileRack>(jet).unwrap().count,
        combat.player_missiles
    );
}

#[test]
fn hunt_sends_rookies_first_and_veterans_second() {
    let (mut app, _) = app();
    let skills = |app: &mut App| {
        app.world_mut()
            .query::<&crate::ai::EnemyPilot>()
            .iter(app.world())
            .map(|p| p.skill)
            .collect::<Vec<_>>()
    };
    assert!(
        skills(&mut app)
            .iter()
            .all(|s| *s == crate::ai::PilotSkill::ROOKIE)
    );
    shoot_down_all(&mut app);
    app.update();
    let second = skills(&mut app);
    assert!(!second.is_empty());
    assert!(second.iter().all(|s| *s == crate::ai::PilotSkill::VETERAN));
}

#[test]
fn difficulty_table_matches_the_design() {
    let table: Vec<(u32, u32, u32)> = Difficulty::PRESETS
        .iter()
        .map(|d| {
            let p = d.params();
            (p.missiles, p.enemies, p.flight_size)
        })
        .collect();
    assert_eq!(
        table,
        vec![(24, 6, 2), (12, 6, 2), (8, 10, 2), (8, 12, 4), (8, 16, 4)]
    );
    for difficulty in Difficulty::PRESETS {
        let params = difficulty.params();
        let mission = Mission::hunt(difficulty);
        assert_eq!(mission.enemy_count(), params.enemies, "{difficulty:?}");
        assert_eq!(
            mission.waves.len() as u32,
            params.enemies / params.flight_size,
            "{difficulty:?}"
        );
    }
}

#[test]
fn custom_difficulty_is_clamped_and_splits_uneven_flights() {
    let custom = Difficulty::Custom(crate::game_state::DifficultyParams {
        enemies: 7,
        flight_size: 3,
        enemy_speed: 9.0,
        ..default()
    });
    let mission = Mission::hunt(custom);
    let sizes: Vec<u32> = mission.waves.iter().map(|w| w.enemy_count()).collect();
    assert_eq!(sizes, vec![3, 3, 1]);
    assert!(mission.waves[0].groups[0].tuning.speed <= 2.0);
}

#[test]
fn a_round_starts_with_the_chosen_difficulty() {
    let (mut app, jet) = app();
    app.insert_resource(GameSettings {
        difficulty: Difficulty::Custom(crate::game_state::DifficultyParams {
            missiles: 20,
            enemies: 4,
            flight_size: 2,
            unlimited_gun: true,
            ..default()
        }),
        ..default()
    });
    app.world_mut().write_message(RestartRound);
    app.update();
    let mission = app.world().resource::<Mission>().clone();
    assert_eq!(mission.enemy_count(), 4);
    assert_eq!(enemies(&mut app).len(), 2, "first flight of two");
    assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 20);
    assert_eq!(
        app.world().get::<Gun>(jet).unwrap().ammo,
        None,
        "unlimited gun"
    );
}

#[test]
fn restart_keys_wait_while_something_else_holds_the_game() {
    let (mut app, jet) = app();
    *app.world_mut().get_mut::<Transform>(jet).unwrap() = Transform::from_xyz(0.0, 0.5, 0.0);
    run_for(&mut app, 2.0);
    assert!(matches!(round(&app).phase, RoundPhase::Over(_)));

    // Paused by the player (a menu would hold the game the same way).
    app.world_mut().resource_mut::<Pause>().player = true;
    press(&mut app, KeyCode::Enter);
    assert!(
        matches!(round(&app).phase, RoundPhase::Over(_)),
        "restarted while held"
    );

    app.world_mut().resource_mut::<Pause>().player = false;
    press(&mut app, KeyCode::Enter);
    assert_eq!(round(&app).phase, RoundPhase::Running);
}

#[test]
fn non_finite_custom_values_count_as_normal() {
    let custom = Difficulty::Custom(crate::game_state::DifficultyParams {
        enemy_speed: f32::NAN,
        enemy_accuracy: f32::NEG_INFINITY,
        ..default()
    });
    let params = custom.params().clamped();
    assert_eq!((params.enemy_speed, params.enemy_accuracy), (1.0, 1.0));
    let mission = Mission::hunt(custom);
    let tuning = mission.waves[0].groups[0].tuning;
    assert_eq!((tuning.speed, tuning.accuracy), (1.0, 1.0));
}

#[test]
fn easy_levels_lock_faster_and_send_fewer_attackers() {
    let (mut app, _) = app();
    app.insert_resource(GameSettings {
        difficulty: Difficulty::SuperEasy,
        ..default()
    });
    app.world_mut().write_message(RestartRound);
    app.update();
    let lock = app.world().resource::<CombatConfig>().lock.time;
    let slots = app.world().resource::<crate::ai::AttackSlots>().max;
    assert_eq!((lock, slots), (0.6, 1));
    let attackers = app
        .world_mut()
        .query::<&crate::ai::EnemyPilot>()
        .iter(app.world())
        .filter(|p| p.attacking)
        .count();
    assert!(attackers <= 1);
}

#[test]
fn protect_sends_bomber_flights_by_the_table() {
    for difficulty in Difficulty::PRESETS {
        let params = difficulty.params();
        let mission = Mission::protect(difficulty);
        assert_eq!(
            mission.waves.len() as u32,
            params.bomber_waves,
            "{difficulty:?}"
        );
        assert_eq!(
            mission.enemy_count(),
            params.bomber_waves * params.bombers_per_wave,
            "{difficulty:?}"
        );
        assert!(mission.waves.iter().skip(1).all(|w| matches!(
            w.trigger,
            WaveTrigger::AfterSecondsOrCleared(t) if t >= 90.0
        )));
        assert_eq!(mission.city_loss, Some(params.city_loss));
    }
    assert!(GameMode::ALL.contains(&GameMode::Protect));
}

fn protect(difficulty: Difficulty) -> GameSettings {
    GameSettings {
        mode: GameMode::Protect,
        difficulty,
        ..default()
    }
}

#[test]
fn protect_is_won_when_every_bomber_is_down() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        protect(Difficulty::SuperEasy),
        true,
    );
    let mission = app.world().resource::<Mission>().clone();
    assert_eq!(mission.mode, GameMode::Protect);
    assert_eq!(enemies(&mut app).len(), 4, "a flight of four bombers");
    // Each wave arrives as soon as the sky is clear.
    for _ in 0..mission.waves.len() {
        shoot_down_all(&mut app);
        app.update();
    }
    run_for(&mut app, mission.end_delay + 0.5);
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::Victory));
}

#[test]
fn protect_is_lost_when_the_centre_falls_and_a_restart_rebuilds_it() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        protect(Difficulty::Normal),
        true,
    );
    // A raid flattens the whole centre at once.
    app.world_mut()
        .write_message(crate::world::damage::DamageArea {
            point: Vec3::ZERO,
            radius: 3_000.0,
            strength: 1.0,
        });
    run_for(&mut app, 3.0);
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::CityDestroyed));

    press(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(round(&app).phase, RoundPhase::Running);
    let health = app.world().resource::<crate::world::damage::CityHealth>();
    assert_eq!(health.center_intact_ratio, 1.0, "city rebuilt");
}

#[test]
fn damage_still_queued_from_the_old_round_does_not_hit_the_new_city() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        protect(Difficulty::Normal),
        true,
    );
    // The old round's last bombs and a restart in the same frame.
    app.world_mut()
        .write_message(crate::world::damage::DamageArea {
            point: Vec3::ZERO,
            radius: 3_000.0,
            strength: 1.0,
        });
    app.world_mut().write_message(RestartRound);
    run_for(&mut app, 0.5);
    assert_eq!(round(&app).phase, RoundPhase::Running);
    let health = app.world().resource::<crate::world::damage::CityHealth>();
    assert_eq!(health.center_intact_ratio, 1.0);
}

#[test]
fn missiles_are_counted_per_mode() {
    let protect: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| d.params_for(GameMode::Protect).missiles)
        .collect();
    // Two missiles a bomber, times 2.0 / 1.5 / 0.9 / 0.75, rounded up;
    // Insane keeps its 12.
    assert_eq!(protect, vec![32, 24, 22, 18, 12]);
    let hunt: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| d.params_for(GameMode::Hunt).missiles)
        .collect();
    // One a fighter: 6, 6, 10, 12 fighters; Insane keeps its 8.
    assert_eq!(hunt, vec![12, 9, 9, 9, 8]);
    for difficulty in Difficulty::PRESETS {
        assert_eq!(
            difficulty.params_for(GameMode::Hunt),
            crate::game_state::DifficultyParams {
                missiles: difficulty.params_for(GameMode::Hunt).missiles,
                ..difficulty.params()
            },
            "only the missiles differ"
        );
    }
    let custom = Difficulty::Custom(crate::game_state::DifficultyParams {
        missiles: 7,
        ..default()
    });
    assert_eq!(custom.params_for(GameMode::Protect).missiles, 7);
}

#[test]
fn a_protect_round_arms_the_jet_with_the_protect_missiles() {
    let (mut app, jet) = custom_app(
        FlightConfig::default().spawn_transform(),
        protect(Difficulty::Normal),
        true,
    );
    assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 22);
    app.insert_resource(GameSettings {
        difficulty: Difficulty::Normal,
        ..default()
    });
    app.world_mut().write_message(RestartRound);
    app.update();
    assert_eq!(
        app.world().get::<MissileRack>(jet).unwrap().count,
        9,
        "Hunt"
    );
}

#[test]
fn fighters_are_bf_109_and_me_309_bombers_a_ju_390_with_he_177s() {
    use crate::ai::EnemyKind;
    // Hard: rookies first (Bf 109 flights), veterans after (Me 309).
    let hunt = Mission::hunt(Difficulty::Hard);
    assert_eq!(hunt.waves[0].groups[0].members, vec![EnemyKind::Bf109K4; 4]);
    assert_eq!(
        hunt.waves.last().unwrap().groups[0].members,
        vec![EnemyKind::Me309; 4]
    );
    let protect = Mission::protect(Difficulty::Normal);
    assert_eq!(
        protect.waves[0].groups[0].members,
        vec![
            EnemyKind::Ju390,
            EnemyKind::He177,
            EnemyKind::He177,
            EnemyKind::He177
        ]
    );
    // Leaders are the slower types, so formations hold together.
    assert!(EnemyKind::Bf109K4.stats().cruise_speed < EnemyKind::Me309.stats().cruise_speed);
    assert!(EnemyKind::Ju390.stats().cruise_speed < EnemyKind::He177.stats().cruise_speed);
    assert_eq!(
        EnemyKind::Ju390.stats().health,
        2.0 * EnemyKind::He177.stats().health
    );
}

#[test]
fn a_round_spawns_the_first_flight_by_its_pilots() {
    let (mut app, _) = app();
    app.world_mut().write_message(RestartRound);
    app.update();
    let mut kinds: Vec<_> = app
        .world_mut()
        .query::<&Enemy>()
        .iter(app.world())
        .map(|e| format!("{:?}", e.kind))
        .collect();
    kinds.sort();
    // Rookies first: the first flight is all Bf 109.
    assert_eq!(kinds, vec!["Bf109K4"; 4]);
    run_for(&mut app, 5.0);
    assert_eq!(round(&app).phase, RoundPhase::Running);
}

#[test]
fn a_protect_round_sends_a_ju_390_with_he_177s_and_their_gunners() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        protect(Difficulty::SuperEasy),
        true,
    );
    app.world_mut().write_message(RestartRound);
    app.update();
    let mut kinds: Vec<_> = app
        .world_mut()
        .query::<(&Enemy, &crate::combat::Turrets)>()
        .iter(app.world())
        .map(|(e, t)| (format!("{:?}", e.kind), t.mounts.len()))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            ("He177".to_string(), 2),
            ("He177".to_string(), 2),
            ("He177".to_string(), 2),
            ("Ju390".to_string(), 2)
        ]
    );
    run_for(&mut app, 5.0);
    assert_eq!(round(&app).phase, RoundPhase::Running);
}

fn battle(difficulty: Difficulty) -> GameSettings {
    GameSettings {
        mode: GameMode::Battle,
        difficulty,
        ..default()
    }
}

#[test]
fn battle_waves_are_bombers_with_a_fighter_escort() {
    use crate::ai::EnemyKind;
    let totals: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| Mission::battle(*d).enemy_count())
        .collect();
    // SE 2×(4+2), Easy 2×(4+2), Normal 3×(4+2), Hard 3×(4+4), Insane
    // 4×(4+4); from Normal on plus zeppelins, alone (Insane three).
    assert_eq!(totals, vec![12, 12, 18 + 1, 24 + 1, 32 + 3]);
    for difficulty in Difficulty::PRESETS {
        let params = difficulty.params();
        let mission = Mission::battle(difficulty);
        assert_eq!(mission.mode, GameMode::Battle);
        assert_eq!(mission.city_loss, Some(params.city_loss));
        let bomber_waves = params.bomber_waves as usize;
        assert_eq!(
            mission.waves.len(),
            bomber_waves + params.zeppelins as usize
        );
        let mut bombers_seen = 0;
        for wave in &mission.waves {
            match &wave.groups[..] {
                [bombers, escort] => {
                    bombers_seen += 1;
                    assert!(bombers.members.iter().all(|k| k.is_bomber()));
                    assert_eq!(bombers.count(), params.bombers_per_wave);
                    assert!(escort.members.iter().all(|k| !k.is_bomber()));
                    assert_eq!(escort.escorts, Some(0));
                    assert_eq!(escort.count(), params.flight_size);
                }
                // A zeppelin alone, launching its own fighters.
                [zeppelin] => {
                    assert_eq!(zeppelin.members, vec![EnemyKind::Zeppelin]);
                    let carrier = zeppelin.carrier.as_ref().expect("a carrier");
                    assert_eq!(carrier.max_alive, params.zeppelin_fighters);
                    assert!(carrier.fighters.iter().all(|k| !k.is_bomber()));
                }
                groups => panic!("unexpected wave of {} groups", groups.len()),
            }
        }
        assert_eq!(bombers_seen, bomber_waves);
        let zeppelins = match difficulty {
            Difficulty::SuperEasy | Difficulty::Easy => 0,
            Difficulty::Insane => 3,
            _ => 1,
        };
        assert_eq!(params.zeppelins, zeppelins, "{difficulty:?}");
    }
    let second = Mission::battle(Difficulty::Hard);
    assert_eq!(second.waves[0].groups[0].members[0], EnemyKind::Ju390);
    assert_eq!(
        second.waves[0].groups[1].members,
        vec![EnemyKind::Bf109K4; 4],
        "Hard: rookie escorts (Bf 109) first"
    );
    assert!(GameMode::ALL.contains(&GameMode::Battle));
}

#[test]
fn battle_missiles_follow_the_two_for_a_bomber_one_for_a_fighter_rule() {
    let missiles: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| d.params_for(GameMode::Battle).missiles)
        .collect();
    // SE 2×(8+2) = 20 needed, Easy the same, Normal 24 + 6 + 9 = 39,
    // Hard 24 + 12 + 9 = 45; times 2.0 / 1.5 / 0.9 / 0.75. Insane keeps 36.
    let needed: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| Mission::battle(*d).missiles_needed())
        .collect();
    assert_eq!(needed, vec![20, 20, 39, 45, 75]);
    assert_eq!(missiles, vec![40, 30, 36, 34, 36]);
    // Carrier fighters are gun work: the count ignores the hangar.
    let without_hangar = Difficulty::Custom(crate::game_state::DifficultyParams {
        zeppelin_fighters: 0,
        ..default()
    });
    assert_eq!(
        Mission::battle(without_hangar).missiles_needed(),
        Mission::battle(Difficulty::Custom(default())).missiles_needed()
    );
}

#[test]
fn a_battle_round_is_won_when_everything_is_down() {
    let (mut app, jet) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::SuperEasy),
        true,
    );
    assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 40);
    let mut kinds: Vec<bool> = app
        .world_mut()
        .query::<&Enemy>()
        .iter(app.world())
        .map(|e| e.kind.is_bomber())
        .collect();
    kinds.sort();
    assert_eq!(kinds, vec![false, false, true, true, true, true]);
    // The escort knows its bombers.
    let mut groups = app.world_mut().query::<(&Enemy, &crate::ai::FlightGroup)>();
    let bomber_flight = groups
        .iter(app.world())
        .find(|(e, _)| e.kind.is_bomber())
        .map(|(_, g)| g.id)
        .unwrap();
    assert!(
        groups
            .iter(app.world())
            .filter(|(e, _)| !e.kind.is_bomber())
            .all(|(_, g)| g.escort == Some(bomber_flight))
    );
    for _ in 0..app.world().resource::<Mission>().waves.len() {
        shoot_down_all(&mut app);
        app.update();
    }
    run_for(&mut app, 3.0);
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::Victory));
}

#[test]
fn a_battle_round_is_lost_with_the_city_or_the_jet() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::Normal),
        true,
    );
    app.world_mut()
        .write_message(crate::world::damage::DamageArea {
            point: Vec3::ZERO,
            radius: 3_000.0,
            strength: 1.0,
        });
    run_for(&mut app, 3.0);
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::CityDestroyed));

    let (mut app, jet) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::Normal),
        true,
    );
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: jet,
        point: Vec3::ZERO,
        team: Team::Enemy,
        weapon: Weapon::Gun,
        damage: 1_000.0,
    });
    run_for(&mut app, 3.0);
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::ShotDown));
}

#[test]
fn waiting_escorts_stay_with_their_bombers() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::Hard),
        true,
    );
    // Nobody attacks: the whole escort waits with the bombers.
    app.insert_resource(crate::ai::AttackSlots { max: 0 });
    run_for(&mut app, 40.0);
    let mut leader = None;
    let mut escorts = Vec::new();
    let mut q = app
        .world_mut()
        .query::<(&Enemy, &crate::ai::FlightGroup, &Transform)>();
    for (enemy, group, tf) in q.iter(app.world()) {
        if enemy.kind.is_bomber() && group.slot == 0 {
            leader = Some(tf.translation);
        } else if !enemy.kind.is_bomber() {
            escorts.push(tf.translation);
        }
    }
    let leader = leader.expect("lead bomber");
    assert_eq!(escorts.len(), 4);
    for escort in escorts {
        let distance = escort.distance(leader);
        assert!(
            distance < 1_500.0,
            "escort {distance:.0} m from its bombers"
        );
    }
}

// --- The player's jet is gone in the frame it is destroyed ----------------

mod instant_wreck {
    use bevy::prelude::*;
    use bevy_rapier3d::prelude::{Collider, ColliderDisabled, RigidBody};

    use super::{app, round, run_for};
    use crate::{
        combat::{Effect, EffectKind, Health, Hit, Team, Weapon},
        flight::Wrecked,
        game_state::{RestartRound, RoundPhase},
        physics::{Layer, aircraft_body},
    };

    fn wrecked(app: &App, jet: Entity) -> bool {
        app.world().get::<Wrecked>(jet).is_some()
    }

    fn hidden(app: &App, jet: Entity) -> bool {
        app.world().get::<Visibility>(jet) == Some(&Visibility::Hidden)
    }

    fn big_explosions(app: &mut App) -> usize {
        app.world_mut()
            .query::<&Effect>()
            .iter(app.world())
            .filter(|e| e.kind == EffectKind::BigExplosion)
            .count()
    }

    fn position(app: &App, jet: Entity) -> Vec3 {
        app.world().get::<Transform>(jet).unwrap().translation
    }

    /// Flies until the jet is wrecked, checking every frame that it never
    /// shows inside `inside` without being wrecked. Returns the frame count.
    fn fly_into(app: &mut App, jet: Entity, inside: impl Fn(Vec3) -> bool) -> u32 {
        for frame in 1..=240 {
            app.update();
            let at = position(app, jet);
            if wrecked(app, jet) {
                // Everything happens in the same frame.
                assert!(hidden(app, jet), "wrecked but still visible");
                assert_eq!(big_explosions(app), 1, "no explosion in the wreck frame");
                assert!(app.world().get::<ColliderDisabled>(jet).is_some());
                return frame;
            }
            assert!(
                !inside(at),
                "frame {frame}: jet at {at} inside without a crash"
            );
        }
        panic!("never crashed");
    }

    /// After the wreck: it stays put and hidden for the rest of the round.
    fn stays_down(app: &mut App, jet: Entity) {
        let at = position(app, jet);
        run_for(app, 1.0);
        assert_eq!(position(app, jet), at, "the wreck flew on");
        assert!(hidden(app, jet) && wrecked(app, jet));
        assert!(app.world().get::<Health>(jet).unwrap().is_dead());
    }

    #[test]
    fn diving_into_the_ground_explodes_at_once_and_never_resurfaces() {
        let (mut app, jet) = app();
        *app.world_mut().get_mut::<Transform>(jet).unwrap() =
            Transform::from_xyz(0.0, 60.0, 0.0).with_rotation(Quat::from_rotation_x(-0.9));
        fly_into(&mut app, jet, |p| p.y < 0.0);
        stays_down(&mut app, jet);
        run_for(&mut app, 2.0);
        assert!(matches!(round(&app).phase, RoundPhase::Over(_)));
    }

    #[test]
    fn flying_into_a_building_explodes_at_the_wall() {
        let (mut app, jet) = app();
        let start = position(&app, jet);
        let forward = app.world().get::<Transform>(jet).unwrap().forward();
        // A 200 m block 200 m ahead, across the flight path.
        let centre = start + forward * 300.0;
        let half = Vec3::splat(100.0);
        app.world_mut().spawn((
            RigidBody::Fixed,
            Collider::cuboid(half.x, half.y, half.z),
            Layer::World.groups(),
            Transform::from_translation(centre),
        ));
        fly_into(&mut app, jet, |p| {
            ((p - centre).abs() - half).max_element() < 0.0
        });
        stays_down(&mut app, jet);
    }

    #[test]
    fn touching_an_enemy_is_instant_death() {
        let (mut app, jet) = app();
        let start = position(&app, jet);
        let forward = app.world().get::<Transform>(jet).unwrap().forward();
        let enemy_at = start + forward * 150.0;
        app.world_mut().spawn((
            Team::Enemy,
            Health::full(50.0),
            aircraft_body(Layer::Enemy),
            Transform::from_translation(enemy_at),
        ));
        fly_into(&mut app, jet, |p| p.distance(enemy_at) < 3.0);
        stays_down(&mut app, jet);
    }

    #[test]
    fn being_shot_down_hides_the_jet_in_the_same_frame() {
        let (mut app, jet) = app();
        app.world_mut().write_message(Hit {
            projectile: Entity::PLACEHOLDER,
            target: jet,
            point: Vec3::ZERO,
            team: Team::Enemy,
            weapon: Weapon::Gun,
            damage: 1_000.0,
        });
        app.update();
        assert!(wrecked(&app, jet) && hidden(&app, jet));
        assert_eq!(big_explosions(&mut app), 1);
        stays_down(&mut app, jet);
    }

    #[test]
    fn enter_on_the_result_screen_restarts_without_a_second_explosion() {
        let (mut app, jet) = app();
        *app.world_mut().get_mut::<Transform>(jet).unwrap() =
            Transform::from_xyz(0.0, 60.0, 0.0).with_rotation(Quat::from_rotation_x(-0.9));
        fly_into(&mut app, jet, |p| p.y < 0.0);
        // Wait for the result screen, as a player would.
        run_for(&mut app, 3.0);
        assert!(matches!(round(&app).phase, RoundPhase::Over(_)));

        let deaths = |app: &App| {
            app.world()
                .resource::<Messages<crate::combat::Destroyed>>()
                .iter_current_update_messages()
                .filter(|d| d.team == Team::Player)
                .count()
        };
        super::hold(&mut app, KeyCode::Enter);
        let mut later_deaths = 0;
        let mut later_explosions = 0;
        for frame in 0..90 {
            app.update();
            if frame == 1 {
                super::release(&mut app, KeyCode::Enter);
            }
            later_deaths += deaths(&app);
            later_explosions = later_explosions.max(big_explosions(&mut app));
            assert!(
                !wrecked(&app, jet) || frame == 0,
                "frame {frame}: wrecked again"
            );
        }
        assert_eq!(later_deaths, 0, "the jet died again after the restart");
        assert_eq!(later_explosions, 0, "a new explosion after the restart");
        assert_eq!(round(&app).phase, RoundPhase::Running);
        assert!(!hidden(&app, jet));
        let before = position(&app, jet);
        run_for(&mut app, 0.5);
        assert!(
            position(&app, jet).distance(before) > 10.0,
            "jet does not fly"
        );
    }

    #[test]
    fn restarting_after_a_win_does_not_crash_on_the_way_back_to_the_spawn() {
        let (mut app, jet) = app();
        let spawn = position(&app, jet);
        // Somewhere else, with a wall between there and the spawn point.
        let away = spawn + Vec3::new(0.0, 0.0, -1_000.0);
        app.world_mut().spawn((
            RigidBody::Fixed,
            Collider::cuboid(300.0, 300.0, 20.0),
            Layer::World.groups(),
            Transform::from_translation(spawn + Vec3::new(0.0, 0.0, -500.0)),
        ));
        for _ in 0..3 {
            *app.world_mut().get_mut::<Transform>(jet).unwrap() = Transform::from_translation(away);
            app.update();
        }
        app.world_mut()
            .resource_mut::<crate::game_state::Round>()
            .phase = RoundPhase::Ending {
            outcome: crate::game_state::Outcome::Victory,
            left: 0.1,
        };
        run_for(&mut app, 0.5);
        assert!(matches!(round(&app).phase, RoundPhase::Over(_)));
        super::hold(&mut app, KeyCode::Enter);
        for frame in 0..60 {
            app.update();
            if frame == 1 {
                super::release(&mut app, KeyCode::Enter);
            }
            assert!(
                !wrecked(&app, jet),
                "frame {frame}: crashed after the restart"
            );
        }
        assert_eq!(round(&app).phase, RoundPhase::Running);
    }

    #[test]
    fn a_new_round_brings_the_jet_back() {
        let (mut app, jet) = app();
        *app.world_mut().get_mut::<Transform>(jet).unwrap() =
            Transform::from_xyz(0.0, 60.0, 0.0).with_rotation(Quat::from_rotation_x(-0.9));
        fly_into(&mut app, jet, |p| p.y < 0.0);
        app.world_mut().write_message(RestartRound);
        run_for(&mut app, 0.2);
        assert!(!wrecked(&app, jet), "still wrecked after the restart");
        assert!(!hidden(&app, jet));
        assert!(app.world().get::<ColliderDisabled>(jet).is_none());
        let before = position(&app, jet);
        run_for(&mut app, 0.5);
        assert!(
            position(&app, jet).distance(before) > 10.0,
            "jet does not fly"
        );
        assert_eq!(round(&app).phase, RoundPhase::Running);
    }
}

#[test]
fn levels_set_gun_ammunition_speed_and_flights() {
    let table: Vec<(Option<u32>, f32, u32, u32)> = Difficulty::PRESETS
        .iter()
        .map(|d| {
            let p = d.params();
            (
                (!p.unlimited_gun).then_some(p.gun_ammo),
                p.enemy_speed,
                p.enemies.div_ceil(p.flight_size),
                p.attackers,
            )
        })
        .collect();
    assert_eq!(
        table,
        vec![
            (None, 0.5, 3, 1),
            (None, 0.6, 3, 1),
            (Some(2_000), 0.75, 5, 2),
            (Some(1_000), 0.9, 3, 3),
            (Some(1_000), 1.0, 4, 4),
        ]
    );
    assert_eq!(Difficulty::SuperEasy.params().bomber_waves, 2);

    // The round loads the gun accordingly.
    for (difficulty, ammo) in [
        (Difficulty::SuperEasy, None),
        (Difficulty::Normal, Some(2_000)),
        (Difficulty::Insane, Some(1_000)),
    ] {
        let (mut app, jet) = app();
        app.insert_resource(GameSettings {
            difficulty,
            ..default()
        });
        app.world_mut().write_message(RestartRound);
        app.update();
        assert_eq!(
            app.world().get::<Gun>(jet).unwrap().ammo,
            ammo,
            "{difficulty:?}"
        );
    }
}

#[test]
fn the_battle_is_only_won_once_the_zeppelin_wreck_has_hit_the_ground() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::Normal),
        true,
    );
    let mission = app.world().resource::<Mission>().clone();
    // Down with every wave as it comes, the zeppelin between the bombers.
    for _ in 0..mission.waves.len() {
        shoot_down_all(&mut app);
        app.update();
    }
    let falling = app
        .world_mut()
        .query_filtered::<&Enemy, With<crate::combat::GoingDown>>()
        .iter(app.world())
        .map(|e| e.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        falling,
        vec![crate::ai::EnemyKind::Zeppelin],
        "burning on its way down"
    );
    assert_eq!(
        round(&app).enemies_left(&mission),
        0,
        "already counts as down"
    );
    run_for(&mut app, 5.0);
    assert_eq!(
        round(&app).phase,
        RoundPhase::Running,
        "not before the impact"
    );
    run_for(&mut app, 40.0);
    // It came down far out: the city stands, the battle is won.
    assert_eq!(round(&app).phase, RoundPhase::Over(Outcome::Victory));
    let wrecks = app
        .world_mut()
        .query_filtered::<(), With<crate::combat::GoingDown>>()
        .iter(app.world())
        .count();
    assert_eq!(wrecks, 0);
}

#[test]
fn the_pilots_pick_the_fighter_type() {
    use crate::{
        ai::{EnemyKind, PilotSkill},
        game_state::{PilotMix, fighter_flight},
    };
    use EnemyKind::{Bf109K4 as Bf, Me309 as Me};
    assert_eq!(fighter_flight(4, PilotMix::Rookies, 0, 2), vec![Bf; 4]);
    assert_eq!(fighter_flight(4, PilotMix::Veterans, 0, 2), vec![Me; 4]);
    // Mixed: alternating, the slower Bf 109 leads.
    assert_eq!(
        fighter_flight(4, PilotMix::Mixed, 0, 2),
        vec![Bf, Me, Bf, Me]
    );
    assert_eq!(
        fighter_flight(2, PilotMix::RookiesThenVeterans, 0, 3),
        vec![Bf; 2]
    );
    assert_eq!(
        fighter_flight(2, PilotMix::RookiesThenVeterans, 2, 3),
        vec![Me; 2]
    );
    // The type brings its pilot; bombers have none of their own.
    assert_eq!(Bf.pilot(), Some(PilotSkill::ROOKIE));
    assert_eq!(Me.pilot(), Some(PilotSkill::VETERAN));
    assert_eq!(EnemyKind::He177.pilot(), None);

    // In a round, a mixed flight's members fly with their type's pilot.
    let (mut app, _) = app();
    app.insert_resource(GameSettings {
        difficulty: Difficulty::Custom(crate::game_state::DifficultyParams {
            pilots: PilotMix::Mixed,
            flight_size: 4,
            ..default()
        }),
        ..default()
    });
    app.world_mut().write_message(RestartRound);
    app.update();
    let pilots: Vec<(EnemyKind, PilotSkill)> = app
        .world_mut()
        .query::<(&Enemy, &crate::ai::EnemyPilot)>()
        .iter(app.world())
        .map(|(e, p)| (e.kind, p.skill))
        .collect();
    assert_eq!(pilots.len(), 4);
    assert!(
        pilots
            .iter()
            .all(|(kind, skill)| Some(*skill) == kind.pilot()),
        "{pilots:?}"
    );
}

#[test]
fn several_zeppelins_come_from_different_sides() {
    let mission = Mission::battle(Difficulty::Insane);
    let zeppelins: Vec<(Vec3, Vec3)> = mission
        .waves
        .iter()
        .flat_map(|w| &w.groups)
        .filter(|g| g.members == vec![crate::ai::EnemyKind::Zeppelin])
        .map(|g| (g.lead, g.heading))
        .collect();
    assert_eq!(zeppelins.len(), 3);
    for (i, a) in zeppelins.iter().enumerate() {
        for b in &zeppelins[i + 1..] {
            assert!(a.0.distance(b.0) > 2_000.0, "{a:?} {b:?}");
        }
    }
    // Custom can ask for none or up to three.
    let custom = |n| {
        Mission::battle(Difficulty::Custom(crate::game_state::DifficultyParams {
            zeppelins: n,
            ..default()
        }))
        .enemy_count()
    };
    assert_eq!(custom(0) + 1, custom(1), "a zeppelin, alone");
    assert_eq!(custom(9), custom(3), "clamped to three");
}

#[test]
fn gun_assist_is_on_for_the_easy_levels_only() {
    let assist: Vec<bool> = Difficulty::PRESETS
        .iter()
        .map(|d| d.params().gun_assist)
        .collect();
    assert_eq!(assist, vec![true, true, false, false, false]);
}

/// Every mode at an easy and a hard level, scattered with `seed`.
fn scattered_missions(seed: u64) -> Vec<Mission> {
    let mut missions = Vec::new();
    for mode in GameMode::ALL {
        for difficulty in [Difficulty::Normal, Difficulty::Insane] {
            let settings = GameSettings {
                mode,
                difficulty,
                ..default()
            };
            missions.push(Mission::from_settings(&settings).scattered(seed));
        }
    }
    missions
}

/// Lead positions and headings of the groups that place themselves (not
/// escorts), wave by wave.
fn entries(mission: &Mission) -> Vec<(Vec3, Vec3)> {
    mission
        .waves
        .iter()
        .flat_map(|w| w.groups.iter().filter(|g| g.escorts.is_none()))
        .map(|g| (g.lead, g.heading))
        .collect()
}

#[test]
fn waves_enter_somewhere_else_every_seed_but_never_at_the_player() {
    use crate::{
        flight::PLAYER_SPAWN,
        game_state::{MIN_HEAD_ON_ANGLE, SECTORS, bearing_of, clear_of_player},
    };
    let sector = |p: Vec3| (bearing_of(p) / (std::f32::consts::TAU / SECTORS as f32)) as usize;
    for seed in 0..200 {
        let missions = scattered_missions(seed);
        assert_eq!(missions, scattered_missions(seed), "seed {seed} replays");
        for mission in &missions {
            // Scattering moves the waves, nothing else.
            let plain = Mission::from_settings(&GameSettings {
                mode: mission.mode,
                difficulty: mission.difficulty,
                ..default()
            });
            assert_eq!(mission.enemy_count(), plain.enemy_count());
            assert_eq!(mission.waves.len(), plain.waves.len());
            for (lead, _) in entries(mission) {
                assert!(clear_of_player(lead), "seed {seed}: {lead}");
            }
            // Fighter flights: never two in a row from the same sector,
            // never flying straight at the player.
            let fighters: Vec<(Vec3, Vec3)> = mission
                .waves
                .iter()
                .flat_map(|w| &w.groups)
                .filter(|g| g.escorts.is_none() && !g.members[0].is_bomber())
                .map(|g| (g.lead, g.heading))
                .collect();
            for pair in fighters.windows(2) {
                assert_ne!(sector(pair[0].0), sector(pair[1].0), "seed {seed}");
            }
            for (lead, heading) in &fighters {
                let to_player = (PLAYER_SPAWN - *lead).with_y(0.0);
                assert!(
                    heading.angle_between(to_player) >= MIN_HEAD_ON_ANGLE - 1e-3,
                    "seed {seed}: head-on from {lead}"
                );
            }
        }
    }
    let first = |seed| entries(&scattered_missions(seed)[0])[0];
    let placements: Vec<(Vec3, Vec3)> = (0..20).map(first).collect();
    for (i, a) in placements.iter().enumerate() {
        for b in &placements[i + 1..] {
            assert_ne!(a, b, "every seed its own first wave");
        }
    }
}

#[test]
fn escorts_keep_their_place_behind_scattered_bombers() {
    let plain = Mission::battle(Difficulty::Hard);
    for seed in 0..50 {
        let mission = plain.clone().scattered(seed);
        for (before, after) in plain.waves.iter().zip(&mission.waves) {
            let [bombers, escort] = &after.groups[..] else {
                continue;
            };
            let gap = before.groups[1].lead - before.groups[0].lead;
            let now = escort.lead - bombers.lead;
            assert!((gap.length() - now.length()).abs() < 0.1, "seed {seed}");
            assert!((gap.y - now.y).abs() < 0.1, "same height above");
            assert!(now.dot(bombers.heading) < 0.0, "behind the bombers");
            assert!(escort.heading.dot(bombers.heading) > 0.999);
        }
    }
}

#[test]
fn zeppelins_come_alone_between_the_bombers_and_from_another_side() {
    use crate::{
        ai::EnemyKind,
        game_state::{ZEPPELIN_GAP, bearing_gap, bearing_of, zeppelin_slot},
    };
    // Spread over the round, each half an interval after a bomber wave.
    assert_eq!(zeppelin_slot(0, 1, 3), 1.5);
    let insane: Vec<f32> = (0..3).map(|z| zeppelin_slot(z, 3, 4)).collect();
    assert_eq!(insane, vec![0.5, 2.5, 3.5]);
    for seed in 0..100 {
        let mission = Mission::battle(Difficulty::Insane).scattered(seed);
        let times: Vec<f32> = mission
            .waves
            .iter()
            .map(|w| match w.trigger {
                WaveTrigger::AtStart => 0.0,
                WaveTrigger::AfterSecondsOrCleared(t) => t,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(times.windows(2).all(|t| t[0] < t[1]), "in order: {times:?}");
        let mut last_bombers = None;
        for wave in &mission.waves {
            let lead = wave.groups[0].lead;
            if wave.groups[0].members == vec![EnemyKind::Zeppelin] {
                assert_eq!(wave.groups.len(), 1, "alone");
                let bombers: f32 = last_bombers.expect("bombers come first");
                assert!(
                    bearing_gap(bearing_of(lead), bombers) >= ZEPPELIN_GAP,
                    "seed {seed}: zeppelin too close to the bombers' side"
                );
            } else {
                last_bombers = Some(bearing_of(lead));
            }
        }
    }
}

#[test]
fn every_round_scatters_anew_unless_the_seed_is_fixed() {
    let (mut app, _) = app();
    let placed = |app: &App| entries(app.world().resource::<Mission>());
    let fixed = placed(&app);
    app.world_mut().write_message(RestartRound);
    app.update();
    assert_eq!(placed(&app), fixed, "fixed seed: the same again");

    app.insert_resource(SpawnSeed::default());
    let mut seen = vec![fixed];
    for _ in 0..3 {
        app.world_mut().write_message(RestartRound);
        app.update();
        let now = placed(&app);
        assert!(!seen.contains(&now), "a new placement every round");
        seen.push(now);
    }
}

#[test]
fn a_zeppelin_launches_fighters_that_count_as_enemies() {
    use crate::ai::{
        Carrier, EnemyKind, FIRST_LAUNCH_DELAY, FlightGroup, LAUNCH_DROP, LAUNCH_INTERVAL,
    };
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::Custom(crate::game_state::DifficultyParams {
            bomber_waves: 1,
            zeppelins: 1,
            zeppelin_fighters: 2,
            ..default()
        })),
        false,
    );
    let mission = app.world().resource::<Mission>().clone();
    // The bombers first; once they are down the zeppelin comes.
    shoot_down_all(&mut app);
    app.update();
    let zeppelin = enemies(&mut app)[0];
    let kind = |app: &App, e: Entity| app.world().get::<Enemy>(e).unwrap().kind;
    assert_eq!(kind(&app, zeppelin), EnemyKind::Zeppelin);
    let flight = app.world().get::<FlightGroup>(zeppelin).unwrap().id;
    // A hit wakes it, whether it has seen the player or not.
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: zeppelin,
        point: Vec3::ZERO,
        team: Team::Player,
        weapon: Weapon::Gun,
        damage: 1.0,
    });
    run_for(&mut app, FIRST_LAUNCH_DELAY + 0.5);
    let fighters = |app: &mut App| {
        app.world_mut()
            .query::<(Entity, &Enemy, &FlightGroup, &Transform)>()
            .iter(app.world())
            .filter(|(_, e, ..)| e.kind != EnemyKind::Zeppelin)
            .map(|(entity, _, group, tf)| (entity, group.escort, tf.translation))
            .collect::<Vec<_>>()
    };
    let first = fighters(&mut app);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].1, Some(flight), "escorts its zeppelin");
    let carrier_at = app.world().get::<Transform>(zeppelin).unwrap().translation;
    assert!(
        first[0].2.y < carrier_at.y - 0.5 * LAUNCH_DROP,
        "drops from below the hull"
    );
    assert_eq!(
        round(&app).enemies_left(&mission),
        2,
        "zeppelin and fighter"
    );

    // The next one 20 s later; then the hangar waits at two in the air.
    run_for(&mut app, LAUNCH_INTERVAL);
    assert_eq!(fighters(&mut app).len(), 2);
    run_for(&mut app, LAUNCH_INTERVAL + 1.0);
    assert_eq!(fighters(&mut app).len(), 2, "at most two at once");
    assert_eq!(app.world().get::<Carrier>(zeppelin).unwrap().count, 2);

    // One shot down: the replacement comes at once (the interval has
    // passed), and the round counts all of them.
    let down = fighters(&mut app)[0].0;
    app.world_mut().write_message(Hit {
        projectile: Entity::PLACEHOLDER,
        target: down,
        point: Vec3::ZERO,
        team: Team::Player,
        weapon: Weapon::Missile,
        damage: 1_000.0,
    });
    run_for(&mut app, 0.2);
    assert_eq!(fighters(&mut app).len(), 2);
    assert_eq!(round(&app).enemies_left(&mission), 3);

    // Down with everything: the burning zeppelin launches nothing more.
    shoot_down_all(&mut app);
    run_for(&mut app, LAUNCH_INTERVAL + 1.0);
    assert!(fighters(&mut app).is_empty());
    assert_eq!(round(&app).enemies_left(&mission), 0);
}

#[test]
fn the_largest_round_counts_twelve_fighters_per_carrier() {
    use crate::ai::MAX_LAUNCHES_PER_CARRIER;
    let params = crate::game_state::DifficultyParams {
        bomber_waves: 1,
        zeppelins: 1,
        zeppelin_fighters: 6,
        ..default()
    };
    let mission = Mission::battle(Difficulty::Custom(params));
    assert_eq!(MAX_LAUNCHES_PER_CARRIER, 12);
    assert_eq!(mission.max_enemy_count(), mission.enemy_count() + 12);
    let none = Mission::battle(Difficulty::Custom(crate::game_state::DifficultyParams {
        zeppelin_fighters: 0,
        ..params
    }));
    assert_eq!(none.max_enemy_count(), none.enemy_count());
    assert_eq!(
        Mission::battle(Difficulty::Insane).max_enemy_count(),
        35 + 3 * 12
    );
    assert_eq!(
        Mission::hunt(Difficulty::Insane).max_enemy_count(),
        Mission::hunt(Difficulty::Insane).enemy_count()
    );
}

#[test]
fn bombers_and_wrecks_sound_bigger_than_fighters() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::Custom(crate::game_state::DifficultyParams {
            bomber_waves: 1,
            bombers_per_wave: 3,
            flight_size: 2,
            zeppelins: 1,
            zeppelin_fighters: 0,
            ..default()
        })),
        false,
    );
    let cues = |app: &App, sound: Sound| {
        app.world()
            .resource::<Messages<SoundCue>>()
            .iter_current_update_messages()
            .filter(|c| c.sound == sound)
            .count()
    };
    shoot_down_all(&mut app);
    assert_eq!(cues(&app, Sound::BigExplosion), 3, "three bombers");
    assert_eq!(cues(&app, Sound::Explosion), 2, "two escorts");
    // The zeppelin: a big explosion when hit, another when the wreck lands.
    app.update();
    shoot_down_all(&mut app);
    assert_eq!(cues(&app, Sound::BigExplosion), 1);
    let mut impacts = 0;
    for _ in 0..(60.0 / FRAME.as_secs_f32()) as u32 {
        app.update();
        impacts += cues(&app, Sound::WreckImpact);
    }
    assert_eq!(impacts, 1, "the wreck hits the ground once");
}
