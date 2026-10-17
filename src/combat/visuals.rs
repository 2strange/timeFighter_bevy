//! Meshes for tracers, missiles and explosions. Visual only: the
//! simulation in [`super::CombatPlugin`] never reads them.
//!
//! Explosions are a few low-poly spheres and cubes animated by scale and
//! position with shared unlit materials: no particles, no compute, cheap
//! on WebGL2.

use std::f32::consts::FRAC_PI_2;

use bevy::{light::NotShadowCaster, prelude::*};
use bevy_rapier3d::prelude::Velocity;

use super::{Bomb, Effect, EffectKind, Missile, Munition, Team, Weapon, weapons::heading};

pub struct CombatVisualsPlugin;

impl Plugin for CombatVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_visuals, load_effect_visuals))
            .add_systems(
                PostUpdate,
                (
                    dress_projectiles,
                    // Same frame as the player's wreck explosion.
                    dress_effects.after(super::wreck::WreckSystems),
                    dress_bombs,
                ),
            )
            .add_systems(Update, (point_missiles, animate_effects));
    }
}

#[derive(Resource)]
struct ProjectileVisuals {
    tracer: Handle<Mesh>,
    player_tracer: Handle<StandardMaterial>,
    enemy_tracer: Handle<StandardMaterial>,
    missile_body: Handle<Mesh>,
    missile_paint: Handle<StandardMaterial>,
    flame: Handle<Mesh>,
    flame_glow: Handle<StandardMaterial>,
}

fn load_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |materials: &mut Assets<StandardMaterial>, emissive: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive,
            unlit: true,
            ..default()
        })
    };
    commands.insert_resource(ProjectileVisuals {
        // Long and thin, like a tracer streak at 1 km/s.
        tracer: meshes.add(Cuboid::new(0.3, 0.3, 9.0)),
        player_tracer: glow(&mut materials, LinearRgba::rgb(6.0, 5.0, 1.5)),
        enemy_tracer: glow(&mut materials, LinearRgba::rgb(6.0, 1.6, 0.6)),
        // Built along +Y; turned to −Z when spawned. Real size (~3.7 m) would
        // vanish at distance, so a little thicker.
        missile_body: meshes.add(Capsule3d::new(0.3, 3.2)),
        missile_paint: materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.9, 0.88),
            perceptual_roughness: 0.5,
            ..default()
        }),
        flame: meshes.add(Cone::new(0.35, 2.4)),
        flame_glow: glow(&mut materials, LinearRgba::rgb(8.0, 3.5, 0.8)),
    });
}

fn dress_projectiles(
    mut commands: Commands,
    visuals: Option<Res<ProjectileVisuals>>,
    added: Query<(Entity, &Munition), Added<Munition>>,
) {
    let Some(visuals) = visuals else {
        return;
    };
    let along_nose = Quat::from_rotation_x(-FRAC_PI_2);
    for (entity, munition) in &added {
        let mut entity = commands.entity(entity);
        entity.insert(Visibility::default());
        match munition.weapon {
            Weapon::Gun => {
                let material = match munition.team {
                    Team::Player => visuals.player_tracer.clone(),
                    Team::Enemy => visuals.enemy_tracer.clone(),
                };
                entity.insert((
                    Mesh3d(visuals.tracer.clone()),
                    MeshMaterial3d(material),
                    NotShadowCaster,
                ));
            }
            Weapon::Missile => {
                // The physics body keeps its launch rotation; the model
                // turns with the flight path under a pivot.
                entity.with_children(|missile| {
                    missile
                        .spawn((MissileModel, Transform::default(), Visibility::default()))
                        .with_children(|model| {
                            model.spawn((
                                Mesh3d(visuals.missile_body.clone()),
                                MeshMaterial3d(visuals.missile_paint.clone()),
                                Transform::from_rotation(along_nose),
                            ));
                            // Exhaust flame behind the body, tip pointing back.
                            model.spawn((
                                Mesh3d(visuals.flame.clone()),
                                MeshMaterial3d(visuals.flame_glow.clone()),
                                Transform::from_xyz(0.0, 0.0, 3.1)
                                    .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                                NotShadowCaster,
                            ));
                        });
                });
            }
        }
    }
}

/// Pivot of a missile's meshes, turned along its velocity every frame.
#[derive(Component)]
struct MissileModel;

fn point_missiles(
    missiles: Query<(&Velocity, &Transform, &Children), With<Missile>>,
    mut models: Query<&mut Transform, (With<MissileModel>, Without<Missile>)>,
) {
    for (velocity, transform, children) in &missiles {
        let local = transform.rotation.inverse() * heading(velocity.linear);
        for child in children {
            if let Ok(mut model) = models.get_mut(*child) {
                model.rotation = local;
            }
        }
    }
}

