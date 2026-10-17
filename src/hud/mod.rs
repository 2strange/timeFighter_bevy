//! Head-up display: flight data, weapons, enemies left, target markers
//! with lock state, gun cross, pitch ladder, lead pipper, hit feedback and
//! the result screen.
//!
//! Fighter-jet style, kept calm: thin lines, one colour for normal
//! information, amber while the seeker tracks, red for a lock or damage.
//! The gun cross sits on the nose line; the pitch ladder is conformal (its
//! rungs lie on the real horizon and elevation lines), so it shows pitch
//! and bank at a glance and rolls with the view. Only the rungs near the
//! current pitch are shown.

mod layout;
#[cfg(test)]
mod tests;

use bevy::{platform::collections::HashSet, prelude::*};

pub use layout::{edge_point, format_time, gun_target, outcome_title};

use crate::{
    ai::Enemy,
    camera::{CameraSystems, ChaseCamera},
    combat::{
        AircraftVelocity, CombatConfig, CombatKeys, Destroyed, GoingDown, Gun, Health, Hit, LockOn,
        LockState, MissileRack, Team, gun_lead, in_seeker_cone, key_labels,
    },
    flight::{FlightState, PlayerAircraft},
    game_state::{Mission, Round, RoundPhase, RoundSystems},
    world::damage::CityHealth,
};

const HUD: Color = Color::srgba(0.72, 1.0, 0.78, 0.78);
const HUD_DIM: Color = Color::srgba(0.72, 1.0, 0.78, 0.32);
const TRACKING: Color = Color::srgb(1.0, 0.75, 0.2);
const ALERT: Color = Color::srgb(1.0, 0.25, 0.2);

/// Distance of the gun reticle's aim point ahead of the jet (m), about
/// where the cannon's rounds still fly straight and fast.
const RETICLE_RANGE: f32 = 800.0;
/// Inset of off-screen target indicators from the screen edge (px).
const EDGE_MARGIN: f32 = 28.0;
const MARKER_SIZE: f32 = 30.0;
/// Gun cross: overall size and the open gap in its centre (px).
const CROSS_SIZE: f32 = 34.0;
const CROSS_GAP: f32 = 10.0;
const LINE: f32 = 1.5;
/// Pitch ladder: rung spacing, how far from the current pitch rungs show
/// (degrees), rung length (px) and the distance its points are placed at.
const LADDER_STEP: i32 = 10;
const LADDER_SHOWN: f32 = 25.0;
/// Rung length as a share of the screen width: the horizon about a third,
/// the other rungs a quarter. Their centre gap leaves the gun cross free.
const HORIZON_WIDTH: f32 = 0.33;
const RUNG_WIDTH: f32 = 0.25;
/// Gap in the middle of a rung, as a share of its length.
const RUNG_GAP: f32 = 0.3;
const LADDER_RANGE: f32 = 1_000.0;
const PIPPER_SIZE: f32 = 12.0;
/// The lead pipper considers targets up to this far off the nose (rad).
const PIPPER_CONE: f32 = 30.0 * std::f32::consts::PI / 180.0;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feedback>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    collect_feedback,
                    update_texts,
                    sync_markers,
                    place_reticle_and_markers,
                    place_ladder,
                    place_pipper,
                    update_result_screen,
                )
                    .chain()
                    .after(CameraSystems)
                    .after(RoundSystems::Track),
            );
    }
}

/// Short-lived highlights, in seconds left.
#[derive(Resource, Default, Debug)]
struct Feedback {
    /// Player's round or missile struck: reticle flashes.
    hit: f32,
    /// An enemy went down: "SPLASH" under the reticle.
    kill: f32,
    /// Player was hit: hull readout turns red.
    damage: f32,
}

#[derive(Component)]
struct HudRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum HudText {
    Status,
    Speed,
    Altitude,
    Gun,
    Missiles,
    Hull,
    Lock,
    Splash,
}

