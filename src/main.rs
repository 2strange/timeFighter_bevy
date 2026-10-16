use bevy::prelude::*;
use timefighter::GamePlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
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
        }))
        .add_plugins(GamePlugin)
        .run();
}
