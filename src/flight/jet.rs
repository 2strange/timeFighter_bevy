//! Placeholder low-poly jet built from primitives, in real dimensions
//! (about 19 m long, 13 m span). Origin near the centre of mass, nose along −Z.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::physics::{Layer, aircraft_body};

use super::{FlightConfig, FlightInput, FlightState, PlayerAircraft};

pub(super) fn spawn_player_jet(
    mut commands: Commands,
    cfg: Res<FlightConfig>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let jet = commands
        .spawn((
            Name::new("Player jet"),
            PlayerAircraft,
            FlightInput::default(),
            FlightState::new(&cfg),
            cfg.spawn_transform(),
            Visibility::default(),
            aircraft_body(Layer::Player),
        ))
        .id();
    if let Some(model) = cfg.player_model {
        commands.entity(jet).insert(model.scene(&assets));
        return;
    }

    let body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.58, 0.62),
        perceptual_roughness: 0.6,
        ..default()
    });
    let canopy = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.1, 0.14),
        perceptual_roughness: 0.15,
        ..default()
    });
    let exhaust = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.5, 0.15),
        emissive: LinearRgba::rgb(4.0, 1.6, 0.4),
        ..default()
    });

    // Long primitives are built along +Y; this turns +Y into the nose direction −Z.
    let along_nose = Quat::from_rotation_x(-FRAC_PI_2);

    let parts = [
        // Fuselage: 1.2 m radius, spans z = −7.7 … +8.7.
        (
            meshes.add(Capsule3d::new(1.2, 14.0)),
            body.clone(),
            Transform::from_xyz(0.0, 0.0, 0.5).with_rotation(along_nose),
        ),
        // Nose cone: base at z = −7.2, tip at z = −10.2.
        (
            meshes.add(Cone::new(1.15, 3.0)),
            body.clone(),
            Transform::from_xyz(0.0, 0.0, -8.7).with_rotation(along_nose),
        ),
        // Canopy.
        (
            meshes.add(Capsule3d::new(0.65, 2.6)),
            canopy,
            Transform::from_xyz(0.0, 1.05, -4.6).with_rotation(along_nose),
        ),
        // Main wing: 13 m span.
        (
            meshes.add(Cuboid::new(13.0, 0.3, 4.5)),
            body.clone(),
            Transform::from_xyz(0.0, -0.2, 1.8),
        ),
        // Horizontal stabiliser.
        (
            meshes.add(Cuboid::new(6.5, 0.2, 2.4)),
            body.clone(),
            Transform::from_xyz(0.0, 0.0, 7.6),
        ),
        // Twin vertical fins.
        (
            meshes.add(Cuboid::new(0.25, 3.2, 2.8)),
            body.clone(),
            Transform::from_xyz(-1.3, 2.2, 6.8),
        ),
        (
            meshes.add(Cuboid::new(0.25, 3.2, 2.8)),
            body,
            Transform::from_xyz(1.3, 2.2, 6.8),
        ),
        // Engine exhaust glow.
        (
            meshes.add(Cylinder::new(0.9, 0.3)),
            exhaust,
            Transform::from_xyz(0.0, 0.0, 9.0).with_rotation(along_nose),
        ),
    ];

    commands.entity(jet).with_children(|jet| {
        for (mesh, material, transform) in parts {
            jet.spawn((Mesh3d(mesh), MeshMaterial3d(material), transform));
        }
    });
}
