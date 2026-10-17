//! Start menu: mode, difficulty (with custom values) and light, before the
//! first round and from the result screen.
//!
//! The menu writes [`GameSettings`] and starts a fresh round with
//! [`RestartRound`]. While it is open the game is paused (the round behind
//! it stands still as a backdrop). Mouse and keyboard: arrows choose, Q/E
//! switch the enemy tabs, Enter starts. The selection is remembered in the
//! browser. Custom values come in areas (weapons, general) and tabs (one
//! per kind of enemy), so the panel stays low; on a window lower still it
//! is scaled down.

mod rows;
mod selection;
mod storage;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{ComputedNode, UiGlobalTransform},
    window::PrimaryWindow,
};

use rows::{Action, Nav, Row, navigate};
pub use selection::{CustomField, EnemyTab, MenuMode, MenuSelection, RowId, Section};

use crate::{
    audio::SoundSettings,
    combat::CombatKeys,
    flight::{KeyHelpAppExt, KeyHelpSection, PitchInvert},
    game_state::{GameSettings, Pause, RestartRound, Round, RoundPhase, TriggerHold},
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
const SPINNER_FROM: usize = 6;
/// Text sizes: title, row labels and choices, headings and hints.
const TITLE_SIZE: f32 = 30.0;
const TEXT_SIZE: f32 = 15.0;
const SMALL_SIZE: f32 = 12.0;
/// Width of the row labels, so the choices line up; narrower in the two
/// columns of weapons and general values.
const LABEL_WIDTH: f32 = 130.0;
const COLUMN_LABEL_WIDTH: f32 = 120.0;
/// In the tab box: room for "Zeppelin at the end".
const TAB_LABEL_WIDTH: f32 = 190.0;

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
        let (selection, menu) = after_start_page(selection, storage::start_request().as_deref());
        if menu != Menu::default() {
            storage::save(&selection.to_line());
        }
        app.add_key_help(KeyHelpSection::Round, "M", "menu after a round");
        app.insert_resource(selection)
            .insert_resource(menu)
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
                    follow_sound_toggle,
                    follow_pitch_invert,
                    rebuild_menu,
                    fit_menu_to_window,
                    show_result_button,
                )
                    .chain(),
            );
    }
}

/// What the start page asked for: with a fixed level the game starts right
/// away (menu closed, the first round uses the selection); with Custom the
/// menu opens on the custom values. Without a request: the menu as usual.
pub fn after_start_page(
    mut selection: MenuSelection,
    request: Option<&str>,
) -> (MenuSelection, Menu) {
    let Some(request) = request else {
        return (selection, Menu::default());
    };
    selection.apply_request(request);
    let menu = if selection.is_custom() {
        let first_custom = selection
            .rows()
            .iter()
            .find(|row| matches!(row.id, RowId::Custom(_)))
            .map_or(RowId::Start, |row| row.id);
        Menu {
            open: true,
            cursor: first_custom,
        }
    } else {
        Menu {
            open: false,
            cursor: RowId::Start,
        }
    };
    (selection, menu)
}

/// The round behind the menu already uses the stored choice (light).
fn prepare_first_round(
    selection: Res<MenuSelection>,
    mut settings: ResMut<GameSettings>,
    mut lighting: ResMut<Lighting>,
    sound: Option<ResMut<SoundSettings>>,
    invert: Option<ResMut<PitchInvert>>,
) {
    *settings = selection.settings();
    *lighting = selection.lighting;
    if let Some(mut invert) = invert {
        invert.set_if_neq(PitchInvert(selection.invert_pitch));
    }
    if let Some(mut sound) = sound {
        apply_sound(&selection, &mut sound);
    }
}

/// The menu's Sound row onto the sound: a volume, or off.
fn apply_sound(selection: &MenuSelection, sound: &mut SoundSettings) {
    match selection.sound_volume() {
        Some(volume) => {
            sound.enabled = true;
            sound.volume = volume;
        }
        None => sound.enabled = false,
    }
}

/// The sound switched with its key (N) shows in the Sound row, and is
/// remembered like a choice in the menu.
fn follow_sound_toggle(sound: Option<Res<SoundSettings>>, mut selection: ResMut<MenuSelection>) {
    let Some(sound) = sound.filter(|s| s.is_changed()) else {
        return;
    };
    let wanted = match (sound.enabled, selection.sound) {
        (false, _) => 0,
        (true, 0) => MenuSelection::sound_step(sound.volume),
        (true, step) => step,
    };
    if selection.sound != wanted {
        selection.sound = wanted;
        storage::save(&selection.to_line());
    }
}

