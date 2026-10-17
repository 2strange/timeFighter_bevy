//! Checks the normalised aircraft files directly (no renderer, no asset
//! server): metres, +Y up, nose along −Z, origin near the box centre.

use std::path::{Path, PathBuf};

use bevy::prelude::Vec3;

use super::AircraftModel;

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `(name, length in m)` for every entry of `tools/aircraft.json`.
fn manifest() -> Vec<(String, f32)> {
    let text = std::fs::read_to_string(repo().join("tools/aircraft.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["aircraft"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap().to_owned(),
                entry["length"].as_f64().unwrap() as f32,
            )
        })
        .collect()
}

fn glb(name: &str) -> PathBuf {
    repo().join("assets/aircraft").join(format!("{name}.glb"))
}

struct Model {
    positions: Vec<[f32; 3]>,
    /// Vertices of propeller materials, where the model has separate ones.
    propellers: Vec<[f32; 3]>,
    min: [f32; 3],
    max: [f32; 3],
}

impl Model {
    fn load(name: &str) -> Model {
        let bytes = std::fs::read(glb(name)).unwrap();
        let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
        let blob = gltf.blob.as_deref();

        for node in gltf.nodes() {
            let (t, r, s) = node.transform().decomposed();
            assert_eq!(t, [0.0; 3], "{name}: node {:?} is translated", node.name());
            assert_eq!(
                r,
                [0.0, 0.0, 0.0, 1.0],
                "{name}: node {:?} is rotated",
                node.name()
            );
            assert_eq!(s, [1.0; 3], "{name}: node {:?} is scaled", node.name());
        }
        for image in gltf.images() {
            assert!(
                matches!(image.source(), gltf::image::Source::View { .. }),
                "{name}: texture is not embedded"
            );
        }

        let mut positions = Vec::new();
        let mut propellers = Vec::new();
        for mesh in gltf.meshes() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| match buffer.source() {
                    gltf::buffer::Source::Bin => blob,
                    gltf::buffer::Source::Uri(_) => None,
                });
                let points: Vec<_> = reader.read_positions().unwrap().collect();
                let material = primitive.material().name().unwrap_or("").to_lowercase();
                if material.starts_with("prop") {
                    propellers.extend(points.iter().copied());
                }
                positions.extend(points);
            }
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        Model {
            positions,
            propellers,
            min,
            max,
        }
    }

    fn extent(&self, axis: usize) -> f32 {
        self.max[axis] - self.min[axis]
    }

    fn centre(&self, axis: usize) -> f32 {
        (self.max[axis] + self.min[axis]) / 2.0
    }
}

#[test]
fn catalogue_matches_manifest() {
    let names: Vec<_> = manifest().into_iter().map(|(name, _)| name).collect();
    for model in AircraftModel::ALL {
        assert!(
            names.iter().any(|n| n == model.file_stem()),
            "{model:?} not in manifest"
        );
        assert!(glb(model.file_stem()).exists(), "{model:?} has no .glb");
    }
}

#[test]
fn models_are_normalised() {
    for (name, length) in manifest() {
        let model = Model::load(&name);
        let (x, y, z) = (model.extent(0), model.extent(1), model.extent(2));

        // Length along Z within 2 % of the real aircraft.
        assert!(
            (z - length).abs() <= 0.02 * length,
            "{name}: length {z} m, expected {length} m"
        );
        // Lying flat: height is the smallest dimension and below the span.
        assert!(y < x && y < z, "{name}: {x} x {y} x {z} is not lying flat");
        // Origin in the box centre in X and Z, inside the box in Y.
        assert!(
            model.centre(0).abs() < 0.01 * length,
            "{name}: off-centre in X"
        );
        assert!(
            model.centre(2).abs() < 0.01 * length,
            "{name}: off-centre in Z"
        );
        assert!(
            model.min[1] < 0.0 && model.max[1] > 0.0,
            "{name}: origin outside in Y"
        );
        // Tall fins lift the box centre; 30 % of the height still means the
        // origin is on the fuselage.
        assert!(
            model.centre(1).abs() < 0.3 * y,
            "{name}: origin far from centre in Y"
        );
    }
}

#[test]
fn noses_point_along_negative_z() {
    // Where the model has its propellers as their own material, they must
    // be at the front. Otherwise: the fin is the highest part and sits at the
    // tail, so the top slice of the model must lie behind the centre (+Z).
    for (name, _) in manifest() {
        let model = Model::load(&name);
        if !model.propellers.is_empty() {
            let mean_z =
                model.propellers.iter().map(|p| p[2]).sum::<f32>() / model.propellers.len() as f32;
            assert!(
                mean_z < -0.3 * model.extent(2) / 2.0,
                "{name}: propellers are not at the front (mean z {mean_z})"
            );
            continue;
        }
        if name == "zeppelin" {
            // An airship: blunt nose, pointed tail, fins near the tail.
            let width_within = |from_nose: bool, share: f32| {
                let cut = share * model.extent(2);
                model
                    .positions
                    .iter()
                    .filter(|p| {
                        if from_nose {
                            p[2] < model.min[2] + cut
                        } else {
                            p[2] > model.max[2] - cut
                        }
                    })
                    .map(|p| p[0].abs())
                    .fold(0.0, f32::max)
            };
            assert!(
                width_within(true, 0.05) > width_within(false, 0.05),
                "{name}: tail is blunter than the nose"
            );
            assert!(
                width_within(false, 0.15) > width_within(true, 0.15),
                "{name}: no fins near the tail"
            );
            continue;
        }
        let cut = model.max[1] - 0.15 * model.extent(1);
        let top: Vec<_> = model.positions.iter().filter(|p| p[1] >= cut).collect();
        let mean_z = top.iter().map(|p| p[2]).sum::<f32>() / top.len() as f32;
        assert!(
            mean_z > 0.2 * model.extent(2) / 2.0,
            "{name}: top of the model is not at the tail (mean z {mean_z})"
        );
    }
}

