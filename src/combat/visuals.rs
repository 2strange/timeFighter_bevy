//! Meshes for tracers and missiles. Visual only: the simulation in
//! [`super::CombatPlugin`] never reads them.

use std::f32::consts::FRAC_PI_2;

use bevy::{light::NotShadowCaster, prelude::*};

use super::{Projectile, Team, Weapon};

pub struct CombatVisualsPlugin;

impl Plugin for CombatVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_visuals)
            .add_systems(PostUpdate, dress_projectiles);
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
    added: Query<(Entity, &Projectile), Added<Projectile>>,
) {
    let Some(visuals) = visuals else {
        return;
    };
    let along_nose = Quat::from_rotation_x(-FRAC_PI_2);
    for (entity, projectile) in &added {
        let mut entity = commands.entity(entity);
        entity.insert(Visibility::default());
        match projectile.weapon {
            Weapon::Gun => {
                let material = match projectile.team {
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
                entity.with_children(|missile| {
                    missile.spawn((
                        Mesh3d(visuals.missile_body.clone()),
                        MeshMaterial3d(visuals.missile_paint.clone()),
                        Transform::from_rotation(along_nose),
                    ));
                    // Exhaust flame behind the body, tip pointing back.
                    missile.spawn((
                        Mesh3d(visuals.flame.clone()),
                        MeshMaterial3d(visuals.flame_glow.clone()),
                        Transform::from_xyz(0.0, 0.0, 3.1)
                            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                        NotShadowCaster,
                    ));
                });
            }
        }
    }
}
