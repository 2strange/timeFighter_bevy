//! Helpers for tests that need the full plugin set without window or GPU.

use bevy::{
    asset::AssetMetaCheck,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    },
    log::LogPlugin,
    prelude::*,
    render::{RenderPlugin, settings::WgpuSettings},
    window::ExitCondition,
    winit::WinitPlugin,
};

/// `DefaultPlugins` like the real app, but no window, no GPU, no logging,
/// plus `plugin`.
pub fn headless_app(plugin: impl Plugin) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .disable::<LogPlugin>()
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: None,
                    ..default()
                }
                .into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                meta_check: AssetMetaCheck::Never,
                ..default()
            }),
    )
    .add_plugins(plugin);
    app.finish();
    app.cleanup();
    app
}

/// Sends a key event as the window would.
pub fn key(app: &mut App, key_code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

/// Presses and releases a key over two updates.
pub fn tap(app: &mut App, key_code: KeyCode) {
    key(app, key_code, ButtonState::Pressed);
    app.update();
    key(app, key_code, ButtonState::Released);
    app.update();
}
