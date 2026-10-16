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
pub const ASPHALT: [f32; 3] = [0.30, 0.31, 0.33];
const SIDEWALK: [f32; 3] = [0.58, 0.57, 0.55];
/// Low contrast on purpose: thin bright lines shimmer at a distance.
const CENTER_LINE: [f32; 3] = [0.62, 0.60, 0.52];
const MEDIAN: [f32; 3] = [0.33, 0.45, 0.27];
const PLAZA: [f32; 3] = [0.52, 0.51, 0.49];
const COURTYARD: [f32; 3] = [0.47, 0.48, 0.43];
const LAWN: [f32; 3] = [0.40, 0.52, 0.30];
const PARK: [f32; 3] = [0.30, 0.47, 0.24];
pub const MEADOW: [f32; 3] = [0.42, 0.54, 0.30];
const YARD: [f32; 3] = [0.55, 0.49, 0.40];
pub const WATER: [f32; 3] = [0.20, 0.33, 0.42];
const QUAY: [f32; 3] = [0.62, 0.60, 0.56];
const QUAY_WALL: [f32; 3] = [0.50, 0.48, 0.45];
const PATCHES: [[f32; 3]; 4] = [
    [0.40, 0.52, 0.29],
    [0.45, 0.55, 0.31],
    [0.37, 0.48, 0.27],
    [0.49, 0.53, 0.33],
];

/// One coloured rectangle of the ground (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundQuad {
    pub rect: Rect,
    pub color: [f32; 3],
}

impl GroundQuad {
    /// Water gets its own, glossier material.
    pub fn is_water(&self) -> bool {
        self.color == WATER
    }
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
/// Every street, quay and patch edge becomes a breakpoint on its axis; each
/// cell of the resulting grid is classified at its centre. Equal plain
/// cells are merged along x, street cells are split into their
/// cross-section strips. Farm fields are cut in afterwards, so their edges
/// do not split the whole map. All quads lie in the plane y = 0, never on
/// top of each other, so nothing can z-fight.
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
        // Quay edges only where a water cell borders land on this axis, so
        // a long river column does not split every row of the map.
        if let Some(river) = &cfg.river {
            for &(gx, gz) in &layout.river {
                let (g, other) = if axis == 0 { (gx, gz) } else { (gz, gx) };
                let water = |g: usize| {
                    if axis == 0 {
                        layout.is_water(g, other)
                    } else {
                        layout.is_water(other, g)
                    }
                };
                let (lo, hi) = layout.gap_range(axis, g);
                if g > 0 && !water(g - 1) {
                    out.push(lo + river.quay_width);
                }
                if g < layout.lines[axis].len() && !water(g + 1) {
                    out.push(hi - river.quay_width);
                }
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

    for farm in &layout.farms {
        let fields = farm.strips.iter().copied();
        for (rect, color) in std::iter::once((farm.yard, YARD)).chain(fields) {
            quads = cut_in(quads, rect, color);
        }
    }
    quads
}

/// Replaces the area of `rect` in `quads` with `color`, splitting the quads
/// it touches into the parts outside it.
fn cut_in(quads: Vec<GroundQuad>, rect: Rect, color: [f32; 3]) -> Vec<GroundQuad> {
    let mut out = Vec::with_capacity(quads.len() + 8);
    for q in quads {
        let hit = q.rect.intersect(rect);
        if hit.is_empty() {
            out.push(q);
            continue;
        }
        let r = q.rect;
        let pieces = [
            Rect::new(r.min.x, r.min.y, r.max.x, hit.min.y),
            Rect::new(r.min.x, hit.max.y, r.max.x, r.max.y),
            Rect::new(r.min.x, hit.min.y, hit.min.x, hit.max.y),
            Rect::new(hit.max.x, hit.min.y, r.max.x, hit.max.y),
        ];
        for piece in pieces.into_iter().filter(|p| !p.is_empty()) {
            out.push(GroundQuad {
                rect: piece,
                color: q.color,
            });
        }
        out.push(GroundQuad { rect: hit, color });
    }
    out
}

fn classify(layout: &CityLayout, p: Vec2) -> Cell {
    if layout.on_river(p) {
        return Cell::Plain(river_surface(layout, p));
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
                Zone::Rural | Zone::Water => countryside(layout, p),
            })
        }
        _ => Cell::Plain(countryside(layout, p)),
    }
}

