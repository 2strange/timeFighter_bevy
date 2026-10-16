//! Start menu: mode, difficulty (with custom values) and light, before the
//! first round and from the result screen.
//!
//! The menu writes [`GameSettings`] and starts a fresh round with
//! [`RestartRound`]. While it is open the game is paused (the round behind
//! it stands still as a backdrop). Mouse and keyboard: arrows choose, Enter
//! starts. The selection is remembered in the browser.

mod rows;
mod selection;
mod storage;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{ComputedNode, UiGlobalTransform},
    window::PrimaryWindow,
};

use rows::{Action, Nav, navigate};
pub use selection::{CustomField, MenuMode, MenuSelection, RowId};

use crate::{
    combat::CombatKeys,
    flight::{KeyHelpAppExt, KeyHelpSection},
    game_state::{
        GameSettings, MissionConfig, Pause, RestartRound, Round, RoundPhase, TriggerHold,
    },
    world::{Lighting, LightingSystems},
};

/// Opens the menu from the result screen.
pub const MENU_KEY: KeyCode = KeyCode::KeyM;

const HUD: Color = Color::srgba(0.72, 1.0, 0.78, 0.95);
const HUD_DIM: Color = Color::srgba(0.72, 1.0, 0.78, 0.5);
const DISABLED: Color = Color::srgba(0.6, 0.6, 0.6, 0.45);
const CURSOR: Color = Color::srgb(1.0, 0.75, 0.2);
const PANEL: Color = Color::srgba(0.03, 0.05, 0.07, 0.88);
/// Rows with more choices than this show one value with arrows.
const SPINNER_FROM: usize = 8;

/// Whether the menu is showing, and the row the keyboard cursor is on.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Menu {
    pub open: bool,
    pub cursor: RowId,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            open: true,
            cursor: RowId::Start,
        }
    }
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        let selection = storage::load()
            .map(|line| MenuSelection::from_line(&line))
            .unwrap_or_default();
        app.add_key_help(KeyHelpSection::Round, "M", "menu after a round");
        app.insert_resource(selection)
            .init_resource::<Menu>()
            .add_systems(
                Startup,
                (prepare_first_round, spawn_menu, spawn_result_button),
            )
            .add_systems(
                Update,
                (
                    open_from_result_screen,
                    menu_input
                        .run_if(|menu: Res<Menu>| menu.open)
                        // The light preview applies in the same frame.
                        .before(LightingSystems),
                    sync_pause,
                    rebuild_menu,
                    show_result_button,
                )
                    .chain(),
            );
    }
}