/// The gun cross on the nose line (where the gun points).
#[derive(Component)]
struct Reticle;

/// One of the gun cross's lines; flashes on a hit.
#[derive(Component)]
struct CrossLine;

/// A pitch ladder rung at `pitch` degrees (0 = horizon).
#[derive(Component)]
struct Rung {
    pitch: i32,
}

/// Lead pipper: where to put the gun reticle so the rounds meet the target.
#[derive(Component)]
struct Pipper;

/// Box around an enemy on screen, or a dot at the screen edge pointing to it.
#[derive(Component)]
struct TargetMarker {
    enemy: Entity,
}

#[derive(Component)]
struct TargetLabel;

#[derive(Component)]
struct ResultScreen;

#[derive(Component)]
struct ResultTitle;

#[derive(Component)]
struct ResultBody;

fn text(content: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn spawn_hud(mut commands: Commands, keys: Res<CombatKeys>) {
    let hint = |keys: &[KeyCode]| format!("[{}]", key_labels(keys));
    commands
        .spawn((
            HudRoot,
            Name::new("HUD"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            // Below the pause screen.
            GlobalZIndex(-10),
        ))
        .with_children(|hud| {
            // Top centre: mode, enemies left, time.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                top: px(12),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_child((HudText::Status, text("", 20.0, HUD)));

            // Speed left and altitude right of the centre.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                top: percent(50),
                width: percent(100),
                margin: UiRect::top(px(-11)),
                justify_content: JustifyContent::Center,
                column_gap: px(320),
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    width: px(150),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_child((HudText::Speed, text("", 20.0, HUD)));
                row.spawn(Node {
                    width: px(150),
                    justify_content: JustifyContent::FlexStart,
                    ..default()
                })
                .with_child((HudText::Altitude, text("", 20.0, HUD)));
            });

            // Bottom left: weapons.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(16),
                bottom: px(16),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|panel| {
                // Each weapon with its key, so nobody has to look it up.
                let row = Node {
                    column_gap: px(10),
                    align_items: AlignItems::Center,
                    ..default()
                };
                panel.spawn(row.clone()).with_children(|row| {
                    row.spawn((HudText::Gun, text("", 20.0, HUD)));
                    row.spawn(text(&hint(&keys.gun), 15.0, HUD_DIM));
                });
                panel.spawn(row.clone()).with_children(|row| {
                    row.spawn((HudText::Missiles, text("", 20.0, HUD)));
                    row.spawn(text(&hint(&keys.missile), 15.0, HUD_DIM));
                });
                panel.spawn(row).with_children(|row| {
                    row.spawn(text("next target", 15.0, HUD_DIM));
                    row.spawn(text(&hint(&keys.next_target), 15.0, HUD_DIM));
                });
            });

            // Bottom right: hull.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(16),
                bottom: px(16),
                ..default()
            })
            .with_child((HudText::Hull, text("", 20.0, HUD)));

            // Pitch ladder, behind everything else.
            for pitch in (-90..=90).step_by(LADDER_STEP as usize) {
                spawn_rung(hud, pitch);
            }

            // Gun cross: four lines around an open centre and a dot, with
            // lock state and kill call below it.
            hud.spawn((
                Reticle,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(CROSS_SIZE),
                    height: px(CROSS_SIZE),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|reticle| {
                let arm = (CROSS_SIZE - CROSS_GAP) / 2.0;
                let mid = (CROSS_SIZE - LINE) / 2.0;
                for (left, top, width, height) in [
                    (0.0, mid, arm, LINE),
                    (CROSS_SIZE - arm, mid, arm, LINE),
                    (mid, 0.0, LINE, arm),
                    (mid, CROSS_SIZE - arm, LINE, arm),
                    (mid, mid, LINE, LINE),
                ] {
                    reticle.spawn((
                        CrossLine,
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(left),
                            top: px(top),
                            width: px(width),
                            height: px(height),
                            ..default()
                        },
                        BackgroundColor(HUD),
                    ));
                }
                reticle
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        top: px(CROSS_SIZE + 6.0),
                        left: px(CROSS_SIZE / 2.0 - 100.0),
                        width: px(200),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|below| {
                        below.spawn((HudText::Lock, text("", 16.0, TRACKING)));
                        below.spawn((HudText::Splash, text("", 18.0, HUD)));
                    });
            });

            // Lead pipper.
            hud.spawn((
                Pipper,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(PIPPER_SIZE),
                    height: px(PIPPER_SIZE),
                    border: UiRect::all(px(LINE)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(TRACKING),
                Visibility::Hidden,
            ));

            // Result screen.
            hud.spawn((
                ResultScreen,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    row_gap: px(16),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                Visibility::Hidden,
            ))
            .with_children(|screen| {
                screen.spawn((
                    ResultTitle,
                    text("", 52.0, Color::WHITE),
                    TextLayout::justify(Justify::Center),
                ));
                screen.spawn((
                    ResultBody,
                    text("", 22.0, Color::srgb(0.9, 0.9, 0.9)),
                    TextLayout::justify(Justify::Center),
                ));
                screen.spawn(text("R: fly again", 18.0, Color::srgb(0.75, 0.75, 0.75)));
            });
        });
}

