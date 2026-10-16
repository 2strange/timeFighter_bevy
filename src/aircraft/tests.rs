//! Checks the normalised aircraft files directly (no renderer, no asset
//! server): metres, +Y up, nose along −Z, origin near the box centre.

use std::path::{Path, PathBuf};

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
        for mesh in gltf.meshes() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| match buffer.source() {
                    gltf::buffer::Source::Bin => blob,
                    gltf::buffer::Source::Uri(_) => None,
                });
                positions.extend(reader.read_positions().unwrap());
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
        assert!(
            model.centre(1).abs() < 0.25 * y,
            "{name}: origin far from centre in Y"
        );
    }
}

#[test]
fn noses_point_along_negative_z() {
    // The fin is the highest part and sits at the tail, so the top slice of
    // the model must lie behind the centre (+Z).
    for (name, _) in manifest() {
        let model = Model::load(&name);
        let cut = model.max[1] - 0.15 * model.extent(1);
        let top: Vec<_> = model.positions.iter().filter(|p| p[1] >= cut).collect();
        let mean_z = top.iter().map(|p| p[2]).sum::<f32>() / top.len() as f32;
        assert!(
            mean_z > 0.2 * model.extent(2) / 2.0,
            "{name}: top of the model is not at the tail (mean z {mean_z})"
        );
    }
}
