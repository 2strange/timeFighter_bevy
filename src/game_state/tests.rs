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
    Difficulty, GameMode, GameSettings, Mission, MissionConfig, Outcome, Pause, PausePlugin,
    RestartRound, Round, RoundPhase, RoundPlugin, WaveTrigger,
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

/// The layout most tests below assume: two flights of four Spitfires (the
/// first set), rookies then veterans, at designed speed.
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
    // The tests below were written for the first set; those about the
    // second set insert their own configuration.
    app.insert_resource(MissionConfig {
        set: crate::game_state::EnemySet::First,
        ..default()
    });
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
fn ramming_a_spitfire_loses_the_round_but_counts_the_kill() {
    let (mut app, jet) = app();
    let mission = app.world().resource::<Mission>().clone();
    let enemy = enemies(&mut app)[0];
    // Put one Spitfire right in the jet's path.
    let jet_tf = *app.world().get::<Transform>(jet).unwrap();
    let in_path = Transform::from_translation(jet_tf.translation + jet_tf.forward() * 4.0)
        .with_rotation(jet_tf.rotation);
    *app.world_mut().get_mut::<Transform>(enemy).unwrap() = in_path;
    run_for(&mut app, mission.end_delay + 0.5);

    let round = round(&app);
    assert_eq!(
        round.phase,
        RoundPhase::Over(Outcome::Crashed(CrashInto::Aircraft("Spitfire")))
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
        let mission = Mission::hunt(difficulty, &MissionConfig::default());
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
    let mission = Mission::hunt(custom, &MissionConfig::default());
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
    let mission = Mission::hunt(custom, &MissionConfig::default());
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
        let mission = Mission::protect(difficulty, &MissionConfig::default());
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
fn protect_flies_the_configured_bomber() {
    for bomber in [crate::ai::EnemyKind::Lancaster, crate::ai::EnemyKind::B17] {
        let (mut app, _) = custom_app(
            FlightConfig::default().spawn_transform(),
            protect(Difficulty::SuperEasy),
            true,
        );
        app.insert_resource(crate::game_state::MissionConfig {
            set: crate::game_state::EnemySet::First,
            bomber,
        });
        app.world_mut().write_message(RestartRound);
        app.update();
        let kinds: Vec<_> = app
            .world_mut()
            .query::<(&Enemy, &crate::combat::Turrets)>()
            .iter(app.world())
            .map(|(e, t)| (e.kind, t.mounts.len()))
            .collect();
        assert_eq!(kinds.len(), 4, "{bomber:?}");
        // Front and rear gunners from the model's mounts.
        assert!(kinds.iter().all(|k| *k == (bomber, 2)), "{kinds:?}");
    }
    assert_eq!(
        crate::game_state::MissionConfig::default().bomber,
        crate::ai::EnemyKind::Lancaster
    );
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
    assert_eq!(protect, vec![24, 16, 16, 12, 12]);
    for difficulty in Difficulty::PRESETS {
        assert_eq!(
            difficulty.params_for(GameMode::Hunt),
            difficulty.params(),
            "Hunt unchanged"
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
    assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 16);
    app.insert_resource(GameSettings {
        difficulty: Difficulty::Normal,
        ..default()
    });
    app.world_mut().write_message(RestartRound);
    app.update();
    assert_eq!(
        app.world().get::<MissileRack>(jet).unwrap().count,
        8,
        "Hunt"
    );
}

#[test]
fn the_second_set_flies_mixed_pairs_and_ju_390_led_bombers() {
    use crate::{ai::EnemyKind, game_state::EnemySet};
    let config = MissionConfig {
        set: EnemySet::Second,
        ..default()
    };
    let hunt = Mission::hunt(Difficulty::Hard, &config);
    assert_eq!(
        hunt.waves[0].groups[0].members,
        vec![
            EnemyKind::Bf109K4,
            EnemyKind::Me309,
            EnemyKind::Bf109K4,
            EnemyKind::Me309
        ]
    );
    let protect = Mission::protect(Difficulty::Normal, &config);
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
    // The first set stays as it was.
    let first = Mission::hunt(
        Difficulty::Hard,
        &MissionConfig {
            set: EnemySet::First,
            ..default()
        },
    );
    assert!(
        first.waves[0].groups[0]
            .members
            .iter()
            .all(|k| *k == EnemyKind::Spitfire)
    );
}

#[test]
fn the_set_is_chosen_by_url_or_command_line() {
    use crate::game_state::EnemySet;
    assert_eq!(
        MissionConfig::from_query("?set=second").set,
        EnemySet::Second
    );
    assert_eq!(
        MissionConfig::from_query("?a=1&set=second").set,
        EnemySet::Second
    );
    assert_eq!(
        MissionConfig::from_query("--showroom --set=second").set,
        EnemySet::Second
    );
    // The second set is the default; the first only on request.
    assert_eq!(MissionConfig::from_query("?set=first").set, EnemySet::First);
    assert_eq!(MissionConfig::from_query("").set, EnemySet::Second);
    assert_eq!(MissionConfig::default().set, EnemySet::Second);
    assert_eq!(
        MissionConfig::from_query("?set=nonsense").set,
        EnemySet::Second
    );
}

#[test]
fn a_second_set_round_spawns_the_mixed_flight() {
    let (mut app, _) = app();
    app.insert_resource(MissionConfig {
        set: crate::game_state::EnemySet::Second,
        ..default()
    });
    app.world_mut().write_message(RestartRound);
    app.update();
    let mut kinds: Vec<_> = app
        .world_mut()
        .query::<&Enemy>()
        .iter(app.world())
        .map(|e| format!("{:?}", e.kind))
        .collect();
    kinds.sort();
    assert_eq!(kinds, vec!["Bf109K4", "Bf109K4", "Me309", "Me309"]);
    run_for(&mut app, 5.0);
    assert_eq!(round(&app).phase, RoundPhase::Running);
}

#[test]
fn a_second_set_protect_round_sends_a_ju_390_with_he_177s() {
    let (mut app, _) = custom_app(
        FlightConfig::default().spawn_transform(),
        protect(Difficulty::SuperEasy),
        true,
    );
    app.insert_resource(MissionConfig {
        set: crate::game_state::EnemySet::Second,
        ..default()
    });
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
    use crate::{ai::EnemyKind, game_state::EnemySet};
    let totals: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| Mission::battle(*d, &MissionConfig::default()).enemy_count())
        .collect();
    // SE 2×(4+2), Easy 2×(4+2), Normal 3×(4+2), Hard 3×(4+4), Insane 4×(4+4).
    assert_eq!(totals, vec![12, 12, 18, 24, 32]);
    for difficulty in Difficulty::PRESETS {
        let params = difficulty.params();
        let mission = Mission::battle(difficulty, &MissionConfig::default());
        assert_eq!(mission.mode, GameMode::Battle);
        assert_eq!(mission.city_loss, Some(params.city_loss));
        for wave in &mission.waves {
            let [bombers, escort] = &wave.groups[..] else {
                panic!("two groups per wave");
            };
            assert!(bombers.members.iter().all(|k| k.is_bomber()));
            assert_eq!(bombers.count(), params.bombers_per_wave);
            assert!(escort.members.iter().all(|k| !k.is_bomber()));
            assert_eq!(escort.count(), params.flight_size);
            assert_eq!(escort.escorts, Some(0));
        }
    }
    let second = Mission::battle(
        Difficulty::Hard,
        &MissionConfig {
            set: EnemySet::Second,
            ..default()
        },
    );
    assert_eq!(second.waves[0].groups[0].members[0], EnemyKind::Ju390);
    assert_eq!(
        second.waves[0].groups[1].members,
        vec![
            EnemyKind::Bf109K4,
            EnemyKind::Me309,
            EnemyKind::Bf109K4,
            EnemyKind::Me309
        ]
    );
    assert!(GameMode::ALL.contains(&GameMode::Battle));
}

#[test]
fn battle_missiles_follow_the_two_for_a_bomber_one_for_a_fighter_rule() {
    let missiles: Vec<u32> = Difficulty::PRESETS
        .iter()
        .map(|d| d.params_for(GameMode::Battle).missiles)
        .collect();
    assert_eq!(missiles, vec![24, 20, 24, 16, 16]);
}

#[test]
fn a_battle_round_is_won_when_everything_is_down() {
    let (mut app, jet) = custom_app(
        FlightConfig::default().spawn_transform(),
        battle(Difficulty::SuperEasy),
        true,
    );
    assert_eq!(app.world().get::<MissileRack>(jet).unwrap().count, 24);
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
            (Some(500), 1.0, 4, 4),
        ]
    );
    assert_eq!(Difficulty::SuperEasy.params().bomber_waves, 2);

    // The round loads the gun accordingly.
    for (difficulty, ammo) in [
        (Difficulty::SuperEasy, None),
        (Difficulty::Normal, Some(2_000)),
        (Difficulty::Insane, Some(500)),
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