fn collect_feedback(
    time: Res<Time>,
    mut feedback: ResMut<Feedback>,
    mut hits: MessageReader<Hit>,
    mut destroyed: MessageReader<Destroyed>,
) {
    let dt = time.delta_secs();
    feedback.hit = (feedback.hit - dt).max(0.0);
    feedback.kill = (feedback.kill - dt).max(0.0);
    feedback.damage = (feedback.damage - dt).max(0.0);
    for hit in hits.read() {
        match hit.team {
            Team::Player => feedback.hit = 0.15,
            Team::Enemy => feedback.damage = 0.4,
        }
    }
    if destroyed.read().any(|d| d.team == Team::Enemy) {
        feedback.kill = 1.2;
    }
}

#[allow(clippy::type_complexity)]
fn update_texts(
    round: Res<Round>,
    mission: Res<Mission>,
    city: Option<Res<CityHealth>>,
    feedback: Res<Feedback>,
    going_down: Query<&Enemy, With<GoingDown>>,
    player: Query<
        (
            &FlightState,
            &Transform,
            Option<&Gun>,
            Option<&MissileRack>,
            Option<&Health>,
            Option<&LockOn>,
        ),
        With<PlayerAircraft>,
    >,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
) {
    let Ok((state, transform, gun, rack, health, lock)) = player.single() else {
        return;
    };
    for (kind, mut text, mut color) in &mut texts {
        let (content, tint) = match kind {
            HudText::Status => {
                // Protect: the city's centre as a percentage, red near the limit.
                let city = mission
                    .city_loss
                    .zip(city.as_deref())
                    .map(|(loss, health)| (health.center_intact_ratio, 1.0 - loss));
                let city_text = city.map_or(String::new(), |(intact, _)| {
                    format!("    CITY {:.0} %", intact * 100.0)
                });
                // A burning wreck on its way down: where will it hit?
                let falling = going_down.iter().next().map_or(String::new(), |e| {
                    format!("    {} GOING DOWN", e.kind.display_name().to_uppercase())
                });
                let tint = match city {
                    _ if !falling.is_empty() => ALERT,
                    Some((intact, limit)) if intact < limit + 0.1 => ALERT,
                    _ => HUD,
                };
                (
                    format!(
                        "{} ({})    ENEMIES {}{}    {}{}",
                        mission.mode.display_name(),
                        mission.difficulty.display_name().to_uppercase(),
                        round.enemies_left(&mission),
                        city_text,
                        format_time(round.elapsed),
                        falling
                    ),
                    tint,
                )
            }
            HudText::Speed => (format!("{:>4.0} km/h", state.speed * 3.6), HUD),
            HudText::Altitude => (format!("{:.0} m", transform.translation.y), HUD),
            HudText::Gun => match gun.map(|g| g.ammo) {
                Some(Some(ammo)) => (
                    format!("GUN {ammo:>4}"),
                    if ammo == 0 { ALERT } else { HUD },
                ),
                Some(None) => ("GUN  INF".into(), HUD),
                None => (String::new(), HUD),
            },
            HudText::Missiles => match rack {
                Some(rack) => (
                    format!(
                        "MSL {}{}",
                        "|".repeat(rack.count as usize),
                        ".".repeat(rack.capacity.saturating_sub(rack.count) as usize)
                    ),
                    if rack.count == 0 { ALERT } else { HUD },
                ),
                None => (String::new(), HUD),
            },
            HudText::Hull => match health {
                Some(health) => (
                    format!("HULL {:>3.0} %", health.fraction() * 100.0),
                    if feedback.damage > 0.0 || health.fraction() < 0.3 {
                        ALERT
                    } else {
                        HUD
                    },
                ),
                None => (String::new(), HUD),
            },
            HudText::Lock => match lock.map(|l| l.state) {
                Some(LockState::Tracking { .. }) => ("TRACKING".into(), TRACKING),
                Some(LockState::Locked { .. }) => ("LOCK".into(), ALERT),
                _ => (String::new(), TRACKING),
            },
            HudText::Splash => (
                if feedback.kill > 0.0 {
                    "SPLASH".into()
                } else {
                    String::new()
                },
                HUD,
            ),
        };
        if text.0 != content {
            text.0 = content;
        }
        color.set_if_neq(TextColor(tint));
    }
}

