//! Turns a [`CityLayout`] into render meshes: one flat ground mesh with the
//! streets painted in as vertex colours, and buildings merged per chunk and
//! facade material. Pure functions, so geometry is tested headless.

use std::collections::BTreeMap;

use bevy::{
    asset::RenderAssetUsages,
    math::Rect,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::layout::{
    BAY, CityLayout, Facade, OFFICE_FLOOR, Part, RESIDENTIAL_FLOOR, Roof, Slot, StreetClass, Zone,
    axis_rect,
};

// Ground colours (sRGB).
const ASPHALT: [f32; 3] = [0.30, 0.31, 0.33];
const SIDEWALK: [f32; 3] = [0.58, 0.57, 0.55];
/// Low contrast on purpose: thin bright lines shimmer at a distance.
const CENTER_LINE: [f32; 3] = [0.62, 0.60, 0.52];
const MEDIAN: [f32; 3] = [0.33, 0.45, 0.27];
const PLAZA: [f32; 3] = [0.52, 0.51, 0.49];
const COURTYARD: [f32; 3] = [0.47, 0.48, 0.43];
const LAWN: [f32; 3] = [0.40, 0.52, 0.30];
const PARK: [f32; 3] = [0.30, 0.47, 0.24];
const MEADOW: [f32; 3] = [0.42, 0.54, 0.30];
const YARD: [f32; 3] = [0.55, 0.49, 0.40];
const PATCHES: [[f32; 3]; 4] = [
    [0.40, 0.52, 0.29],
    [0.45, 0.55, 0.31],
    [0.37, 0.48, 0.27],
    [0.49, 0.53, 0.33],
];

/// Centre line or median width per street class.
fn median_width(class: StreetClass) -> f32 {
    match class {
        StreetClass::Street => 0.6,
        StreetClass::Avenue => 4.0,
        StreetClass::Boulevard => 8.0,
    }
}

/// One coloured rectangle of the ground (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundQuad {
    pub rect: Rect,
    pub color: [f32; 3],
}

/// What a ground cell shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    Plain([f32; 3]),
    /// Cross-section of line `i` on `axis`: sidewalks, lanes, centre line.
    Street {
        axis: usize,
        i: usize,
    },
}

/// Partitions the whole ground into non-overlapping coloured rectangles.
///
/// Every street, block, field and patch edge becomes a breakpoint on its
/// axis; each cell of the resulting grid is classified at its centre.
/// Equal plain cells are merged along x, street cells are split into their
/// cross-section strips. All quads lie in the plane y = 0, never on top of
/// each other, so nothing can z-fight.
pub fn ground_quads(layout: &CityLayout) -> Vec<GroundQuad> {
    let cfg = &layout.config;
    let mut breaks: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
    for (axis, out) in breaks.iter_mut().enumerate() {
        out.extend([-cfg.ground_half_extent, cfg.ground_half_extent]);
        for line in &layout.lines[axis] {
            out.extend([line.min(), line.max()]);
        }
        let mut edge = cfg.inner_half_extent;
        while edge < cfg.ground_half_extent {
            out.extend([-edge, edge]);
            edge += cfg.patch_size;
        }
        for farm in &layout.farms {
            for r in std::iter::once(farm.yard).chain(farm.strips.iter().map(|s| s.0)) {
                let (lo, hi) = if axis == 0 {
                    (r.min.x, r.max.x)
                } else {
                    (r.min.y, r.max.y)
                };
                out.extend([lo, hi]);
            }
        }
        out.sort_by(f32::total_cmp);
        out.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    }

    let mut quads = Vec::new();
    for zw in breaks[1].windows(2) {
        let mut run: Option<(f32, f32, [f32; 3])> = None;
        for xw in breaks[0].windows(2) {
            let cell = classify(
                layout,
                Vec2::new((xw[0] + xw[1]) / 2.0, (zw[0] + zw[1]) / 2.0),
            );
            match cell {
                Cell::Plain(color) => match &mut run {
                    Some((_, x1, c)) if *c == color => *x1 = xw[1],
                    _ => {
                        if let Some((x0, x1, c)) = run.take() {
                            quads.push(GroundQuad {
                                rect: Rect::new(x0, zw[0], x1, zw[1]),
                                color: c,
                            });
                        }
                        run = Some((xw[0], xw[1], color));
                    }
                },
                Cell::Street { axis, i } => {
                    if let Some((x0, x1, c)) = run.take() {
                        quads.push(GroundQuad {
                            rect: Rect::new(x0, zw[0], x1, zw[1]),
                            color: c,
                        });
                    }
                    let cell = Rect::new(xw[0], zw[0], xw[1], zw[1]);
                    street_strips(layout, axis, i, cell, &mut quads);
                }
            }
        }
        if let Some((x0, x1, c)) = run {
            quads.push(GroundQuad {
                rect: Rect::new(x0, zw[0], x1, zw[1]),
                color: c,
            });
        }
    }
    quads
}

