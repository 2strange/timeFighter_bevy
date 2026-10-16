//! Round flow: start, waves, victory or defeat, result screen, restart.
//!
//! Headless; the result screen itself is drawn by the HUD. On the result
//! screen game time stands still ([`Pause::round_over`]).

use bevy::prelude::*;

use super::{GameSettings, Mission, Pause, WaveTrigger};
use crate::{
    ai::{Enemy, enemy_bundle},
    combat::{
        CombatConfig, CombatSystems, CrashInto, DestroyCause, Destroyed, Effect, Hit, Munition,
        ShotFired, Team, Weapon, player_loadout,
    },
    flight::{
        FlightConfig, FlightState, FlightSystems, KeyHelpAppExt, KeyHelpSection, PlayerAircraft,
    },
    physics::{CrashSweep, EnemyCrashed, PlayerCrashed, ProjectileHit},
    world::Lighting,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Victory,
    ShotDown,
    /// Flew into something, or was rammed.
    Crashed(CrashInto),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum RoundPhase {
    /// Resets everything and spawns the first wave on the next update.
    #[default]
    Starting,
    Running,
    /// Decided; the world keeps moving for the mission's end delay.
    Ending {
        outcome: Outcome,
        left: f32,
    },
    /// Result screen, game time stopped. Enter or R restarts.
    Over(Outcome),
}

/// The player's numbers for the result screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundStats {
    pub kills: u32,
    pub gun_rounds: u32,
    pub gun_hits: u32,
    pub missiles_fired: u32,
    pub missile_hits: u32,
}

impl RoundStats {
    /// Share of everything fired (rounds and missiles) that hit, if
    /// anything was fired.
    pub fn hit_rate(&self) -> Option<f32> {
        let fired = self.gun_rounds + self.missiles_fired;
        (fired > 0).then(|| (self.gun_hits + self.missile_hits) as f32 / fired as f32)
    }