/// One marker per living enemy.
fn sync_markers(
    mut commands: Commands,
    root: Query<Entity, With<HudRoot>>,
    enemies: Query<Entity, With<Enemy>>,
    markers: Query<(Entity, &TargetMarker)>,
) {
    let Ok(root) = root.single() else {
        return;
    };
    for (marker, target) in &markers {
        if enemies.get(target.enemy).is_err() {
            commands.entity(marker).despawn();
        }
    }
    let marked: HashSet<Entity> = markers.iter().map(|(_, m)| m.enemy).collect();
    for enemy in &enemies {
        if marked.contains(&enemy) {
            continue;
        }
        commands.entity(root).with_children(|hud| {
            hud.spawn((
                TargetMarker { enemy },
                Node {
                    position_type: PositionType::Absolute,
                    width: px(MARKER_SIZE),
                    height: px(MARKER_SIZE),
                    border: UiRect::all(px(LINE)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BorderColor::all(HUD_DIM),
                Visibility::Hidden,
            ))
            .with_children(|marker| {
                marker.spawn((
                    TargetLabel,
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(MARKER_SIZE + 2.0),
                        ..default()
                    },
                    text("", 14.0, TRACKING),
                ));
            });
        });
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_reticle_and_markers(
    round: Res<Round>,
    combat: Res<CombatConfig>,
    feedback: Res<Feedback>,
    camera: Query<(&Camera, &Transform), With<ChaseCamera>>,
    player: Query<(&Transform, Option<&LockOn>), With<PlayerAircraft>>,
    enemies: Query<&Transform, With<Enemy>>,
    mut reticle: Query<(&mut Node, &mut Visibility), (With<Reticle>, Without<TargetMarker>)>,
    mut cross_lines: Query<&mut BackgroundColor, With<CrossLine>>,
    mut markers: Query<
        (
            &TargetMarker,
            &mut Node,
            &mut Visibility,
            &mut BorderColor,
            &Children,
        ),
        Without<Reticle>,
    >,
    mut labels: Query<(&mut Text, &mut TextColor), With<TargetLabel>>,
) {
    let (Ok((camera, camera_tf)), Ok((jet, lock))) = (camera.single(), player.single()) else {
        return;
    };
    // The chase camera is a root entity, so its Transform is its global
    // transform; using it here avoids lagging a frame behind the camera.
    let camera_gt = GlobalTransform::from(*camera_tf);
    let Some(viewport) = camera.logical_viewport_size() else {
        return;
    };
    let playing = round.outcome().is_none();

    if let Ok((mut node, mut visibility)) = reticle.single_mut() {
        let aim = jet.translation + jet.forward() * RETICLE_RANGE;
        match camera.world_to_viewport(&camera_gt, aim) {
            Ok(p) if playing => {
                node.left = px(p.x - CROSS_SIZE / 2.0);
                node.top = px(p.y - CROSS_SIZE / 2.0);
                visibility.set_if_neq(Visibility::Visible);
                let tint = if feedback.hit > 0.0 {
                    Color::WHITE
                } else {
                    HUD
                };
                for mut line in &mut cross_lines {
                    line.set_if_neq(BackgroundColor(tint));
                }
            }
            _ => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }

    let lock_state = lock.map(|l| l.state).unwrap_or_default();
    let forward = jet.forward().as_vec3();
    for (marker, mut node, mut visibility, mut border, children) in &mut markers {
        let Ok(enemy) = enemies.get(marker.enemy) else {
            continue;
        };
        if !playing {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let to_enemy = enemy.translation - jet.translation;
        let (tint, size, label) = match lock_state {
            LockState::Locked { target } if target == marker.enemy => (
                ALERT,
                MARKER_SIZE,
                format!("LOCK {:.1} km", to_enemy.length() / 1000.0),
            ),
            LockState::Tracking { target, progress } if target == marker.enemy => (
                TRACKING,
                // Closes in on the target while the seeker works.
                MARKER_SIZE * (1.8 - 0.8 * progress),
                format!("{:.1} km", to_enemy.length() / 1000.0),
            ),
            _ if in_seeker_cone(forward, to_enemy, combat.lock.cone, combat.lock.range) => {
                (HUD, MARKER_SIZE, String::new())
            }
            _ => (HUD_DIM, MARKER_SIZE, String::new()),
        };

        let on_screen = camera
            .world_to_viewport(&camera_gt, enemy.translation)
            .ok()
            .filter(|p| p.cmpge(Vec2::ZERO).all() && p.cmple(viewport).all());
        let (center, size, label) = match on_screen {
            Some(p) => (p, size, label),
            None => {
                // Off screen: a small dot at the edge in the enemy's direction.
                let view = camera_gt
                    .affine()
                    .inverse()
                    .transform_point3(enemy.translation);
                (
                    edge_point(viewport, Vec2::new(view.x, -view.y), EDGE_MARGIN),
                    10.0,
                    String::new(),
                )
            }
        };
        node.left = px(center.x - size / 2.0);
        node.top = px(center.y - size / 2.0);
        node.width = px(size);
        node.height = px(size);
        visibility.set_if_neq(Visibility::Visible);
        border.set_if_neq(BorderColor::all(tint));
        for child in children {
            if let Ok((mut text, mut color)) = labels.get_mut(*child) {
                if text.0 != label {
                    text.0 = label.clone();
                }
                color.set_if_neq(TextColor(tint));
            }
        }
    }
}

fn spawn_rung(hud: &mut ChildSpawnerCommands, pitch: i32) {
    // Width is set per frame from the screen size; parts are shares of it.
    let half = percent((1.0 - RUNG_GAP) / 2.0 * 100.0);
    hud.spawn((
        Rung { pitch },
        Node {
            position_type: PositionType::Absolute,
            height: px(LINE),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        UiTransform::default(),
        Visibility::Hidden,
    ))
    .with_children(|rung| {
        for side in [-1.0, 1.0] {
            // Above the horizon solid lines, below dashed (as in real HUDs).
            rung.spawn(Node {
                width: half,
                height: px(LINE),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|segment| {
                let (dashes, dash) = if pitch < 0 { (3, 20.0) } else { (1, 100.0) };
                for _ in 0..dashes {
                    segment.spawn((
                        Node {
                            width: percent(dash),
                            height: px(LINE),
                            ..default()
                        },
                        BackgroundColor(HUD_DIM),
                    ));
                }
            });
            if pitch != 0 {
                // Pitch number just outside the outer end.
                let mut label = Node {
                    position_type: PositionType::Absolute,
                    top: px(-8.0),
                    ..default()
                };
                if side < 0.0 {
                    label.right = percent(100);
                    label.margin = UiRect::right(px(5));
                } else {
                    label.left = percent(100);
                    label.margin = UiRect::left(px(5));
                }
                rung.spawn((label, text(&pitch.abs().to_string(), 12.0, HUD_DIM)));
            }
        }
    });
}

/// Places the ladder rungs near the current pitch on their real elevation
/// lines, rotated with the horizon as the camera sees it.
#[allow(clippy::type_complexity)]
fn place_ladder(
    round: Res<Round>,
    camera: Query<(&Camera, &Transform), With<ChaseCamera>>,
    player: Query<&Transform, With<PlayerAircraft>>,
    mut rungs: Query<(&Rung, &mut Node, &mut UiTransform, &mut Visibility)>,
) {
    let view = (|| {
        if round.outcome().is_some() {
            return None;
        }
        let (camera, camera_tf) = camera.single().ok()?;
        let jet = player.single().ok()?;
        let forward = jet.forward().as_vec3();
        // Straight up or down the heading is undefined: no ladder.
        let heading = Vec3::new(forward.x, 0.0, forward.z).try_normalize()?;
        let pitch = forward.y.clamp(-1.0, 1.0).asin().to_degrees();
        Some((
            camera,
            GlobalTransform::from(*camera_tf),
            jet.translation,
            heading,
            pitch,
        ))
    })();
    for (rung, mut node, mut transform, mut visibility) in &mut rungs {
        let placed = view.and_then(|(camera, camera_gt, origin, heading, pitch)| {
            if (rung.pitch as f32 - pitch).abs() > LADDER_SHOWN {
                return None;
            }
            let (centre, angle) = rung_on_screen(
                camera,
                &camera_gt,
                origin,
                heading,
                (rung.pitch as f32).to_radians(),
            )?;
            let viewport = camera.logical_viewport_size()?;
            (centre.cmpge(Vec2::ZERO).all() && centre.cmple(viewport).all())
                .then_some((centre, angle, viewport.x))
        });
        match placed {
            Some((centre, angle, viewport_width)) => {
                let share = if rung.pitch == 0 {
                    HORIZON_WIDTH
                } else {
                    RUNG_WIDTH
                };
                let width = viewport_width * share;
                node.width = px(width);
                node.left = px(centre.x - width / 2.0);
                node.top = px(centre.y - LINE / 2.0);
                transform.rotation = Rot2::radians(angle);
                visibility.set_if_neq(Visibility::Visible);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// Screen centre and clockwise screen angle (rad) of the line at
/// `elevation` (rad) above the horizon, straight ahead along `heading`.
fn rung_on_screen(
    camera: &Camera,
    camera_gt: &GlobalTransform,
    origin: Vec3,
    heading: Vec3,
    elevation: f32,
) -> Option<(Vec2, f32)> {
    let direction = heading * elevation.cos() + Vec3::Y * elevation.sin();
    let centre = origin + direction * LADDER_RANGE;
    let right = heading.cross(Vec3::Y);
    let a = camera.world_to_viewport(camera_gt, centre).ok()?;
    let b = camera
        .world_to_viewport(camera_gt, centre + right * 50.0)
        .ok()?;
    let d = b - a;
    Some((a, d.y.atan2(d.x)))
}

/// Shows where to aim the gun: for the locked target, or else the enemy
/// closest to the nose, if the rounds can reach it. Put the reticle on the
/// pipper and the rounds meet the target.
#[allow(clippy::type_complexity)]
fn place_pipper(
    round: Res<Round>,
    camera: Query<(&Camera, &Transform), With<ChaseCamera>>,
    player: Query<
        (&Transform, Option<&AircraftVelocity>, &Gun, Option<&LockOn>),
        With<PlayerAircraft>,
    >,
    enemies: Query<(Entity, &Transform, Option<&AircraftVelocity>), With<Enemy>>,
    mission: Res<Mission>,
    mut pipper: Query<(&mut Node, &mut Visibility), With<Pipper>>,
) {
    let Ok((mut node, mut visibility)) = pipper.single_mut() else {
        return;
    };
    let solution = (|| {
        // Only with gun assist (easy levels, or chosen in Custom).
        if round.outcome().is_some() || !mission.difficulty.params().gun_assist {
            return None;
        }
        let (camera, camera_tf) = camera.single().ok()?;
        let (jet, jet_velocity, gun, lock) = player.single().ok()?;
        let shooter_velocity = jet_velocity.map_or(Vec3::ZERO, |v| v.0);
        let candidates = enemies.iter().map(|(entity, tf, velocity)| {
            (entity, tf.translation, velocity.map_or(Vec3::ZERO, |v| v.0))
        });
        let (_, position, velocity) = gun_target(
            jet.translation,
            jet.forward().as_vec3(),
            lock.and_then(LockOn::locked_target),
            PIPPER_CONE,
            candidates,
        )?;
        let to_target = position - jet.translation;
        let (direction, flight_time) =
            gun_lead(to_target, shooter_velocity, velocity, gun.spec.muzzle_speed)?;
        if flight_time > gun.spec.lifetime {
            return None;
        }
        // Same depth as the reticle, so both line up exactly when aimed.
        let aim = jet.translation + direction * RETICLE_RANGE;
        let p = camera
            .world_to_viewport(&GlobalTransform::from(*camera_tf), aim)
            .ok()?;
        Some(p)
    })();
    match solution {
        Some(p) => {
            node.left = px(p.x - PIPPER_SIZE / 2.0);
            node.top = px(p.y - PIPPER_SIZE / 2.0);
            visibility.set_if_neq(Visibility::Visible);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

fn update_result_screen(
    round: Res<Round>,
    mission: Res<Mission>,
    mut screen: Query<&mut Visibility, With<ResultScreen>>,
    mut title: Query<&mut Text, (With<ResultTitle>, Without<ResultBody>)>,
    mut body: Query<&mut Text, (With<ResultBody>, Without<ResultTitle>)>,
) {
    if !round.is_changed() {
        return;
    }
    let outcome = match round.phase {
        RoundPhase::Over(outcome) => Some(outcome),
        _ => None,
    };
    for mut visibility in &mut screen {
        visibility.set_if_neq(if outcome.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }
    let Some(outcome) = outcome else {
        return;
    };
    let stats = round.stats;
    let percent =
        |rate: Option<f32>| rate.map_or("-".to_string(), |r| format!("{:.0} %", r * 100.0));
    if let Ok(mut title) = title.single_mut() {
        title.0 = outcome_title(outcome);
    }
    if let Ok(mut body) = body.single_mut() {
        body.0 = format!(
            "Time  {}\nKills  {} / {}\nHit rate  {}\nGun  {} of {} rounds\nMissiles  {} of {}",
            format_time(round.elapsed),
            stats.kills,
            mission.enemy_count(),
            percent(stats.hit_rate()),
            stats.gun_hits,
            stats.gun_rounds,
            stats.missile_hits,
            stats.missiles_fired,
        );
    }
}