fn classify(layout: &CityLayout, p: Vec2) -> Cell {
    for farm in &layout.farms {
        if farm.yard.contains(p) {
            return Cell::Plain(YARD);
        }
        if let Some((_, color)) = farm.strips.iter().find(|(r, _)| r.contains(p)) {
            return Cell::Plain(*color);
        }
    }
    match (layout.slot(0, p.x), layout.slot(1, p.y)) {
        (Slot::Line(i), Slot::Line(k)) => {
            let along_z = layout.crossing_active(0, i, k);
            let along_x = layout.crossing_active(1, k, i);
            match (along_z, along_x) {
                (true, true) => Cell::Plain(ASPHALT),
                (true, false) => Cell::Street { axis: 0, i },
                (false, true) => Cell::Street { axis: 1, i: k },
                (false, false) => Cell::Plain(countryside(layout, p)),
            }
        }
        (Slot::Line(i), Slot::Gap(g)) if layout.segment_active(0, i, g) => {
            Cell::Street { axis: 0, i }
        }
        (Slot::Gap(g), Slot::Line(k)) if layout.segment_active(1, k, g) => {
            Cell::Street { axis: 1, i: k }
        }
        (Slot::Gap(gx), Slot::Gap(gz)) => {
            let zone = layout
                .block(gx as isize - 1, gz as isize - 1)
                .map_or(Zone::Rural, |b| b.zone);
            Cell::Plain(match zone {
                Zone::Downtown => PLAZA,
                Zone::Midtown => COURTYARD,
                Zone::Suburb => LAWN,
                Zone::Park => PARK,
                Zone::Rural => countryside(layout, p),
            })
        }
        _ => Cell::Plain(countryside(layout, p)),
    }
}

/// Meadow near the city, a patchwork of fields further out for a sense of
/// speed when flying low over open land.
fn countryside(layout: &CityLayout, p: Vec2) -> [f32; 3] {
    let cfg = &layout.config;
    if p.x.abs() < cfg.inner_half_extent && p.y.abs() < cfg.inner_half_extent {
        return MEADOW;
    }
    let tile = (p / cfg.patch_size).floor();
    let mut rng = super::layout::Rng::stream(cfg.seed, 6, tile.x as i64, tile.y as i64);
    rng.pick(&PATCHES)
}

/// The part of line `i`'s cross-section that falls inside `cell`.
fn street_strips(
    layout: &CityLayout,
    axis: usize,
    i: usize,
    cell: Rect,
    out: &mut Vec<GroundQuad>,
) {
    let line = layout.lines[axis][i];
    let sidewalk = layout.config.sidewalk_width;
    let median = median_width(line.class);
    let (lo, hi) = (line.min(), line.max());
    let c = line.center;
    let strips = [
        (lo, lo + sidewalk, SIDEWALK),
        (lo + sidewalk, c - median / 2.0, ASPHALT),
        (
            c - median / 2.0,
            c + median / 2.0,
            if line.class == StreetClass::Street {
                CENTER_LINE
            } else {
                MEDIAN
            },
        ),
        (c + median / 2.0, hi - sidewalk, ASPHALT),
        (hi - sidewalk, hi, SIDEWALK),
    ];
    let (cell_lo, cell_hi, along) = if axis == 0 {
        (cell.min.x, cell.max.x, (cell.min.y, cell.max.y))
    } else {
        (cell.min.y, cell.max.y, (cell.min.x, cell.max.x))
    };
    for (a, b, color) in strips {
        let (a, b) = (a.max(cell_lo), b.min(cell_hi));
        if b > a {
            out.push(GroundQuad {
                rect: axis_rect(axis, (a, b), along),
                color,
            });
        }
    }
}