#[test]
fn sizes_match_the_files() {
    for model in AircraftModel::ALL {
        let file = Model::load(model.file_stem());
        let size = model.size();
        for i in 0..3 {
            assert!(
                (size.min[i] - file.min[i]).abs() < 0.02
                    && (size.max[i] - file.max[i]).abs() < 0.02,
                "{model:?} axis {i}: size {size:?}, file {:?}..{:?}",
                file.min,
                file.max
            );
        }
    }
}

#[test]
fn colliders_fill_but_do_not_exceed_the_models() {
    for model in AircraftModel::ALL {
        let size = model.size();
        let aabb = model.collider().raw.compute_local_aabb();
        let (min, max) = (
            [aabb.mins.x, aabb.mins.y, aabb.mins.z],
            [aabb.maxs.x, aabb.maxs.y, aabb.maxs.z],
        );
        for i in 0..3 {
            assert!(
                min[i] >= size.min[i] - 0.3,
                "{model:?} sticks out below axis {i}"
            );
            assert!(
                max[i] <= size.max[i] + 0.3,
                "{model:?} sticks out above axis {i}"
            );
        }
        // Nose to tail and wing tip to wing tip are covered.
        assert!(
            max[2] - min[2] >= 0.95 * size.length(),
            "{model:?} too short"
        );
        assert!(
            max[0] - min[0] >= 0.95 * size.span(),
            "{model:?} too narrow"
        );
        // The fin reaches the top.
        assert!(max[1] >= 0.9 * size.max.y, "{model:?} fin too low");
    }
}

#[test]
fn gun_mounts_match_the_normaliser_output() {
    let text = std::fs::read_to_string(repo().join("assets/aircraft/mounts.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    for model in AircraftModel::ALL {
        let stored = json.get(model.file_stem());
        // Gun positions and engines are both measured points.
        let mounts: Vec<_> = model
            .gun_mounts()
            .iter()
            .chain(model.engine_pods())
            .collect();
        assert_eq!(
            stored.map_or(0, |m| m.as_object().unwrap().len()),
            mounts.len(),
            "{model:?}"
        );
        for mount in mounts {
            let p = &stored.unwrap()[mount.name];
            let p = Vec3::new(
                p[0].as_f64().unwrap() as f32,
                p[1].as_f64().unwrap() as f32,
                p[2].as_f64().unwrap() as f32,
            );
            assert!(
                p.distance(mount.position) < 0.02,
                "{model:?} {}",
                mount.name
            );
        }
    }
}

#[test]
fn bomber_mounts_sit_on_the_aircraft_front_and_rear() {
    for model in [AircraftModel::B17, AircraftModel::Lancaster] {
        let size = model.size();
        let mounts = model.gun_mounts();
        assert!(mounts.len() >= 2);
        for mount in mounts {
            let p = mount.position;
            assert!(
                p.cmpge(size.min).all() && p.cmple(size.max).all(),
                "{model:?} {mount:?}"
            );
            assert!(
                p.x.abs() < 0.5,
                "{model:?} {} off the centre line",
                mount.name
            );
        }
        // Front gun near the nose, rear gun near the tail.
        assert!(mounts[0].position.z < size.min.z + 2.0, "{model:?} front");
        assert!(mounts[1].position.z > size.max.z - 2.0, "{model:?} rear");
    }
}

#[test]
fn zeppelin_collider_holds_its_guns_and_engines_but_not_the_air_around_the_nose() {
    let model = AircraftModel::Zeppelin;
    let collider = model.collider();
    let distance = |p: Vec3| collider.raw.distance_to_local_point(p, true);
    for point in model.gun_mounts().iter().chain(model.engine_pods()) {
        assert!(distance(point.position) < 0.5, "{point:?} outside");
    }
    assert_eq!(model.gun_mounts().len(), 5);
    // The gunners' facing comes from the name: exactly one direction word.
    for mount in model.gun_mounts() {
        let words = ["front", "rear", "tail", "top", "gondola"];
        let hits = words.iter().filter(|w| mount.name.contains(*w)).count();
        assert_eq!(hits, 1, "{}", mount.name);
    }
    assert_eq!(model.engine_pods().len(), 4);
    // Engines in pairs, mirrored left and right.
    for pair in model.engine_pods().chunks(2) {
        assert!((pair[0].position.x + pair[1].position.x).abs() < 0.01);
        assert!(pair[0].position.x < 0.0, "left first");
    }
    // The nose tapers: 7.5 m behind the tip the hull is about 10 m in
    // radius, so a point 14 m off the axis there is in the air.
    let size = model.size();
    assert!(distance(Vec3::new(14.0, 0.5, size.min.z + 7.5)) > 1.0);
    // The middle of the hull is solid up to its 21.8 m radius.
    assert_eq!(distance(Vec3::new(0.0, 21.0, -20.0)), 0.0);
}

#[test]
fn bombers_are_light_enough_to_fly_in_numbers() {
    for name in ["b17", "lancaster"] {
        let bytes = std::fs::read(glb(name)).unwrap();
        let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
        let triangles: usize = gltf
            .meshes()
            .flat_map(|m| m.primitives())
            .map(|p| {
                p.indices().map_or_else(
                    || p.get(&gltf::Semantic::Positions).unwrap().count(),
                    |i| i.count(),
                ) / 3
            })
            .sum();
        assert!(triangles <= 12_000, "{name}: {triangles} triangles");
    }
}