#[derive(Resource)]
struct EffectVisuals {
    ball: Handle<Mesh>,
    cube: Handle<Mesh>,
    fire: Handle<StandardMaterial>,
    core: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
    debris: Handle<StandardMaterial>,
}

fn load_effect_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let unlit = |color: Color| StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    };
    commands.insert_resource(EffectVisuals {
        // Unit diameter, low poly.
        ball: meshes.add(Sphere::new(0.5).mesh().ico(1).expect("small subdivision")),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        fire: materials.add(unlit(Color::srgb(1.0, 0.5, 0.12))),
        core: materials.add(unlit(Color::srgb(1.0, 0.92, 0.6))),
        smoke: materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            ..unlit(Color::srgba(0.12, 0.11, 0.1, 0.65))
        }),
        debris: materials.add(unlit(Color::srgb(0.08, 0.08, 0.08))),
    });
}

/// Parts of an explosion, animated from the parent effect's progress.
#[derive(Component, Clone, Copy)]
enum EffectPart {
    Fire,
    Core,
    Smoke,
    /// Flies off with this velocity (m/s), falling.
    Debris(Vec3),
}

fn dress_effects(
    mut commands: Commands,
    visuals: Option<Res<EffectVisuals>>,
    added: Query<(Entity, &Effect), Added<Effect>>,
) {
    let Some(visuals) = visuals else {
        return;
    };
    for (entity, effect) in &added {
        let mut parts = vec![
            (EffectPart::Fire, visuals.fire.clone(), visuals.ball.clone()),
            (EffectPart::Core, visuals.core.clone(), visuals.ball.clone()),
        ];
        if effect.kind != EffectKind::Spark {
            parts.push((
                EffectPart::Smoke,
                visuals.smoke.clone(),
                visuals.ball.clone(),
            ));
            // Debris in a fixed star pattern: looks random enough, costs no RNG.
            let speed = effect.kind.size() * 2.5;
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let up = 0.4 + 0.5 * ((i * 3) % 4) as f32 / 3.0;
                let dir = Vec3::new(angle.cos(), up, angle.sin()).normalize();
                parts.push((
                    EffectPart::Debris(dir * speed),
                    visuals.debris.clone(),
                    visuals.cube.clone(),
                ));
            }
        }
        commands
            .entity(entity)
            .insert(Visibility::default())
            .with_children(|effect| {
                for (part, material, mesh) in parts {
                    effect.spawn((
                        part,
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_scale(Vec3::ZERO),
                        NotShadowCaster,
                    ));
                }
            });
    }
}

fn animate_effects(
    time: Res<Time>,
    effects: Query<(&Effect, &Children)>,
    mut parts: Query<(&mut EffectPart, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (effect, children) in &effects {
        let size = effect.kind.size();
        let t = effect.progress();
        // Fireball: swells fast, then collapses by 60 % of the lifetime.
        let fire = if t < 0.12 {
            t / 0.12
        } else {
            (1.0 - (t - 0.12) / 0.48).max(0.0)
        };
        for child in children {
            let Ok((mut part, mut transform)) = parts.get_mut(*child) else {
                continue;
            };
            match &mut *part {
                EffectPart::Fire => transform.scale = Vec3::splat(size * fire),
                EffectPart::Core => transform.scale = Vec3::splat(size * 0.6 * fire),
                EffectPart::Smoke => {
                    // Grows, drifts up and thins out at the end.
                    let grow = (t / 0.5).min(1.0);
                    let fade = ((1.0 - t) / 0.3).min(1.0);
                    transform.scale = Vec3::splat(size * 1.5 * grow * fade);
                    transform.translation.y = size * 0.6 * t;
                }
                EffectPart::Debris(velocity) => {
                    velocity.y -= 9.81 * dt;
                    transform.translation += *velocity * dt;
                    transform.rotate_x(6.0 * dt);
                    transform.scale = Vec3::splat(size * 0.06 * (1.0 - t));
                }
            }
        }
    }
}

/// Bombs: a dark, slim body, built along +Y and turned to −Z.
fn dress_bombs(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    added: Query<Entity, Added<Bomb>>,
) {
    if added.is_empty() {
        return;
    }
    let (mesh, material) = shared
        .get_or_insert_with(|| {
            (
                meshes.add(Capsule3d::new(0.3, 1.6)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.15, 0.16, 0.14),
                    perceptual_roughness: 0.7,
                    ..default()
                }),
            )
        })
        .clone();
    for entity in &added {
        commands
            .entity(entity)
            .insert(Visibility::default())
            .with_child((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                NotShadowCaster,
            ));
    }
}
