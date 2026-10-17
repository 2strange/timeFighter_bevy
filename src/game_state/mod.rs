//! Game flow: the player's settings (mode, difficulty), the mission
//! (waves), the round from start to result screen, and pausing.

mod mission;
mod pause;
mod round;
mod settings;
#[cfg(test)]
mod tests;

pub use mission::{
    Formation, GameMode, MIN_AXIS_OFFSET, MIN_HEAD_ON_ANGLE, MIN_PLAYER_DISTANCE, Mission, SECTORS,
    SpawnGroup, WaveSpec, WaveTrigger, ZEPPELIN_GAP, bearing_gap, bearing_of, bomber_flight,
    clear_of_player, fighter_flight, zeppelin_slot,
};
pub use pause::{Pause, PausePlugin, TriggerHold};
pub use round::{
    Outcome, RESTART_KEYS, RestartRound, Round, RoundPhase, RoundPlugin, RoundStats, RoundSystems,
    SpawnSeed, weapons_free,
};
pub use settings::{
    Difficulty, DifficultyParams, GameSettings, MAX_ZEPPELIN_FIGHTERS, MAX_ZEPPELINS, PilotMix,
};
