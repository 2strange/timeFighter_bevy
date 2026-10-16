//! Placeholder enemy meshes in real dimensions until the normalised glTF
//! models arrive. Origin near the centre of mass, nose along −Z.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use super::{Enemy, EnemyKind};

pub struct EnemyVisualsPlugin;

impl Plugin for EnemyVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_parts)
            .add_systems(PostUpdate, dress_enemies);
    }
}

/// Shared meshes and materials of the placeholder Spitfire.
#[derive(Resource)]
struct SpitfireParts(Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)>);

fn load_parts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camo = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.38, 0.27),
        perceptual_roughness: 0.8,
        ..default()
    });
    let canopy = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.12, 0.16),
        perceptual_roughness: 0.2,
        ..default()
    });
    let spinner = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.2),
        perceptual_roughness: 0.6,
        ..default()
    });
    // Long primitives are built along +Y; this turns +Y into the nose direction −Z.
    let along_nose = Quat::from_rotation_x(-FRAC_PI_2);
    commands.insert_resource(SpitfireParts(vec![
        // Fuselage 0.8 m radius, z = −4.1 … +4.1 (with spinner 9.1 m total).
        (
            meshes.add(Capsule3d::new(0.8, 6.6)),
            camo.clone(),
            Transform::from_xyz(0.0, 0.0, 0.0).with_rotation(along_nose),
        ),
        // Spinner, z = −4.1 … −4.6.
        (
            meshes.add(Cone::new(0.45, 0.9)),
            spinner,
            Transform::from_xyz(0.0, 0.0, -4.4).with_rotation(along_nose),
        ),
        // Wing: 11.2 m span, slightly ahead of the centre.
        (
            meshes.add(Cuboid::new(11.2, 0.25, 2.2)),
            camo.clone(),
            Transform::from_xyz(0.0, -0.35, -0.9),
        ),
        // Tailplane.
        (
            meshes.add(Cuboid::new(3.3, 0.15, 1.0)),
            camo.clone(),
            Transform::from_xyz(0.0, 0.1, 3.7),
        ),
        // Fin.
        (
            meshes.add(Cuboid::new(0.15, 1.6, 1.2)),
            camo,
            Transform::from_xyz(0.0, 1.0, 3.6),
        ),
        // Canopy.
        (
            meshes.add(Capsule3d::new(0.45, 1.2)),
            canopy,
            Transform::from_xyz(0.0, 0.75, -0.4).with_rotation(along_nose),
        ),
    ]));
}

fn dress_enemies(
    mut commands: Commands,
    parts: Option<Res<SpitfireParts>>,
    added: Query<(Entity, &Enemy), Added<Enemy>>,
) {
    let Some(parts) = parts else {
        return;
    };
    for (entity, enemy) in &added {
        let parts = match enemy.kind {
            EnemyKind::Spitfire => &parts.0,
        };
        commands
            .entity(entity)
            .insert(Visibility::default())
            .with_children(|aircraft| {
                for (mesh, material, transform) in parts {
                    aircraft.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        *transform,
                    ));
                }
            });
    }
}