/// The round behind the menu already uses the stored choice (light).
fn prepare_first_round(
    selection: Res<MenuSelection>,
    mut settings: ResMut<GameSettings>,
    mut lighting: ResMut<Lighting>,
    mut mission: ResMut<MissionConfig>,
) {
    *settings = selection.settings();
    *lighting = selection.lighting;
    mission.bomber = selection.bomber;
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuPanel;

/// A clickable choice; `None` index is the start button.
#[derive(Component, Clone, Copy)]
struct MenuButton {
    row: RowId,
    index: Option<usize>,
    enabled: bool,
}

#[derive(Component)]
struct ResultMenuButton;

/// Mouse clicks on UI nodes. Bevy 0.20's picking-based buttons would need
/// the picking feature; a hit test against the laid-out nodes is enough here.
#[derive(SystemParam)]
struct Pointer<'w, 's> {
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl Pointer<'_, '_> {
    /// The cursor in physical pixels, if the left button went down this frame.
    fn click(&self) -> Option<Vec2> {
        let pressed = self
            .mouse
            .as_ref()
            .is_some_and(|m| m.just_pressed(MouseButton::Left));
        if !pressed {
            return None;
        }
        self.windows.single().ok()?.physical_cursor_position()
    }
}

/// Whether a visible node was clicked at `point`.
fn hit(
    point: Vec2,
    node: &ComputedNode,
    at: &UiGlobalTransform,
    visible: &InheritedVisibility,
) -> bool {
    visible.get() && node.contains_point(*at, point)
}

fn keyboard_navigation(keys: &ButtonInput<KeyCode>) -> Option<Nav> {
    let pressed = |codes: &[KeyCode]| keys.any_just_pressed(codes.iter().copied());
    if pressed(&[KeyCode::ArrowUp, KeyCode::KeyW]) {
        Some(Nav::Up)
    } else if pressed(&[KeyCode::ArrowDown, KeyCode::KeyS]) {
        Some(Nav::Down)
    } else if pressed(&[KeyCode::ArrowLeft, KeyCode::KeyA]) {
        Some(Nav::Left)
    } else if pressed(&[KeyCode::ArrowRight, KeyCode::KeyD]) {
        Some(Nav::Right)
    } else if pressed(&[KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) {
        Some(Nav::Confirm)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Pointer,
    buttons: Query<(
        &MenuButton,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    mut menu: ResMut<Menu>,
    mut selection: ResMut<MenuSelection>,
    mut settings: ResMut<GameSettings>,
    mut lighting: ResMut<Lighting>,
    mut restart: MessageWriter<RestartRound>,
    (mut hold, combat, mut mission): (ResMut<TriggerHold>, Res<CombatKeys>, ResMut<MissionConfig>),
) {
    let rows = selection.rows();
    let cursor = rows.iter().position(|r| r.id == menu.cursor).unwrap_or(0);

    let mut action =
        keyboard_navigation(&keys).map_or(Action::None, |nav| navigate(&rows, cursor, nav));
    if let Some(point) = pointer.click() {
        for (button, node, at, visible) in &buttons {
            if button.enabled && hit(point, node, at, visible) {
                action = match button.index {
                    Some(index) => Action::Select(button.row, index),
                    None => Action::Start,
                };
            }
        }
    }

    match action {
        Action::Focus(row) => menu.cursor = row,
        Action::Select(row, index) => {
            menu.cursor = row;
            if selection.select(row, index) && *lighting != selection.lighting {
                // Show the light at once; the menu sits over the city.
                *lighting = selection.lighting;
            }
        }
        Action::Start => {
            *settings = selection.settings();
            mission.bomber = selection.bomber;
            *lighting = selection.lighting;
            storage::save(&selection.to_line());
            restart.write(RestartRound);
            menu.open = false;
            // The Space or click that started must not fire the gun: weapon
            // inputs still down wait until they are released once.
            let weapon_keys: Vec<_> = combat.gun.iter().chain(&combat.missile).copied().collect();
            let weapon_mouse: Vec<_> = combat
                .gun_mouse
                .iter()
                .chain(&combat.missile_mouse)
                .copied()
                .collect();
            hold.hold_pressed(
                (&keys, &weapon_keys),
                pointer
                    .mouse
                    .as_deref()
                    .map(|m| (m, weapon_mouse.as_slice())),
            );
        }
        Action::None => {}
    }
}

fn open_from_result_screen(
    keys: Res<ButtonInput<KeyCode>>,
    round: Option<Res<Round>>,
    pointer: Pointer,
    buttons: Query<
        (&ComputedNode, &UiGlobalTransform, &InheritedVisibility),
        With<ResultMenuButton>,
    >,
    mut menu: ResMut<Menu>,
) {
    let over = round.is_some_and(|r| matches!(r.phase, RoundPhase::Over(_)));
    let clicked = pointer.click().is_some_and(|point| {
        buttons
            .iter()
            .any(|(node, at, visible)| hit(point, node, at, visible))
    });
    if !menu.open && over && (keys.just_pressed(MENU_KEY) || clicked) {
        *menu = Menu {
            open: true,
            cursor: RowId::Start,
        };
    }
}

fn sync_pause(menu: Res<Menu>, mut pause: ResMut<Pause>) {
    if pause.menu != menu.open {
        pause.menu = menu.open;
        if menu.open {
            // The menu replaces the pause screen.
            pause.player = false;
        }
    }
}

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            // Above the HUD and the result screen.
            GlobalZIndex(10),
        ))
        .with_child((
            MenuPanel,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(24)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(HUD_DIM),
        ));
}

fn label(content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// Redraws the panel whenever the selection, cursor or visibility changes.
fn rebuild_menu(
    mut commands: Commands,
    menu: Res<Menu>,
    selection: Res<MenuSelection>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
    panel: Query<Entity, With<MenuPanel>>,
) {
    if !menu.is_changed() && !selection.is_changed() {
        return;
    }
    for mut visibility in &mut root {
        *visibility = if menu.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let Ok(panel) = panel.single() else {
        return;
    };
    commands.entity(panel).despawn_related::<Children>();
    commands.entity(panel).with_children(|panel| {
        panel.spawn(label("TIMEFIGHTER", 40.0, HUD));
        for row in selection.rows() {
            let focused = row.id == menu.cursor;
            if row.is_button() {
                panel
                    .spawn((
                        MenuButton {
                            row: row.id,
                            index: None,
                            enabled: true,
                        },
                        Node {
                            margin: UiRect::top(px(12)),
                            padding: UiRect::axes(px(24), px(8)),
                            border: UiRect::all(px(2)),
                            align_self: AlignSelf::Center,
                            ..default()
                        },
                        BorderColor::all(if focused { CURSOR } else { HUD }),
                    ))
                    .with_child(label(
                        "START  (Enter)",
                        24.0,
                        if focused { CURSOR } else { HUD },
                    ));
                continue;
            }
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn((
                        label(
                            format!("{} {}", if focused { ">" } else { " " }, row.label),
                            18.0,
                            if focused { CURSOR } else { HUD },
                        ),
                        Node {
                            width: px(170),
                            ..default()
                        },
                    ));
                    // Long value lists show as "< value >" instead of every step.
                    if row.choices.len() > SPINNER_FROM {
                        let arrow =
                            |line: &mut ChildSpawnerCommands, text: &str, to: Option<usize>| {
                                line.spawn((
                                    MenuButton {
                                        row: row.id,
                                        index: to,
                                        enabled: to.is_some(),
                                    },
                                    Node {
                                        padding: UiRect::axes(px(8), px(3)),
                                        ..default()
                                    },
                                ))
                                .with_child(label(
                                    text,
                                    16.0,
                                    if to.is_some() { HUD } else { DISABLED },
                                ));
                            };
                        arrow(line, "<", row.step(-1));
                        line.spawn((
                            Node {
                                min_width: px(70),
                                justify_content: JustifyContent::Center,
                                padding: UiRect::axes(px(8), px(3)),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BorderColor::all(HUD),
                        ))
                        .with_child(label(
                            row.choices[row.selected].label.clone(),
                            16.0,
                            HUD,
                        ));
                        arrow(line, ">", row.step(1));
                        return;
                    }
                    for (index, choice) in row.choices.iter().enumerate() {
                        let selected = index == row.selected;
                        let color = if !choice.enabled {
                            DISABLED
                        } else if selected {
                            HUD
                        } else {
                            HUD_DIM
                        };
                        line.spawn((
                            MenuButton {
                                row: row.id,
                                index: Some(index),
                                enabled: choice.enabled,
                            },
                            Node {
                                padding: UiRect::axes(px(8), px(3)),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BorderColor::all(if selected { color } else { Color::NONE }),
                        ))
                        .with_child(label(
                            choice.label.clone(),
                            16.0,
                            color,
                        ));
                    }
                });
        }
        panel.spawn(label(
            "Arrows: choose    Enter: start    Mouse works too",
            14.0,
            HUD_DIM,
        ));
    });
}

fn spawn_result_button(mut commands: Commands) {
    commands
        .spawn((
            ResultMenuButton,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(48),
                left: percent(50),
                margin: UiRect::left(px(-60)),
                width: px(120),
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(HUD),
            BackgroundColor(PANEL),
            GlobalZIndex(5),
            Visibility::Hidden,
        ))
        .with_child(label("MENU  (M)", 18.0, HUD));
}

fn show_result_button(
    menu: Res<Menu>,
    round: Option<Res<Round>>,
    mut button: Query<&mut Visibility, With<ResultMenuButton>>,
) {
    let show = !menu.open && round.is_some_and(|r| matches!(r.phase, RoundPhase::Over(_)));
    for mut visibility in &mut button {
        let wanted = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(wanted);
    }
}

#[cfg(test)]
mod tests;
