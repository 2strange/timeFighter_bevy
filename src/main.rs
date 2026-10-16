use bevy::{asset::AssetMetaCheck, log::LogPlugin, prelude::*};
use timefighter::{AppMode, AppModePlugin};

fn main() {
    let mode = AppMode::detect();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    // Assets ship without .meta files; skip probing for them
                    // (on the web every probe is a failing HTTP request).
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: mode.window_title().into(),
                        // Browser: render into <canvas id="game"> and fill its parent.
                        canvas: Some("#game".into()),
                        fit_canvas_to_parent: true,
                        // Keep arrow keys and space from scrolling the page.
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(log_settings()),
        )
        .add_plugins(AppModePlugin(mode))
        .run();
}

/// The browser console gets warnings and errors, plus the game's own info
/// lines (e.g. the city fingerprint for comparing builds).
fn log_settings() -> LogPlugin {
    if cfg!(target_arch = "wasm32") {
        LogPlugin {
            level: bevy::log::Level::WARN,
            filter: "wgpu=error,timefighter=info".into(),
            ..default()
        }
    } else {
        LogPlugin::default()
    }
}