/// Quay along the banks, water in between. A point is quay when land lies
/// within one quay width of it.
fn river_surface(layout: &CityLayout, p: Vec2) -> [f32; 3] {
    let quay = layout.config.river.as_ref().map_or(0.0, |r| r.quay_width);
    let probes = [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y].map(|d| p + d * quay);
    if probes.iter().all(|&q| layout.on_river(q)) {
        WATER
    } else {
        QUAY
    }
}

/// Whether `p` shows open water (sunken), as opposed to quay or land.
pub fn is_water_at(layout: &CityLayout, p: Vec2) -> bool {
    layout.on_river(p) && river_surface(layout, p) == WATER
}

/// Vertical quay wall from the water up to street level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuayWall {
    /// 0: the wall lies in the plane x = `at`; 1: in the plane z = `at`.
    pub axis: usize,
    pub at: f32,
    /// Extent along the other horizontal axis.
    pub from: f32,
    pub to: f32,
    /// Whether the water lies on the + side (the wall faces +x / +z).
    pub water_positive: bool,
}

/// Walls along every edge where open water meets quay or land: each water
/// quad edge minus the parts shared with a neighbouring water quad. Edges
/// at the ground border are left open.
pub fn quay_walls(layout: &CityLayout, quads: &[GroundQuad]) -> Vec<QuayWall> {
    type Edges = (Vec<(f32, f32)>, Vec<(f32, f32)>);
    let mut edges: BTreeMap<(usize, u32), Edges> = BTreeMap::new();
    for q in quads.iter().filter(|q| q.is_water()) {
        let r = q.rect;
        for (axis, lo, hi, span) in [
            (0, r.min.x, r.max.x, (r.min.y, r.max.y)),
            (1, r.min.y, r.max.y, (r.min.x, r.max.x)),
        ] {
            // Water lies on the + side of the low edge, the − side of the high.
            edges.entry((axis, lo.to_bits())).or_default().0.push(span);
            edges.entry((axis, hi.to_bits())).or_default().1.push(span);
        }
    }
    let edge = layout.config.ground_half_extent;
    let mut walls = Vec::new();
    for ((axis, bits), (pos, neg)) in edges {
        let at = f32::from_bits(bits);
        if at.abs() >= edge {
            continue;
        }
        let (pos, neg) = (merge_spans(pos), merge_spans(neg));
        for (from, to) in subtract_spans(&pos, &neg) {
            walls.push(QuayWall {
                axis,
                at,
                from,
                to,
                water_positive: true,
            });
        }
        for (from, to) in subtract_spans(&neg, &pos) {
            walls.push(QuayWall {
                axis,
                at,
                from,
                to,
                water_positive: false,
            });
        }
    }
    walls
}