/// "Invert pitch" switched elsewhere (the phone overlay's pause screen)
/// shows in the menu row and is remembered.
fn follow_pitch_invert(invert: Option<Res<PitchInvert>>, mut selection: ResMut<MenuSelection>) {
    let Some(invert) = invert.filter(|i| i.is_changed()) else {
        return;
    };
    if selection.invert_pitch != invert.0 {
        selection.invert_pitch = invert.0;
        storage::save(&selection.to_line());
    }
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
    touches: Option<Res<'w, Touches>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl Pointer<'_, '_> {
    /// Where the left button or a finger went down this frame, in
    /// physical pixels.
    fn click(&self) -> Option<Vec2> {
        let window = self.windows.single().ok()?;
        if let Some(touch) = self
            .touches
            .as_ref()
            .and_then(|t| t.iter_just_pressed().next())
        {
            return Some(touch.position() * window.scale_factor());
        }
        let pressed = self
            .mouse
            .as_ref()
            .is_some_and(|m| m.just_pressed(MouseButton::Left));
        if !pressed {
            return None;
        }
        window.physical_cursor_position()
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
    (mut hold, combat): (ResMut<TriggerHold>, Res<CombatKeys>),
    mut sound: Option<ResMut<SoundSettings>>,
    mut invert: Option<ResMut<PitchInvert>>,
) {
    let rows = selection.rows();
    let cursor = rows.iter().position(|r| r.id == menu.cursor).unwrap_or(0);

    // Q/E switch the enemy tab from any row.
    if keys.just_pressed(KeyCode::KeyQ) {
        selection.switch_tab(-1);
    } else if keys.just_pressed(KeyCode::KeyE) {
        selection.switch_tab(1);
    }
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
            let changed = selection.select(row, index);
            if changed && *lighting != selection.lighting {
                // Show the light at once; the menu sits over the city.
                *lighting = selection.lighting;
            }
            if changed
                && row == RowId::Sound
                && let Some(sound) = sound.as_deref_mut()
            {
                apply_sound(&selection, sound);
            }
            if changed
                && row == RowId::InvertPitch
                && let Some(invert) = invert.as_deref_mut()
            {
                *invert = PitchInvert(selection.invert_pitch);
            }
        }
        Action::Start => {
            *settings = selection.settings();
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
    // The cursor's row went away (another tab, mode or level): to the tab
    // bar if there is one, else to Start.
    let rows = selection.rows();
    if menu.open && !rows.iter().any(|r| r.id == menu.cursor) {
        menu.cursor = if rows.iter().any(|r| r.id == RowId::Tabs) {
            RowId::Tabs
        } else {
            RowId::Start
        };
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
                row_gap: px(4),
                padding: UiRect::axes(px(22), px(16)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(HUD_DIM),
            UiTransform::IDENTITY,
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

/// Redraws the panel whenever the selection, cursor or visibility changes:
/// the title with the modes beside it, the level, then (with Custom) the
/// weapons and general values side by side and the tab bar over a box with
/// the open tab's values, and the start button.
fn rebuild_menu(
    mut commands: Commands,
    menu: Res<Menu>,
    selection: Res<MenuSelection>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
    panel: Query<Entity, With<MenuPanel>>,
    (mobile, mut was_touch): (Option<Res<crate::mobile::MobileInput>>, Local<bool>),
) {
    // Touch screens get tap wording instead of keys.
    let touch = crate::mobile::touch(mobile.as_deref());
    let touch_changed = touch != *was_touch;
    *was_touch = touch;
    if !menu.is_changed() && !selection.is_changed() && !touch_changed {
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
    // Consecutive rows of one section form a block.
    let rows = selection.rows();
    let mut blocks: Vec<(Section, Vec<&Row<RowId>>)> = Vec::new();
    for row in &rows {
        match blocks.last_mut() {
            Some((section, block)) if *section == row.id.section() => block.push(row),
            _ => blocks.push((row.id.section(), vec![row])),
        }
    }
    let cursor = menu.cursor;
    // An area with its heading, as one column.
    let area = |parent: &mut ChildSpawnerCommands, section: Section, block: &[&Row<RowId>]| {
        parent
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|column| {
                if let Some(heading) = section.heading() {
                    column.spawn(label(heading, SMALL_SIZE, HUD_DIM));
                }
                for row in block {
                    spawn_row(column, row, cursor == row.id, Some(COLUMN_LABEL_WIDTH));
                }
            });
    };
    commands.entity(panel).despawn_related::<Children>();
    commands.entity(panel).with_children(|panel| {
        // Weapons and general values side by side.
        let areas: Vec<_> = blocks
            .iter()
            .filter(|(section, _)| matches!(section, Section::Weapons | Section::General))
            .cloned()
            .collect();
        for (section, block) in blocks {
            match section {
                Section::Top => {
                    for row in block {
                        if row.id == RowId::Mode {
                            // Title and modes on one line.
                            panel
                                .spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: px(16),
                                    margin: UiRect::bottom(px(4)),
                                    ..default()
                                })
                                .with_children(|line| {
                                    line.spawn(label("TIMEFIGHTER", TITLE_SIZE, HUD));
                                    spawn_row(line, row, menu.cursor == row.id, None);
                                });
                        } else {
                            spawn_row(panel, row, menu.cursor == row.id, Some(LABEL_WIDTH));
                        }
                    }
                }
                Section::TabBar => {
                    for row in block {
                        spawn_tab_bar(panel, row, menu.cursor == row.id);
                    }
                }
                Section::Tab(_) => {
                    panel
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(4),
                                padding: UiRect::all(px(8)),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BorderColor::all(HUD_DIM),
                        ))
                        .with_children(|tab| {
                            for row in block {
                                spawn_row(tab, row, menu.cursor == row.id, Some(TAB_LABEL_WIDTH));
                            }
                        });
                }
                Section::Bottom => {
                    for row in block {
                        spawn_start_button(panel, row, menu.cursor == row.id, touch);
                    }
                }
                // Without Custom only the light: a plain row.
                Section::General if areas.len() == 1 => {
                    for row in block {
                        spawn_row(panel, row, menu.cursor == row.id, Some(LABEL_WIDTH));
                    }
                }
                Section::Weapons => {
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(28),
                            margin: UiRect::top(px(6)),
                            ..default()
                        })
                        .with_children(|columns| {
                            for (section, block) in &areas {
                                area(columns, *section, block);
                            }
                        });
                }
                Section::General => {}
            }
        }
        panel.spawn(label(
            if touch {
                "Tap a value to choose it, START to fly"
            } else {
                "Arrows: choose    Q/E: tabs    Enter: start    Mouse works too"
            },
            SMALL_SIZE,
            HUD_DIM,
        ));
    });
}

