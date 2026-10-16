//! Game flow: the mission (mode and waves), the round from start to result
//! screen, and pausing.

mod mission;
mod pause;
mod round;
#[cfg(test)]
mod tests;

pub use mission::{Formation, GameMode, Mission, SpawnGroup, WaveSpec, WaveTrigger};
pub use pause::{Pause, PausePlugin};
pub use round::{
    Outcome, RESTART_KEYS, RestartRound, Round, RoundPhase, RoundPlugin, RoundStats, RoundSystems,
    weapons_free,
};
