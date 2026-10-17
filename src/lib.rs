//! TimeFighter: arcade flight combat.
//!
//! Conventions: 1 unit = 1 metre, +Y up, aircraft noses point along −Z.

pub mod ai;
pub mod aircraft;
pub mod audio;
pub mod camera;
pub mod combat;
pub mod flight;
pub mod game_state;
pub mod hud;
pub mod menu;
pub mod mobile;
pub mod physics;
pub mod score;
pub mod world;

#[cfg(test)]
mod test_support;

use bevy::prelude::*;

/// Longest time step (s) the simulation advances at once (flight model and
/// physics). Below 10 fps the game runs in slow motion instead of letting
/// things jump, e.g. after a hitch or a background tab.
pub const MAX_STEP: f32 = 0.1;

/// Everything the game adds on top of Bevy's `DefaultPlugins`.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            flight::FlightPlugin,
            flight::PlayerFlightPlugin,
            camera::ChaseCameraPlugin,
            game_state::PausePlugin,
            physics::PhysicsPlugin,
            world::WorldPlugin,
        ))
        .add_plugins((
            combat::CombatPlugin,
            combat::CombatVisualsPlugin,
            ai::AiPlugin,
            ai::EnemyVisualsPlugin,
            game_state::RoundPlugin,
            hud::HudPlugin,
            audio::GameAudioPlugin,
            score::ScorePlugin,
            mobile::MobilePlugin,
        ));
    }
}

/// What the app starts: the game, or the aircraft showroom. Both live in the
/// same binary (and the same WebAssembly file), so there is only one build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    Game,
    Showroom,
}

impl AppMode {
    /// Browser: `showroom.html`, or any page with `?showroom` in the query.
    pub fn from_url(path: &str, query: &str) -> AppMode {
        let page = path.rsplit('/').next().unwrap_or_default();
        let flag = query
            .trim_start_matches('?')
            .split('&')
            .any(|part| part == "showroom" || part.starts_with("showroom="));
        if page == "showroom.html" || flag {
            AppMode::Showroom
        } else {
            AppMode::Game
        }
    }

    /// Native: `--showroom` on the command line.
    pub fn from_args(mut args: impl Iterator<Item = String>) -> AppMode {
        if args.any(|arg| arg == "--showroom") {
            AppMode::Showroom
        } else {
            AppMode::Game
        }
    }

    /// Mode of the running app: from the page URL in the browser, from the
    /// command line otherwise.
    pub fn detect() -> AppMode {
        #[cfg(target_arch = "wasm32")]
        {
            let location = web_sys::window().map(|window| window.location());
            let path = location
                .as_ref()
                .and_then(|l| l.pathname().ok())
                .unwrap_or_default();
            let query = location
                .as_ref()
                .and_then(|l| l.search().ok())
                .unwrap_or_default();
            AppMode::from_url(&path, &query)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            AppMode::from_args(std::env::args().skip(1))
        }
    }

    pub fn window_title(self) -> &'static str {
        match self {
            AppMode::Game => "TimeFighter",
            AppMode::Showroom => "TimeFighter - Aircraft showroom",
        }
    }
}

/// The plugins for `mode` on top of `DefaultPlugins`.
pub struct AppModePlugin(pub AppMode);

impl Plugin for AppModePlugin {
    fn build(&self, app: &mut App) {
        match self.0 {
            AppMode::Game => app.add_plugins((GamePlugin, menu::MenuPlugin)),
            AppMode::Showroom => app.add_plugins(aircraft::ShowroomPlugin),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::AppMode;

    #[test]
    fn mode_from_url() {
        assert_eq!(AppMode::from_url("/", ""), AppMode::Game);
        assert_eq!(AppMode::from_url("/index.html", ""), AppMode::Game);
        assert_eq!(
            AppMode::from_url("/demo/showroom.html", ""),
            AppMode::Showroom
        );
        assert_eq!(
            AppMode::from_url("/index.html", "?showroom"),
            AppMode::Showroom
        );
        assert_eq!(AppMode::from_url("/", "?a=1&showroom=1"), AppMode::Showroom);
        assert_eq!(AppMode::from_url("/", "?showroomx"), AppMode::Game);
    }

    #[test]
    fn mode_from_args() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(AppMode::from_args(args(&[]).into_iter()), AppMode::Game);
        assert_eq!(
            AppMode::from_args(args(&["--showroom"]).into_iter()),
            AppMode::Showroom
        );
    }
}
