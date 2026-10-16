//! Head-up display: flight data, weapons, enemies left, target markers
//! with lock state, gun reticle, hit feedback and the result screen.
//!
//! Kept calm: one colour for normal information, amber while the seeker
//! tracks, red for a lock or damage. Everything else stays out of view.

mod layout;
#[cfg(test)]
mod tests;

use bevy::{platform::collections::HashSet, prelude::*};

pub use layout::{edge_point, format_time, gun_target};

use crate::{
    ai::Enemy,
    camera::{CameraSystems, ChaseCamera},
    combat::{
        AircraftVelocity, CombatConfig, Destroyed, Gun, Health, Hit, LockOn, LockState,
        MissileRack, Team, gun_lead, in_seeker_cone,
    },
    flight::{FlightState, PlayerAircraft},
    game_state::{Mission, Outcome, Round, RoundPhase, RoundSystems},
};

const HUD: Color = Color::srgba(0.72, 1.0, 0.78, 0.92);
const HUD_DIM: Color = Color::srgba(0.72, 1.0, 0.78, 0.45);
const TRACKING: Color = Color::srgb(1.0, 0.75, 0.2);
const ALERT: Color = Color::srgb(1.0, 0.25, 0.2);

/// Distance of the gun reticle's aim point ahead of the jet (m), about
/// where the cannon's rounds still fly straight and fast.
const RETICLE_RANGE: f32 = 800.0;
/// Inset of off-screen target indicators from the screen edge (px).
const EDGE_MARGIN: f32 = 28.0;
const MARKER_SIZE: f32 = 30.0;
const RETICLE_SIZE: f32 = 26.0;
const PIPPER_SIZE: f32 = 12.0;
/// The lead pipper considers targets up to this far off the nose (rad).
const PIPPER_CONE: f32 = 30.0 * std::f32::consts::PI / 180.0;
/// Within this distance (m) the pipper fills in: close enough to hit well.
const PIPPER_GOOD_RANGE: f32 = 900.0;

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

#[derive(Component)]
struct Reticle;

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

fn spawn_hud(mut commands: Commands) {
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
                panel.spawn((HudText::Gun, text("", 20.0, HUD)));
                panel.spawn((HudText::Missiles, text("", 20.0, HUD)));
            });

            // Bottom right: hull.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(16),
                bottom: px(16),
                ..default()
            })
            .with_child((HudText::Hull, text("", 20.0, HUD)));

            // Gun reticle with lock state and kill call below it.
            hud.spawn((
                Reticle,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(RETICLE_SIZE),
                    height: px(RETICLE_SIZE),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(HUD),
                Visibility::Hidden,
            ))
            .with_children(|reticle| {
                reticle.spawn((
                    Node {
                        width: px(2),
                        height: px(2),
                        ..default()
                    },
                    BackgroundColor(HUD),
                ));
                reticle
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        top: px(RETICLE_SIZE + 6.0),
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
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(TRACKING),
                BackgroundColor(Color::NONE),
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
                screen.spawn((ResultTitle, text("", 52.0, Color::WHITE)));
                screen.spawn((
                    ResultBody,
                    text("", 22.0, Color::srgb(0.9, 0.9, 0.9)),
                    TextLayout::justify(Justify::Center),
                ));
                screen.spawn(text(
                    "Enter or R: fly again",
                    18.0,
                    Color::srgb(0.75, 0.75, 0.75),
                ));
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
    feedback: Res<Feedback>,
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
            HudText::Status => (
                format!(
                    "{}    ENEMIES {}    {}",
                    mission.mode.display_name(),
                    round.enemies_left(&mission),
                    format_time(round.elapsed)
                ),
                HUD,
            ),
            HudText::Speed => (format!("{:>4.0} km/h", state.speed * 3.6), HUD),
            HudText::Altitude => (format!("{:.0} m", transform.translation.y), HUD),
            HudText::Gun => match gun.and_then(|g| g.ammo) {
                Some(ammo) => (
                    format!("GUN {ammo:>4}"),
                    if ammo == 0 { ALERT } else { HUD },
                ),
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
                    border: UiRect::all(px(2)),
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
    mut reticle: Query<
        (&mut Node, &mut Visibility, &mut BorderColor),
        (With<Reticle>, Without<TargetMarker>),
    >,
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

    if let Ok((mut node, mut visibility, mut border)) = reticle.single_mut() {
        let aim = jet.translation + jet.forward() * RETICLE_RANGE;
        match camera.world_to_viewport(&camera_gt, aim) {
            Ok(p) if playing => {
                node.left = px(p.x - RETICLE_SIZE / 2.0);
                node.top = px(p.y - RETICLE_SIZE / 2.0);
                visibility.set_if_neq(Visibility::Visible);
                let tint = if feedback.hit > 0.0 {
                    Color::WHITE
                } else {
                    HUD
                };
                border.set_if_neq(BorderColor::all(tint));
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
    mut pipper: Query<(&mut Node, &mut Visibility, &mut BackgroundColor), With<Pipper>>,
) {
    let Ok((mut node, mut visibility, mut fill)) = pipper.single_mut() else {
        return;
    };
    let solution = (|| {
        if round.outcome().is_some() {
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
        Some((p, to_target.length()))
    })();
    match solution {
        Some((p, distance)) => {
            node.left = px(p.x - PIPPER_SIZE / 2.0);
            node.top = px(p.y - PIPPER_SIZE / 2.0);
            visibility.set_if_neq(Visibility::Visible);
            let color = if distance <= PIPPER_GOOD_RANGE {
                TRACKING
            } else {
                Color::NONE
            };
            fill.set_if_neq(BackgroundColor(color));
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
        title.0 = match outcome {
            Outcome::Victory => "MISSION COMPLETE",
            Outcome::ShotDown => "SHOT DOWN",
            Outcome::Crashed => "CRASHED",
        }
        .into();
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