/// Sorted, merged union of 1-D spans.
fn merge_spans(mut spans: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(spans.len());
    for (a, b) in spans {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// `a` minus `b`, both sorted and merged.
fn subtract_spans(a: &[(f32, f32)], b: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for &(mut lo, hi) in a {
        for &(blo, bhi) in b {
            if bhi <= lo || blo >= hi {
                continue;
            }
            if blo > lo {
                out.push((lo, blo));
            }
            lo = lo.max(bhi);
            if lo >= hi {
                break;
            }
        }
        if lo < hi {
            out.push((lo, hi));
        }
    }
    out
}

/// Quay walls as quads facing the water, from `-depth` up to 0.
pub fn quay_wall_mesh_data(walls: &[QuayWall], depth: f32) -> MeshData {
    let mut data = MeshData::default();
    for w in walls {
        let corner = |s: f32, y: f32| {
            if w.axis == 0 {
                Vec3::new(w.at, y, s)
            } else {
                Vec3::new(s, y, w.at)
            }
        };
        let verts = [
            corner(w.from, -depth),
            corner(w.to, -depth),
            corner(w.to, 0.0),
            corner(w.from, 0.0),
        ];
        let mut toward_water = Vec3::ZERO;
        toward_water[if w.axis == 0 { 0 } else { 2 }] = if w.water_positive { 1.0 } else { -1.0 };
        let land_side = (verts[0] + verts[2]) / 2.0 - toward_water;
        data.polygon_facing_out(&verts, land_side, QUAY_WALL);
    }
    data
}

/// Land surface for the physics colliders: the ground minus open water, as
/// few rectangles as possible (x, z). Together with a water plane at
/// `-water_depth` they make the ground solid, including the quay walls.
pub fn land_rects(layout: &CityLayout, quads: &[GroundQuad]) -> Vec<Rect> {
    let edge = layout.config.ground_half_extent;
    let mut breaks: [Vec<f32>; 2] = [vec![-edge, edge], vec![-edge, edge]];
    for q in quads.iter().filter(|q| q.is_water()) {
        breaks[0].extend([q.rect.min.x, q.rect.max.x]);
        breaks[1].extend([q.rect.min.y, q.rect.max.y]);
    }
    for b in &mut breaks {
        b.sort_by(f32::total_cmp);
        b.dedup();
    }
    let mut rects = Vec::new();
    for zw in breaks[1].windows(2) {
        let mut run: Option<(f32, f32)> = None;
        for xw in breaks[0].windows(2) {
            let centre = Vec2::new((xw[0] + xw[1]) / 2.0, (zw[0] + zw[1]) / 2.0);
            if is_water_at(layout, centre) {
                if let Some((x0, x1)) = run.take() {
                    rects.push(Rect::new(x0, zw[0], x1, zw[1]));
                }
            } else {
                run = Some(run.map_or((xw[0], xw[1]), |(x0, _)| (x0, xw[1])));
            }
        }
        if let Some((x0, x1)) = run {
            rects.push(Rect::new(x0, zw[0], x1, zw[1]));
        }
    }
    rects
}

/// Meadow near the city, a patchwork of fields further out for a sense of
/// speed when flying low over open land.
fn countryside(layout: &CityLayout, p: Vec2) -> [f32; 3] {
    let cfg = &layout.config;
    if p.x.abs() < cfg.inner_half_extent && p.y.abs() < cfg.inner_half_extent {
        return MEADOW;
    }
    let tile = (p / cfg.patch_size).floor();
    let mut rng = super::rng::Rng::stream(cfg.seed, 6, tile.x as i64, tile.y as i64);
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
    let median = line.class.median_width();
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
    /// Like `polygon`, but flips the winding if needed so the face points
    /// away from `inside`.
    fn polygon_facing_out(&mut self, verts: &[Vec3], inside: Vec3, srgb: [f32; 3]) {
        let normal = (verts[1] - verts[0]).cross(verts[2] - verts[0]);
        let centroid = verts.iter().sum::<Vec3>() / verts.len() as f32;
        let uvs = vec![Vec2::ZERO; verts.len()];
        if normal.dot(centroid - inside) >= 0.0 {
            self.polygon(verts, &uvs, srgb);
        } else {
            let reversed: Vec<Vec3> = verts.iter().rev().copied().collect();
            self.polygon(&reversed, &uvs, srgb);
        }
    }

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

    pub fn extend(&mut self, other: MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.uvs.extend(other.uvs);
        self.colors.extend(other.colors);
        self.indices
            .extend(other.indices.into_iter().map(|i| i + base));
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

/// Flat quads at height `y` (0 for land, minus the water depth for water).
pub fn ground_mesh_data(quads: &[GroundQuad], y: f32) -> MeshData {
    let mut data = MeshData::default();
    for q in quads {
        let (a, b) = (q.rect.min, q.rect.max);
        // Counter-clockwise seen from above: the normal points up.
        let verts = [
            Vec3::new(a.x, y, a.y),
            Vec3::new(a.x, y, b.y),
            Vec3::new(b.x, y, b.y),
            Vec3::new(b.x, y, a.y),
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
    building_meshes_with(layout, &[])
}

/// Like [`building_meshes`], with destroyed buildings (`destroyed[i]`) drawn
/// as their ruin. A shorter slice counts as "nothing destroyed" beyond it.
pub fn building_meshes_with(layout: &CityLayout, destroyed: &[bool]) -> ChunkMeshes {
    let mut chunks: BTreeMap<(i32, i32), [MeshData; 3]> = BTreeMap::new();
    for parts in structures(layout, destroyed) {
        let meshes = chunks.entry(chunk_of(layout, &parts)).or_default();
        for part in parts.iter() {
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

/// The meshes of one chunk only, per surface (indexed like `SURFACES`),
/// for rebuilding it after damage.
pub fn chunk_meshes(layout: &CityLayout, destroyed: &[bool], chunk: (i32, i32)) -> [MeshData; 3] {
    let mut meshes: [MeshData; 3] = Default::default();
    for parts in structures(layout, destroyed) {
        if chunk_of(layout, &parts) == chunk {
            for part in parts.iter() {
                add_part(part, &mut meshes);
            }
        }
    }
    meshes
}

/// Chunk a building (by its original parts) or bridge belongs to. A ruin
/// stands on its building's base, so it stays in the same chunk.
pub fn chunk_of(layout: &CityLayout, parts: &[Part]) -> (i32, i32) {
    // The base part decides: ruins keep it, so damage never moves a building
    // into another chunk.
    let base = parts[0];
    let c = (base.center.xz() / layout.config.chunk_size).floor();
    (c.x as i32, c.y as i32)
}

/// Parts to draw per building and bridge, ruins for destroyed buildings.
fn structures<'a>(
    layout: &'a CityLayout,
    destroyed: &'a [bool],
) -> impl Iterator<Item = std::borrow::Cow<'a, [Part]>> + 'a {
    let buildings = layout.buildings.iter().enumerate().map(|(i, b)| {
        if destroyed.get(i).copied().unwrap_or(false) {
            std::borrow::Cow::Owned(vec![super::damage::ruin_part(b)])
        } else {
            std::borrow::Cow::Borrowed(b.parts.as_slice())
        }
    });
    let bridges = layout
        .bridges
        .iter()
        .map(|b| std::borrow::Cow::Borrowed(b.parts.as_slice()));
    buildings.chain(bridges)
}

pub const SURFACES: [Surface; 3] = [Surface::Office, Surface::Residential, Surface::Plain];

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
        Roof::Ramp {
            along_x,
            up_positive,
            thickness,
        } => {
            add_ramp(
                part,
                along_x,
                up_positive,
                thickness,
                &mut meshes[Surface::Plain as usize],
            );
            return;
        }
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

    if part.underside {
        // Counter-clockwise seen from below: the normal points down.
        meshes[Surface::Plain as usize].polygon(
            &[
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(lo.x, lo.y, hi.z),
            ],
            &[Vec2::ZERO; 4],
            part.wall_color,
        );
    }
}

/// Wedge-shaped ramp slab (see `Roof::Ramp`): top from the low edge up to
/// the deck, underside from the same edge up to the deck's underside.
fn add_ramp(part: &Part, along_x: bool, up_positive: bool, thickness: f32, mesh: &mut MeshData) {
    let (lo, hi) = (part.min(), part.max());
    let (run, across) = if along_x { (0, 2) } else { (2, 0) };
    let (low, high) = if up_positive {
        (lo[run], hi[run])
    } else {
        (hi[run], lo[run])
    };
    let at = |s: f32, c: f32, y: f32| {
        let mut v = Vec3::ZERO;
        v[run] = s;
        v[across] = c;
        v.y = y;
        v
    };
    let (c0, c1) = (lo[across], hi[across]);
    let (a0, a1) = (at(low, c0, lo.y), at(low, c1, lo.y));
    let (t0, t1) = (at(high, c0, hi.y), at(high, c1, hi.y));
    let (b0, b1) = (
        at(high, c0, hi.y - thickness),
        at(high, c1, hi.y - thickness),
    );
    let inside = (a0 + a1 + t0 + t1 + b0 + b1) / 6.0;
    mesh.polygon_facing_out(&[a0, a1, t1, t0], inside, part.roof_color);
    mesh.polygon_facing_out(&[a0, a1, b1, b0], inside, part.wall_color);
    mesh.polygon_facing_out(&[t0, t1, b1, b0], inside, part.wall_color);
    mesh.polygon_facing_out(&[a0, t0, b0], inside, part.wall_color);
    mesh.polygon_facing_out(&[a1, t1, b1], inside, part.wall_color);
}