/// Vertex buffers for one merged, flat-shaded mesh.
#[derive(Default, Debug, Clone)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Adds a convex planar polygon given counter-clockwise as seen from its
    /// front; the normal follows from that winding.
    pub fn polygon(&mut self, verts: &[Vec3], uvs: &[Vec2], srgb: [f32; 3]) {
        let normal = (verts[1] - verts[0]).cross(verts[2] - verts[0]).normalize();
        let color = Color::srgb(srgb[0], srgb[1], srgb[2])
            .to_linear()
            .to_f32_array();
        let base = self.positions.len() as u32;
        for (v, uv) in verts.iter().zip(uvs) {
            self.positions.push(v.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
            self.colors.push(color);
        }
        for k in 1..verts.len() as u32 - 1 {
            self.indices.extend([base, base + k, base + k + 1]);
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn into_mesh(self) -> Mesh {
        // Render-world only: the CPU copy is dropped after upload; the bounds
        // for frustum culling are computed when the positions are inserted.
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

pub fn ground_mesh_data(quads: &[GroundQuad]) -> MeshData {
    let mut data = MeshData::default();
    for q in quads {
        let (a, b) = (q.rect.min, q.rect.max);
        // Counter-clockwise seen from above: the normal points up.
        let verts = [
            Vec3::new(a.x, 0.0, a.y),
            Vec3::new(a.x, 0.0, b.y),
            Vec3::new(b.x, 0.0, b.y),
            Vec3::new(b.x, 0.0, a.y),
        ];
        data.polygon(&verts, &[Vec2::ZERO; 4], q.color);
    }
    data
}

/// Which shared material a building surface uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Surface {
    Office,
    Residential,
    /// Untextured: roofs and windowless walls.
    Plain,
}

/// Size of one facade texture repetition (m): four bays by four storeys.
pub fn facade_tile(surface: Surface) -> Vec2 {
    match surface {
        Surface::Office => Vec2::new(4.0 * BAY, 4.0 * OFFICE_FLOOR),
        _ => Vec2::new(4.0 * BAY, 4.0 * RESIDENTIAL_FLOOR),
    }
}

/// Building meshes grouped by chunk `(cx, cz)` and surface.
pub type ChunkMeshes = BTreeMap<((i32, i32), Surface), MeshData>;

pub fn building_meshes(layout: &CityLayout) -> ChunkMeshes {
    let mut chunks: BTreeMap<(i32, i32), [MeshData; 3]> = BTreeMap::new();
    for building in &layout.buildings {
        let (lo, hi) = building.bounds();
        let chunk = ((lo.xz() + hi.xz()) / 2.0 / layout.config.chunk_size).floor();
        let meshes = chunks.entry((chunk.x as i32, chunk.y as i32)).or_default();
        for part in &building.parts {
            add_part(part, meshes);
        }
    }
    let mut out = ChunkMeshes::new();
    for (chunk, meshes) in chunks {
        for (surface, data) in SURFACES.into_iter().zip(meshes) {
            if !data.is_empty() {
                out.insert((chunk, surface), data);
            }
        }
    }
    out
}

const SURFACES: [Surface; 3] = [Surface::Office, Surface::Residential, Surface::Plain];

/// Walls, gable ends and roof of one part, into the mesh per surface
/// (indexed like `SURFACES`). No floor: a part stands on the ground or on
/// the part below.
fn add_part(part: &Part, meshes: &mut [MeshData; 3]) {
    let wall_surface = match part.facade {
        Facade::Office => Surface::Office,
        Facade::Residential => Surface::Residential,
        Facade::Plain => Surface::Plain,
    };
    let tile = facade_tile(wall_surface);
    let (lo, hi) = (part.min(), part.max());
    let (roof_height, ridge_along_x) = match part.roof {
        Roof::Flat => (0.0, true),
        Roof::Gable {
            ridge_along_x,
            height,
        } => (height, ridge_along_x),
    };
    let eave = hi.y - roof_height;

    for n in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
        // Seen from outside, `right` points to the viewer's right.
        let right = Vec3::Y.cross(n);
        let face = part.center + n * part.half_size.dot(n.abs());
        let half_width = part.half_size.dot(right.abs());
        let left = Vec3::new(face.x, 0.0, face.z) - right * half_width;
        let at = |s: f32, y: f32| left + right * s + Vec3::Y * y;
        let uv = |s: f32, y: f32| Vec2::new(s / tile.x, -y / tile.y);
        let w = 2.0 * half_width;
        meshes[wall_surface as usize].polygon(
            &[at(0.0, lo.y), at(w, lo.y), at(w, eave), at(0.0, eave)],
            &[uv(0.0, lo.y), uv(w, lo.y), uv(w, eave), uv(0.0, eave)],
            part.wall_color,
        );
        // Gable triangle on the faces across the ridge.
        let gable_end = roof_height > 0.0 && (n.x != 0.0) == ridge_along_x;
        if gable_end {
            meshes[wall_surface as usize].polygon(
                &[at(0.0, eave), at(w, eave), at(w / 2.0, hi.y)],
                &[uv(0.0, eave), uv(w, eave), uv(w / 2.0, hi.y)],
                part.wall_color,
            );
        }
        // Roof slope on the faces along the ridge.
        if roof_height > 0.0 && !gable_end {
            let ridge = Vec3::new(part.center.x, hi.y, part.center.z) - right * half_width;
            meshes[Surface::Plain as usize].polygon(
                &[at(0.0, eave), at(w, eave), ridge + right * w, ridge],
                &[Vec2::ZERO; 4],
                part.roof_color,
            );
        }
    }

    if roof_height == 0.0 {
        meshes[Surface::Plain as usize].polygon(
            &[
                Vec3::new(lo.x, hi.y, lo.z),
                Vec3::new(lo.x, hi.y, hi.z),
                Vec3::new(hi.x, hi.y, hi.z),
                Vec3::new(hi.x, hi.y, lo.z),
            ],
            &[Vec2::ZERO; 4],
            part.roof_color,
        );
    }
}
