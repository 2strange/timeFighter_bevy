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
                    primary_window: Some(primary_window(mode)),
                    ..default()
                })
                .set(log_settings()),
        )
        .add_plugins(AppModePlugin(mode))
        .run();
}

fn primary_window(mode: AppMode) -> Window {
    let mut window = Window {
        title: mode.window_title().into(),
        // Browser: render into <canvas id="game"> and fill its parent.
        canvas: Some("#game".into()),
        fit_canvas_to_parent: true,
        // Keep arrow keys and space from scrolling the page.
        prevent_default_event_handling: true,
        ..default()
    };
    if let Some(scale) = touch_pixel_ratio_cap() {
        window.resolution.set_scale_factor_override(Some(scale));
    }
    window
}

/// Phones render at most at twice the CSS pixels: a 3x screen would cost
/// more than twice the pixels for a picture the eye barely tells apart.
fn touch_pixel_ratio_cap() -> Option<f32> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let coarse = window.match_media("(pointer: coarse)").ok()??.matches();
        let ratio = window.device_pixel_ratio() as f32;
        (coarse && ratio > 2.0).then_some(2.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
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
