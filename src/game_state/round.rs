//! Round flow: start, waves, victory or defeat, result screen, restart.
//!
//! Headless; the result screen itself is drawn by the HUD. On the result
//! screen game time stands still ([`Pause::round_over`]).

use bevy::prelude::*;

use super::{GameSettings, Mission, MissionConfig, Pause, TriggerHold, WaveTrigger};
use crate::{
    ai::{AttackSlots, Enemy, FlightGroup, bomber_bundle, enemy_bundle},
    combat::{
        Bomb, BombImpact, CombatConfig, CombatSystems, CrashInto, DestroyCause, Destroyed, Effect,
        Hit, Munition, ShotFired, Team, Weapon, player_loadout,
    },
    flight::{
        FlightConfig, FlightState, FlightSystems, KeyHelpAppExt, KeyHelpSection, PlayerAircraft,
    },
    physics::{CrashSweep, EnemyCrashed, PlayerCrashed, ProjectileHit},
    world::{
        Lighting, LightingSystems,
        damage::{CityDamageSystems, CityHealth, DamageArea, RepairCity},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Victory,
    ShotDown,
    /// Flew into something, or was rammed.
    Crashed(CrashInto),
    /// Protect: the bombers destroyed too much of the city centre.
    CityDestroyed,
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

/// Waiting flights circle this far from the map centre (m), on the
/// bearing they came from.
const HOLDING_DISTANCE: f32 = 2_200.0;

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
        )
        // The city's damage plugin registers it too.
        .add_message::<RepairCity>();
        app.init_resource::<GameSettings>()
            .init_resource::<MissionConfig>()
            .init_resource::<Mission>()
            .init_resource::<Round>()
            .add_message::<RestartRound>()
            .configure_sets(Update, CombatSystems::Fire.run_if(weapons_free))
            .configure_sets(
                Update,
                (
                    RoundSystems::Start
                        .before(FlightSystems::Input)
                        // The repair applies in the restart frame.
                        .before(CityDamageSystems)
                        .before(CombatSystems::Input),
                    RoundSystems::Track
                        .after(CombatSystems::Resolve)
                        // City health is current when the round decides.
                        .after(CityDamageSystems),
                    // Bombs land (combat) before the city takes the damage,
                    // all in one frame.
                    CityDamageSystems.after(CombatSystems::Resolve),
                ),
            )
            .add_systems(
                Update,
                (
                    (restart, drop_stale_messages, start_round)
                        .chain()
                        .in_set(RoundSystems::Start),
                    // Before the world applies the lighting, same frame.
                    apply_lighting_setting.before(LightingSystems),
                    (count_events, spawn_waves, decide, advance_phase)
                        .chain()
                        .in_set(RoundSystems::Track),
                ),
            );
    }
}

/// Weapons fire only while the round runs, the game is not paused, and no
/// input held over from the menu is still down (see [`TriggerHold`]).
pub fn weapons_free(
    round: Option<Res<Round>>,
    pause: Option<Res<Pause>>,
    hold: Option<Res<TriggerHold>>,
) -> bool {
    round.is_some_and(|r| r.is_running())
        && !pause.is_some_and(|p| p.is_paused())
        && !hold.is_some_and(|h| h.is_holding())
}

/// On a restart, drops hits, crashes and shots still queued from the old
/// round (for example from its last physics step), so none of them reach
/// the new round's statistics, damage or victory count.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drop_stale_messages(
    round: Res<Round>,
    projectile_hits: Option<ResMut<Messages<ProjectileHit>>>,
    player_crashes: Option<ResMut<Messages<PlayerCrashed>>>,
    enemy_crashes: Option<ResMut<Messages<EnemyCrashed>>>,
    hits: Option<ResMut<Messages<Hit>>>,
    destroyed: Option<ResMut<Messages<Destroyed>>>,
    shots: Option<ResMut<Messages<ShotFired>>>,
    city: (
        Option<ResMut<Messages<DamageArea>>>,
        Option<ResMut<Messages<BombImpact>>>,
    ),
) {
    if round.phase != RoundPhase::Starting {
        return;
    }
    // Damage still queued from the old round's last bombs would otherwise
    // land on the repaired city.
    clear(city.0);
    clear(city.1);
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
    pause: Option<Res<Pause>>,
    mut requests: MessageReader<RestartRound>,
    mut round: ResMut<Round>,
) {
    let requested = requests.read().count() > 0;
    // The keys act on the result screen only, and not while something else
    // holds the game (the player's pause, a menu using the same keys).
    let held_otherwise = pause.is_some_and(|p| {
        Pause {
            round_over: false,
            ..*p
        }
        .is_paused()
    });
    let key = matches!(round.phase, RoundPhase::Over(_))
        && !held_otherwise
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
    mission_config: Res<MissionConfig>,
    mut mission: ResMut<Mission>,
    mut combat: ResMut<CombatConfig>,
    mut slots: ResMut<AttackSlots>,
    mut repair: MessageWriter<RepairCity>,
    flight: Option<Res<FlightConfig>>,
    pause: Option<ResMut<Pause>>,
    leftovers: Query<Entity, Or<(With<Enemy>, With<Munition>, With<Effect>, With<Bomb>)>>,
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
    *mission = Mission::from_settings(&settings, &mission_config);
    // The whole city stands again. Applies this frame: the start runs
    // before the city's damage systems.
    repair.write(RepairCity);
    let params = settings.difficulty.params_for(settings.mode).clamped();
    combat.player_missiles = params.missiles;
    combat.player_gun_unlimited = params.unlimited_gun;
    combat.lock.time = params.lock_time;
    slots.max = params.attackers;
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
            WaveTrigger::AfterSecondsOrCleared(t) => {
                round.elapsed >= t || round.enemies_alive() == 0
            }
        };
        if !due {
            break;
        }
        for (g, group) in wave.groups.iter().enumerate() {
            let id = (round.next_wave * 16 + g) as u32;
            let holding = FlightGroup::holding_point(group.lead, HOLDING_DISTANCE);
            for (slot, transform) in group.transforms().enumerate() {
                let flight = FlightGroup {
                    id,
                    slot: slot as u32,
                    offset: group.formation.slot(slot as u32),
                    holding,
                };
                if group.kind.is_bomber() {
                    commands.spawn((
                        bomber_bundle(group.kind, group.tuning, transform, id),
                        flight,
                    ));
                } else {
                    commands.spawn((
                        enemy_bundle(group.kind, group.skill, group.tuning, transform),
                        flight,
                    ));
                }
            }
        }
        round.spawned += wave.enemy_count();
        round.next_wave += 1;
    }
}

/// Victory once every wave has entered and every enemy is down.
fn decide(mission: Res<Mission>, health: Option<Res<CityHealth>>, mut round: ResMut<Round>) {
    // Protect: too much of the centre in ruins loses, before anything else.
    if round.is_running()
        && let (Some(loss), Some(health)) = (mission.city_loss, &health)
        && health.center_intact_ratio <= 1.0 - loss
    {
        round.phase = RoundPhase::Ending {
            outcome: Outcome::CityDestroyed,
            left: mission.end_delay,
        };
        return;
    }
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
