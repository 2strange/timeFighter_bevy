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
    Difficulty, GameSettings, Mission, Outcome, Pause, PausePlugin, RestartRound, Round,
    RoundPhase, RoundPlugin,
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
    // Hard: two flights of four, the layout most tests below assume.
    app.insert_resource(GameSettings {
        difficulty: Difficulty::Hard,
        ..default()
    });
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
        vec![(20, 4, 2), (10, 6, 2), (8, 8, 2), (6, 8, 4), (6, 12, 4)]
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