/// One row: the label (when `label_width` is given, with the cursor mark)
/// and its choices, or for long lists one value between arrows.
fn spawn_row(
    parent: &mut ChildSpawnerCommands,
    row: &Row<RowId>,
    focused: bool,
    label_width: Option<f32>,
) {
    let mark = if focused { ">" } else { " " };
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            ..default()
        })
        .with_children(|line| {
            let text = match label_width {
                Some(_) => format!("{mark} {}", row.label),
                None => mark.to_owned(),
            };
            line.spawn((
                label(text, TEXT_SIZE, if focused { CURSOR } else { HUD }),
                TextLayout::no_wrap(),
                Node {
                    width: label_width.map_or(Val::Auto, px),
                    ..default()
                },
            ));
            if row.choices.len() > SPINNER_FROM {
                spawn_spinner(line, row);
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
                        padding: UiRect::axes(px(7), px(2)),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BorderColor::all(if selected { color } else { Color::NONE }),
                ))
                .with_children(|button| choice_text(button, &choice.label, color));
            }
        });
}

/// "< value >" for rows with many steps.
fn spawn_spinner(line: &mut ChildSpawnerCommands, row: &Row<RowId>) {
    let arrow = |line: &mut ChildSpawnerCommands, text: &str, to: Option<usize>| {
        line.spawn((
            MenuButton {
                row: row.id,
                index: to,
                enabled: to.is_some(),
            },
            Node {
                padding: UiRect::axes(px(7), px(2)),
                ..default()
            },
        ))
        .with_child(label(
            text,
            TEXT_SIZE,
            if to.is_some() { HUD } else { DISABLED },
        ));
    };
    arrow(line, "<", row.step(-1));
    line.spawn((
        Node {
            min_width: px(70),
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(7), px(2)),
            border: UiRect::all(px(1)),
            ..default()
        },
        BorderColor::all(HUD),
    ))
    .with_children(|value| choice_text(value, &row.choices[row.selected].label, HUD));
    arrow(line, ">", row.step(1));
}

