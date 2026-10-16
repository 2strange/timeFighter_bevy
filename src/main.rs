use bevy::{asset::AssetMetaCheck, prelude::*};
use timefighter::GamePlugin;

fn main() {
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
                        title: "TimeFighter".into(),
                        // Browser: render into <canvas id="game"> and fill its parent.
                        canvas: Some("#game".into()),
                        fit_canvas_to_parent: true,
                        // Keep arrow keys and space from scrolling the page.
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(GamePlugin)
        .run();
}
