//! Game flow: the player's settings (mode, difficulty), the mission
//! (waves), the round from start to result screen, and pausing.

mod mission;
mod pause;
mod round;
mod settings;
#[cfg(test)]
mod tests;

pub use mission::{Formation, GameMode, Mission, SpawnGroup, WaveSpec, WaveTrigger};
pub use pause::{Pause, PausePlugin};
pub use round::{
    Outcome, RESTART_KEYS, RestartRound, Round, RoundPhase, RoundPlugin, RoundStats, RoundSystems,
    weapons_free,
};
pub use settings::{Difficulty, DifficultyParams, GameSettings, PilotMix};