/// A choice's text; the infinity sign is drawn as two rings, since the
/// built-in font has only ASCII.
fn choice_text(parent: &mut ChildSpawnerCommands, text: &str, color: Color) {
    if text != selection::INFINITY {
        parent.spawn(label(text, TEXT_SIZE, color));
        return;
    }
    let ring = TEXT_SIZE * 0.5;
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            height: px(TEXT_SIZE * 1.2),
            padding: UiRect::horizontal(px(2)),
            ..default()
        })
        .with_children(|sign| {
            for overlap in [0.0, -1.5] {
                sign.spawn((
                    Node {
                        width: px(ring),
                        height: px(ring),
                        margin: UiRect::left(px(overlap)),
                        border: UiRect::all(px(1.5)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(color),
                ));
            }
        });
}

/// The enemy tabs: the open one framed, those the mode does not use greyed.
fn spawn_tab_bar(parent: &mut ChildSpawnerCommands, row: &Row<RowId>, focused: bool) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::End,
            column_gap: px(4),
            margin: UiRect::top(px(8)),
            ..default()
        })
        .with_children(|bar| {
            bar.spawn(label(
                if focused { ">" } else { " " },
                TEXT_SIZE,
                if focused { CURSOR } else { HUD },
            ));
            for (index, choice) in row.choices.iter().enumerate() {
                let open = index == row.selected;
                let color = match (choice.enabled, open, focused) {
                    (false, ..) => DISABLED,
                    (true, true, true) => CURSOR,
                    (true, true, false) => HUD,
                    (true, false, _) => HUD_DIM,
                };
                bar.spawn((
                    MenuButton {
                        row: row.id,
                        index: Some(index),
                        enabled: choice.enabled,
                    },
                    Node {
                        padding: UiRect::axes(px(12), px(4)),
                        border: UiRect {
                            left: px(1),
                            right: px(1),
                            top: px(if open { 2.0 } else { 1.0 }),
                            bottom: px(0),
                        },
                        ..default()
                    },
                    BorderColor::all(if open { color } else { DISABLED }),
                    BackgroundColor(if open {
                        Color::srgba(0.72, 1.0, 0.78, 0.08)
                    } else {
                        Color::NONE
                    }),
                ))
                .with_child(label(choice.label.clone(), TEXT_SIZE, color));
            }
        });
}

fn spawn_start_button(
    parent: &mut ChildSpawnerCommands,
    row: &Row<RowId>,
    focused: bool,
    touch: bool,
) {
    let color = if focused { CURSOR } else { HUD };
    parent
        .spawn((
            MenuButton {
                row: row.id,
                index: None,
                enabled: true,
            },
            Node {
                margin: UiRect::vertical(px(8)),
                padding: UiRect::axes(px(24), px(6)),
                border: UiRect::all(px(2)),
                align_self: AlignSelf::Center,
                ..default()
            },
            BorderColor::all(color),
        ))
        .with_child(label(
            if touch { "START" } else { "START  (Enter)" },
            22.0,
            color,
        ));
}

/// Scale of the panel so it fits `available` (physical px, height and
/// width) with a small margin; never enlarged.
fn fit_scale(content: Vec2, available: Vec2) -> f32 {
    if content.x <= 0.0 || content.y <= 0.0 {
        return 1.0;
    }
    let room = available * 0.96 / content;
    room.x.min(room.y).min(1.0)
}

/// Scales the panel down on windows too low (or narrow) for it; the layout
/// keeps its size, so this does not feed back. Clicks still hit, the hit
/// test uses the transformed nodes.
fn fit_menu_to_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut panel: Query<(&ComputedNode, &mut UiTransform), With<MenuPanel>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let available = Vec2::new(
        window.physical_width() as f32,
        window.physical_height() as f32,
    );
    for (node, mut transform) in &mut panel {
        let scale = Vec2::splat(fit_scale(node.size(), available));
        if transform.scale != scale {
            transform.scale = scale;
        }
    }
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
    mobile: Option<Res<crate::mobile::MobileInput>>,
) {
    // Touch screens have the overlay's own MENU button.
    let show = !menu.open
        && round.is_some_and(|r| matches!(r.phase, RoundPhase::Over(_)))
        && !crate::mobile::touch(mobile.as_deref());
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
