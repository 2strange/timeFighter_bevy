//! Round flow: start, waves, victory or defeat, result screen, restart.
//!
//! Headless; the result screen itself is drawn by the HUD. On the result
//! screen game time stands still ([`Pause::round_over`]).

use bevy::prelude::*;

use super::{Mission, Pause, WaveTrigger};
use crate::{
    ai::{Enemy, enemy_bundle},
    combat::{
        CombatConfig, CombatSystems, DestroyCause, Destroyed, Hit, Munition, ShotFired, Team,
        Weapon, player_loadout,
    },
    flight::{FlightConfig, FlightState, PlayerAircraft},
    physics::CrashSweep,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Victory,
    ShotDown,
    Crashed,
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
pub struct RoundSystems;

/// Headless round flow. Needs [`crate::combat::CombatPlugin`] and
/// [`crate::ai::AiPlugin`].
pub struct RoundPlugin;

impl Plugin for RoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mission>()
            .init_resource::<Round>()
            .add_message::<RestartRound>()
            .configure_sets(Update, CombatSystems::Fire.run_if(weapons_free))
            .configure_sets(Update, RoundSystems.after(CombatSystems::Resolve))
            .add_systems(
                Update,
                (
                    restart,
                    start_round,
                    count_events,
                    spawn_waves,
                    decide,
                    advance_phase,
                )
                    .chain()
                    .in_set(RoundSystems),
            );
    }
}

/// Weapons fire only while the round runs.
pub fn weapons_free(round: Option<Res<Round>>) -> bool {
    round.is_some_and(|r| r.is_running())
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
    combat: Res<CombatConfig>,
    flight: Option<Res<FlightConfig>>,
    pause: Option<ResMut<Pause>>,
    leftovers: Query<Entity, Or<(With<Enemy>, With<Munition>)>>,
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
                let outcome = if event.cause == DestroyCause::Crash {
                    Outcome::Crashed
                } else {
                    Outcome::ShotDown
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
                commands.spawn(enemy_bundle(group.kind, transform));
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
