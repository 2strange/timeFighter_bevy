//! The player's jet: the configured glTF model, or a low-poly jet built from
//! primitives in real dimensions (about 19 m long, 13 m span) when no model
//! is configured or the model fails to load. Origin near the centre of mass,
//! nose along −Z.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::physics::{AircraftSize, Layer, aircraft_body_sized};

use super::{FlightConfig, FlightInput, FlightState, PlayerAircraft};

pub(super) fn spawn_player_jet(
    mut commands: Commands,
    cfg: Res<FlightConfig>,
    assets: Res<AssetServer>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
) {
    let jet = commands
        .spawn((
            Name::new("Player jet"),
            PlayerAircraft,
            FlightInput::default(),
            FlightState::new(&cfg),
            cfg.spawn_transform(),
            Visibility::default(),
            aircraft_body_sized(
                Layer::Player,
                cfg.player_model
                    .map_or(AircraftSize::JET, |model| model.size()),
            ),
        ))
        .id();
    match cfg.player_model {
        Some(model) => {
            commands.entity(jet).insert(model.scene(&assets));
        }
        None => add_primitive_jet(&mut commands, jet, meshes, materials),
    }
}

/// Swaps a player model that failed to load for the primitive jet.
pub(super) fn fall_back_to_primitive_jet(
    mut commands: Commands,
    assets: Res<AssetServer>,
    player: Query<(Entity, &WorldAssetRoot), With<PlayerAircraft>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((jet, scene)) = player.single() else {
        return;
    };
    if assets.load_state(&scene.0).is_failed() {
        warn!("player model failed to load, using the built-in jet");
        commands.entity(jet).remove::<WorldAssetRoot>();
        add_primitive_jet(&mut commands, jet, meshes, materials);
    }
}

fn add_primitive_jet(
    commands: &mut Commands,
    jet: Entity,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{GamePlugin, test_support::headless_app};

    fn player(app: &mut App) -> Entity {
        app.world_mut()
            .query_filtered::<Entity, With<PlayerAircraft>>()
            .single(app.world())
            .unwrap()
    }

    /// Meshes of the jet model below `root` (engine flames not counted).
    fn meshes_below(world: &World, root: Entity) -> usize {
        let mut count = 0;
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            let flame = world
                .get::<super::super::jet_fx::EngineFlame>(entity)
                .is_some();
            count += usize::from(world.get::<Mesh3d>(entity).is_some() && !flame);
            if let Some(children) = world.get::<Children>(entity) {
                stack.extend(children.iter());
            }
        }
        count
    }

    /// Updates until `done` holds, at most 20 s (assets load on threads).
    fn update_until(app: &mut App, mut done: impl FnMut(&mut App) -> bool) {
        let start = Instant::now();
        while !done(app) {
            assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            app.update();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn player_flies_the_f14_by_default() {
        let mut app = headless_app(GamePlugin);
        app.update();
        let jet = player(&mut app);
        assert!(app.world().get::<WorldAssetRoot>(jet).is_some());
        update_until(&mut app, |app| meshes_below(app.world(), jet) > 0);
        // The glTF scene, not the primitive jet (which has no scene root).
        assert!(app.world().get::<WorldAssetRoot>(jet).is_some());
    }

    #[test]
    fn primitive_jet_without_a_model() {
        let mut app = headless_app(GamePlugin);
        app.insert_resource(FlightConfig {
            player_model: None,
            ..default()
        });
        app.update();
        let jet = player(&mut app);
        assert!(app.world().get::<WorldAssetRoot>(jet).is_none());
        assert_eq!(meshes_below(app.world(), jet), 8);
    }

    #[test]
    fn primitive_jet_replaces_a_model_that_fails_to_load() {
        // A jet whose scene points at a missing file and has no meshes.
        let mut app = headless_app(GamePlugin);
        app.insert_resource(FlightConfig {
            player_model: None,
            ..default()
        });
        app.update();
        let jet = player(&mut app);
        let missing = app
            .world()
            .resource::<AssetServer>()
            .load(GltfAssetLabel::Scene(0).from_asset("aircraft/missing.glb"));
        app.world_mut()
            .entity_mut(jet)
            .despawn_related::<Children>()
            .insert(WorldAssetRoot(missing));

        // Loading fails on a background thread; how many frames that takes
        // depends on the machine's load, so wait on the outcome, not frames.
        update_until(&mut app, |app| {
            app.world().get::<WorldAssetRoot>(jet).is_none() && meshes_below(app.world(), jet) > 0
        });
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(meshes_below(app.world(), jet), 8, "one primitive jet, once");
    }
}