    pub fn gun_accuracy(&self) -> Option<f32> {
        (self.gun_rounds > 0).then(|| self.gun_hits as f32 / self.gun_rounds as f32)
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Round {
    pub phase: RoundPhase,
    /// Game time since the start, only while running (s).
    pub elapsed: f32,
    /// Index of the next wave to enter.
    pub next_wave: usize,
    pub spawned: u32,
    pub destroyed: u32,
    pub stats: RoundStats,
}

impl Round {
    pub fn is_running(&self) -> bool {
        self.phase == RoundPhase::Running
    }

    pub fn outcome(&self) -> Option<Outcome> {
        match self.phase {
            RoundPhase::Ending { outcome, .. } | RoundPhase::Over(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// Enemies still to shoot down, including waves not yet arrived.
    pub fn enemies_left(&self, mission: &Mission) -> u32 {
        mission.enemy_count().saturating_sub(self.destroyed)
    }

    fn enemies_alive(&self) -> u32 {
        self.spawned.saturating_sub(self.destroyed)
    }
}

/// Starts a fresh round, whatever the current phase.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestartRound;

/// Keys that restart the round from the result screen.
pub const RESTART_KEYS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::KeyR];

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RoundSystems {
    /// Restart and reset, first thing in the frame: nothing of the old
    /// round is simulated or counted after it.
    Start,
    /// Counting, waves, victory and defeat, after combat resolved the frame.
    Track,
}

/// Headless round flow. Needs [`crate::combat::CombatPlugin`] and
/// [`crate::ai::AiPlugin`].
pub struct RoundPlugin;

impl Plugin for RoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_key_help(
            KeyHelpSection::Round,
            "Enter / R",
            "fly again after a round",
        );
        app.init_resource::<GameSettings>()
            .init_resource::<Mission>()
            .init_resource::<Round>()
            .add_message::<RestartRound>()
            .configure_sets(Update, CombatSystems::Fire.run_if(weapons_free))
            .configure_sets(
                Update,
                (
                    RoundSystems::Start
                        .before(FlightSystems::Input)
                        .before(CombatSystems::Input),
                    RoundSystems::Track.after(CombatSystems::Resolve),
                ),
            )
            .add_systems(
                Update,
                (
                    (restart, drop_stale_messages, start_round)
                        .chain()
                        .in_set(RoundSystems::Start),
                    apply_lighting_setting,
                    (count_events, spawn_waves, decide, advance_phase)
                        .chain()
                        .in_set(RoundSystems::Track),
                ),
            );
    }
}

/// Weapons fire only while the round runs and the game is not paused.
pub fn weapons_free(round: Option<Res<Round>>, pause: Option<Res<Pause>>) -> bool {
    round.is_some_and(|r| r.is_running()) && !pause.is_some_and(|p| p.is_paused())
}

/// On a restart, drops hits, crashes and shots still queued from the old
/// round (for example from its last physics step), so none of them reach
/// the new round's statistics, damage or victory count.
#[allow(clippy::too_many_arguments)]
fn drop_stale_messages(
    round: Res<Round>,
    projectile_hits: Option<ResMut<Messages<ProjectileHit>>>,
    player_crashes: Option<ResMut<Messages<PlayerCrashed>>>,
    enemy_crashes: Option<ResMut<Messages<EnemyCrashed>>>,
    hits: Option<ResMut<Messages<Hit>>>,
    destroyed: Option<ResMut<Messages<Destroyed>>>,
    shots: Option<ResMut<Messages<ShotFired>>>,
) {
    if round.phase != RoundPhase::Starting {
        return;
    }
    clear(projectile_hits);
    clear(player_crashes);
    clear(enemy_crashes);
    clear(hits);
    clear(destroyed);
    clear(shots);
}

fn clear<M: Message>(messages: Option<ResMut<Messages<M>>>) {
    if let Some(mut messages) = messages {
        messages.clear();
    }
}

fn restart(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut requests: MessageReader<RestartRound>,
    mut round: ResMut<Round>,
) {
    let requested = requests.read().count() > 0;
    let key = matches!(round.phase, RoundPhase::Over(_))
        && keys.is_some_and(|k| k.any_just_pressed(RESTART_KEYS));
    if requested || key {
        round.phase = RoundPhase::Starting;
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn start_round(
    mut commands: Commands,
    mut round: ResMut<Round>,
    settings: Res<GameSettings>,
    mut mission: ResMut<Mission>,
    mut combat: ResMut<CombatConfig>,
    flight: Option<Res<FlightConfig>>,
    pause: Option<ResMut<Pause>>,
    leftovers: Query<Entity, Or<(With<Enemy>, With<Munition>, With<Effect>)>>,
    mut player: Query<
        (
            Entity,
            &mut FlightState,
            &mut Transform,
            Option<&mut CrashSweep>,
        ),
        With<PlayerAircraft>,
    >,
) {
    if round.phase != RoundPhase::Starting {
        return;
    }
    if let Some(mut pause) = pause {
        pause.round_over = false;
    }
    // The player's choices apply from this round on.
    *mission = Mission::from_settings(&settings);
    let params = settings.difficulty.params().clamped();
    combat.player_missiles = params.missiles;
    combat.player_gun_unlimited = params.unlimited_gun;
    for entity in &leftovers {
        commands.entity(entity).despawn();
    }
    for (jet, mut state, mut transform, sweep) in &mut player {
        commands.entity(jet).insert(player_loadout(&combat));
        if let Some(flight) = &flight {
            *state = FlightState::new(flight);
            *transform = flight.spawn_transform();
        }
        // Crash detection reports one crash per aircraft; arm it again at
        // the spawn point (also when the crash happened close to it).
        if let Some(mut sweep) = sweep {
            sweep.reset_at(&transform);
        }
    }
    *round = Round {
        phase: RoundPhase::Running,
        ..default()
    };
}

fn count_events(
    mission: Res<Mission>,
    mut round: ResMut<Round>,
    mut shots: MessageReader<ShotFired>,
    mut hits: MessageReader<Hit>,
    mut destroyed: MessageReader<Destroyed>,
) {
    let counting = matches!(round.phase, RoundPhase::Running | RoundPhase::Ending { .. });
    for shot in shots.read().filter(|s| s.team == Team::Player) {
        if !counting {
            continue;
        }
        match shot.weapon {
            Weapon::Gun => round.stats.gun_rounds += 1,
            Weapon::Missile => round.stats.missiles_fired += 1,
        }
    }
    for hit in hits.read().filter(|h| h.team == Team::Player) {
        if !counting {
            continue;
        }
        match hit.weapon {
            Weapon::Gun => round.stats.gun_hits += 1,
            Weapon::Missile => round.stats.missile_hits += 1,
        }
    }
    for event in destroyed.read() {
        match event.team {
            Team::Enemy => {
                round.destroyed += 1;
                if counting {
                    round.stats.kills += 1;
                }
            }
            Team::Player if round.is_running() => {
                let outcome = match event.cause {
                    DestroyCause::Crash(into) => Outcome::Crashed(into),
                    DestroyCause::Gun | DestroyCause::Missile => Outcome::ShotDown,
                };
                round.phase = RoundPhase::Ending {
                    outcome,
                    left: mission.end_delay,
                };
            }
            Team::Player => {}
        }
    }
}

fn spawn_waves(mut commands: Commands, mission: Res<Mission>, mut round: ResMut<Round>) {
    if !round.is_running() {
        return;
    }
    while let Some(wave) = mission.waves.get(round.next_wave) {
        let due = match wave.trigger {
            WaveTrigger::AtStart => true,
            WaveTrigger::AfterSeconds(t) => round.elapsed >= t,
            WaveTrigger::WhenEnemiesAtMost(n) => round.enemies_alive() <= n,
        };
        if !due {
            break;
        }
        for group in &wave.groups {
            for transform in group.transforms() {
                commands.spawn(enemy_bundle(
                    group.kind,
                    group.skill,
                    group.tuning,
                    transform,
                ));
            }
        }
        round.spawned += wave.enemy_count();
        round.next_wave += 1;
    }
}

/// Victory once every wave has entered and every enemy is down.
fn decide(mission: Res<Mission>, mut round: ResMut<Round>) {
    if round.is_running() && round.next_wave >= mission.waves.len() && round.enemies_alive() == 0 {
        round.phase = RoundPhase::Ending {
            outcome: Outcome::Victory,
            left: mission.end_delay,
        };
    }
}

fn advance_phase(time: Res<Time>, pause: Option<ResMut<Pause>>, round: ResMut<Round>) {
    let dt = time.delta_secs();
    let round = round.into_inner();
    match &mut round.phase {
        RoundPhase::Running => round.elapsed += dt,
        RoundPhase::Ending { outcome, left } => {
            *left -= dt;
            if *left <= 0.0 {
                let outcome = *outcome;
                round.phase = RoundPhase::Over(outcome);
                if let Some(mut pause) = pause {
                    pause.round_over = true;
                }
            }
        }
        RoundPhase::Starting | RoundPhase::Over(_) => {}
    }
}

/// The lighting chosen in the menu replaces the current one when the
/// choice changes (the L key still toggles in between).
fn apply_lighting_setting(settings: Res<GameSettings>, lighting: Option<ResMut<Lighting>>) {
    if let Some(mut lighting) = lighting
        && settings.is_changed()
    {
        lighting.set_if_neq(settings.lighting);
    }
}
