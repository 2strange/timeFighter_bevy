//! City layout as plain data: street grid, zoned blocks, buildings, farms.
//!
//! [`CityLayout::generate`] is a pure function of [`CityConfig`] (including the
//! seed), so it is tested without Bevy's renderer. Rendering and, later,
//! physics colliders are built from this data.
//!
//! The street grid is axis-aligned: "x-lines" have a constant x and run along
//! Z, "z-lines" have a constant z and run along X. A line's width is the full
//! corridor between the building lines (sidewalk to sidewalk); buildings keep
//! an extra setback, so the gap between facades is never below that width.
//!
//! Only basic float arithmetic and `sqrt` are used (no trigonometry or
//! `powf`), which keeps a seed's layout identical on native and WASM.

use bevy::{math::Rect, prelude::*};

/// Horizontal facade module (m). Footprints are multiples of it, so the
/// window raster ends cleanly at the building edges.
pub const BAY: f32 = 3.0;
/// Storey height of office towers (m).
pub const OFFICE_FLOOR: f32 = 3.6;
/// Storey height of residential and farm buildings (m).
pub const RESIDENTIAL_FLOOR: f32 = 3.0;

/// Tuning of the generator. All lengths in metres.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CityConfig {
    pub seed: u64,
    /// Street grid lines are generated up to this distance from the centre.
    pub grid_half_extent: f32,
    /// Distance at which the suburbs end; zone radii are fractions of it.
    pub city_radius: f32,
    /// Block edge length between two corridors, drawn uniformly per gap.
    pub block_min: f32,
    pub block_max: f32,
    /// Corridor widths, facade line to facade line.
    pub street_width: f32,
    pub avenue_width: f32,
    pub boulevard_width: f32,
    /// Every n-th line counted from the centre is an avenue.
    pub avenue_every: usize,
    pub sidewalk_width: f32,
    /// Minimum distance between a facade and the corridor edge.
    pub setback: f32,
    /// Minimum distance between two buildings in the same block.
    pub building_gap: f32,
    /// Zone limits as fractions of `city_radius` (suburbs reach 1.0).
    pub downtown_radius: f32,
    pub midtown_radius: f32,
    /// Random shift of a block's zone radius, so zone borders are ragged.
    pub zone_noise: f32,
    /// Height cap for towers at the very centre and at the downtown edge.
    pub tower_height_center: f32,
    pub tower_height_edge: f32,
    pub farm_count: usize,
    /// Half edge length of the whole ground plane.
    pub ground_half_extent: f32,
    /// Inside this square: city, farms and meadow; outside: patchwork land.
    pub inner_half_extent: f32,
    /// Edge length of one patchwork tile; `inner_half_extent` is a multiple.
    pub patch_size: f32,
    /// Edge length of a render chunk (buildings are merged per chunk).
    pub chunk_size: f32,
}

impl Default for CityConfig {
    // Scale reference: the jet spans 13 m and flies 50–500 m/s.
    // - Street 28 m: four 3.5 m lanes plus 7 m sidewalks each side, a normal
    //   four-lane city street. With the 3 m setback the facades are ≥ 34 m
    //   apart: tight but flyable wings level, easy in knife-edge.
    // - Avenue 48 m (≥ 54 m between facades): the canyons to fly through,
    //   ~20 m clearance per wingtip; one every third line, so every ~500 m.
    // - Boulevard 64 m: the two central axes crossing downtown; the x = 0
    //   boulevard lies straight ahead of the spawn point.
    fn default() -> Self {
        Self {
            seed: 0x7146_f16e,
            grid_half_extent: 3_000.0,
            city_radius: 2_400.0,
            block_min: 110.0,
            block_max: 190.0,
            street_width: 28.0,
            avenue_width: 48.0,
            boulevard_width: 64.0,
            avenue_every: 3,
            sidewalk_width: 6.0,
            setback: 3.0,
            building_gap: 6.0,
            downtown_radius: 0.26,
            midtown_radius: 0.52,
            zone_noise: 0.06,
            tower_height_center: 320.0,
            tower_height_edge: 110.0,
            farm_count: 7,
            ground_half_extent: 20_000.0,
            inner_half_extent: 4_000.0,
            patch_size: 500.0,
            chunk_size: 800.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StreetClass {
    Street,
    Avenue,
    Boulevard,
}

/// One street corridor of the grid, infinite along its direction; which
/// stretches are actually paved is decided per segment (see `segment_active`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreetLine {
    /// Centre coordinate across the line (x for x-lines, z for z-lines).
    pub center: f32,
    pub width: f32,
    pub class: StreetClass,
}

impl StreetLine {
    pub fn min(&self) -> f32 {
        self.center - self.width / 2.0
    }
    pub fn max(&self) -> f32 {
        self.center + self.width / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Zone {
    Downtown,
    Midtown,
    Suburb,
    Park,
    Rural,
}

impl Zone {
    /// Built-up blocks get their surrounding streets paved.
    pub fn is_urban(self) -> bool {
        !matches!(self, Zone::Rural)
    }
}

/// The land between four corridors.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// Ground rectangle (x, z).
    pub rect: Rect,
    pub zone: Zone,
}

/// Which texture a wall uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Facade {
    /// Glass ribbon windows, `OFFICE_FLOOR` storeys.
    Office,
    /// Punched windows, `RESIDENTIAL_FLOOR` storeys.
    Residential,
    /// No windows (rooftop plant, barn, silo).
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Roof {
    Flat,
    /// Pitched roof inside the part's box; `height` is eave to ridge.
    Gable {
        ridge_along_x: bool,
        height: f32,
    },
}

/// A box-shaped piece of a building. The visual shape fills the box exactly
/// (a gable roof stays inside it), so the box doubles as a collider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub center: Vec3,
    pub half_size: Vec3,
    pub facade: Facade,
    pub roof: Roof,
    /// sRGB colours.
    pub wall_color: [f32; 3],
    pub roof_color: [f32; 3],
}

impl Part {
    pub fn min(&self) -> Vec3 {
        self.center - self.half_size
    }
    pub fn max(&self) -> Vec3 {
        self.center + self.half_size
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BuildingKind {
    Tower,
    SetbackTower,
    MidRise,
    Slab,
    Apartment,
    House,
    Farmhouse,
    Barn,
    Silo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Building {
    pub kind: BuildingKind,
    pub zone: Zone,
    /// Stacked boxes; the first one stands on the ground and contains the
    /// footprint of all others.
    pub parts: Vec<Part>,
}

impl Building {
    /// Collision boxes for physics: `(centre, half extents)` per part.
    pub fn collision_boxes(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        self.parts.iter().map(|p| (p.center, p.half_size))
    }

    /// Axis-aligned bounds of all parts.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.parts.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p.min()), hi.max(p.max())),
        )
    }

    pub fn height(&self) -> f32 {
        self.bounds().1.y
    }
}

/// Farmland at the city edge: a yard with buildings next to a striped field.
#[derive(Clone, Debug, PartialEq)]
pub struct Farm {
    pub yard: Rect,
    /// Field strips with their sRGB crop colour.
    pub strips: Vec<(Rect, [f32; 3])>,
}

impl Farm {
    pub fn rect(&self) -> Rect {
        self.strips.iter().fold(self.yard, |r, (s, _)| r.union(*s))
    }
}

/// Where a coordinate falls on one axis of the street grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Inside the corridor of line `i`.
    Line(usize),
    /// Between line `g - 1` and line `g` (0 = before the first line).
    Gap(usize),
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CityLayout {
    pub config: CityConfig,
    /// `lines[0]`: x-lines (run along Z), `lines[1]`: z-lines (run along X),
    /// each sorted by centre.
    pub lines: [Vec<StreetLine>; 2],
    /// Row-major: block `(ix, iz)` at `iz * (nx - 1) + ix`, between x-lines
    /// `ix`, `ix + 1` and z-lines `iz`, `iz + 1`.
    pub blocks: Vec<Block>,
    pub buildings: Vec<Building>,
    pub farms: Vec<Farm>,
}

impl CityLayout {
    pub fn generate(config: &CityConfig) -> Self {
        let lines = [
            street_lines(config, &mut Rng::stream(config.seed, 1, 0, 0)),
            street_lines(config, &mut Rng::stream(config.seed, 2, 0, 0)),
        ];
        let mut layout = Self {
            config: config.clone(),
            lines,
            blocks: Vec::new(),
            buildings: Vec::new(),
            farms: Vec::new(),
        };

        let (nx, nz) = layout.block_counts();
        for iz in 0..nz {
            for ix in 0..nx {
                let (x, z) = (&layout.lines[0], &layout.lines[1]);
                let rect = Rect::new(x[ix].max(), z[iz].max(), x[ix + 1].min(), z[iz + 1].min());
                let mut rng = Rng::stream(config.seed, 3, ix as i64, iz as i64);
                let zone = zone_at(config, rect.center(), &mut rng);
                layout.blocks.push(Block { rect, zone });
            }
        }

        for (i, block) in layout.blocks.iter().enumerate() {
            let mut rng = Rng::stream(config.seed, 4, i as i64, 0);
            fill_block(config, block, &mut rng, &mut layout.buildings);
        }

        layout.place_farms();
        layout
    }

    /// Number of blocks along x and z.
    pub fn block_counts(&self) -> (usize, usize) {
        (
            self.lines[0].len().saturating_sub(1),
            self.lines[1].len().saturating_sub(1),
        )
    }

    /// Block `(ix, iz)`, or `None` outside the grid.
    pub fn block(&self, ix: isize, iz: isize) -> Option<&Block> {
        let (nx, nz) = self.block_counts();
        if ix < 0 || iz < 0 || ix as usize >= nx || iz as usize >= nz {
            return None;
        }
        self.blocks.get(iz as usize * nx + ix as usize)
    }

    fn urban(&self, ix: isize, iz: isize) -> bool {
        self.block(ix, iz).is_some_and(|b| b.zone.is_urban())
    }

    /// Locates coordinate `v` on `axis` (0 = x, 1 = z).
    pub fn slot(&self, axis: usize, v: f32) -> Slot {
        let lines = &self.lines[axis];
        let gap = lines.partition_point(|l| l.center < v);
        for i in [gap.wrapping_sub(1), gap] {
            if lines.get(i).is_some_and(|l| v > l.min() && v < l.max()) {
                return Slot::Line(i);
            }
        }
        Slot::Gap(gap)
    }

    /// Whether line `i` on `axis` is paved across gap `g` of the other axis.
    /// Boulevards run through the whole map; other streets only where they
    /// touch a built-up block.
    pub fn segment_active(&self, axis: usize, i: usize, g: usize) -> bool {
        if self.lines[axis][i].class == StreetClass::Boulevard {
            return true;
        }
        let (i, g) = (i as isize, g as isize);
        let (a, b) = if axis == 0 {
            ((i - 1, g - 1), (i, g - 1))
        } else {
            ((g - 1, i - 1), (g - 1, i))
        };
        self.urban(a.0, a.1) || self.urban(b.0, b.1)
    }

    /// Whether line `i` on `axis` is paved anywhere around crossing line `k`
    /// of the other axis.
    pub fn crossing_active(&self, axis: usize, i: usize, k: usize) -> bool {
        self.segment_active(axis, i, k) || self.segment_active(axis, i, k + 1)
    }

    /// Range of gap `g` along `axis`, open ends clamped to the ground edge.
    pub fn gap_range(&self, axis: usize, g: usize) -> (f32, f32) {
        let lines = &self.lines[axis];
        let edge = self.config.ground_half_extent;
        let lo = if g == 0 { -edge } else { lines[g - 1].max() };
        let hi = lines.get(g).map_or(edge, StreetLine::min);
        (lo, hi)
    }

    /// Every paved rectangle (x, z): active segments and their crossings.
    pub fn street_rects(&self) -> Vec<Rect> {
        let mut rects = Vec::new();
        for axis in 0..2 {
            let other = 1 - axis;
            for (i, line) in self.lines[axis].iter().enumerate() {
                for g in 0..=self.lines[other].len() {
                    if self.segment_active(axis, i, g) {
                        let (lo, hi) = self.gap_range(other, g);
                        rects.push(axis_rect(axis, (line.min(), line.max()), (lo, hi)));
                    }
                }
            }
        }
        for (i, xl) in self.lines[0].iter().enumerate() {
            for (k, zl) in self.lines[1].iter().enumerate() {
                if self.crossing_active(0, i, k) || self.crossing_active(1, k, i) {
                    rects.push(Rect::new(xl.min(), zl.min(), xl.max(), zl.max()));
                }
            }
        }
        rects
    }

    /// Farms in the countryside ring: clear of paved streets, urban blocks and
    /// each other.
    fn place_farms(&mut self) {
        let cfg = &self.config;
        let streets = self.street_rects();
        let urban: Vec<Rect> = self
            .blocks
            .iter()
            .filter(|b| b.zone.is_urban())
            .map(|b| b.rect)
            .collect();
        let mut rng = Rng::stream(cfg.seed, 5, 0, 0);
        let reach = cfg.inner_half_extent - 450.0;
        let mut farms = Vec::new();
        let mut buildings = Vec::new();

        for _ in 0..400 {
            if farms.len() >= cfg.farm_count {
                break;
            }
            let center = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
            let r = center.length() / cfg.city_radius;
            if !(1.08..=1.6).contains(&r) {
                continue;
            }
            let farm = farm_at(center, &mut rng);
            let rect = farm.rect();
            let clear = |others: &[Rect], margin: f32| {
                others
                    .iter()
                    .all(|o| rect.inflate(margin).intersect(*o).is_empty())
            };
            let other_farms: Vec<Rect> = farms.iter().map(Farm::rect).collect();
            if rect.min.x.abs().max(rect.max.x.abs()) > reach + 300.0
                || rect.min.y.abs().max(rect.max.y.abs()) > reach + 300.0
                || !clear(&streets, 15.0)
                || !clear(&urban, 30.0)
                || !clear(&other_farms, 60.0)
            {
                continue;
            }
            farm_buildings(farm.yard, &mut rng, &mut buildings);
            farms.push(farm);
        }
        self.farms = farms;
        self.buildings.extend(buildings);
    }
}

/// Rectangle with `along` on `axis` (0 = x) and `across` on the other axis.
pub fn axis_rect(axis: usize, along: (f32, f32), across: (f32, f32)) -> Rect {
    if axis == 0 {
        Rect::new(along.0, across.0, along.1, across.1)
    } else {
        Rect::new(across.0, along.0, across.1, along.1)
    }
}

/// Grid lines on one axis: a boulevard through the centre, then corridors
/// outwards on both sides with random block sizes in between.
fn street_lines(cfg: &CityConfig, rng: &mut Rng) -> Vec<StreetLine> {
    let width = |class| match class {
        StreetClass::Street => cfg.street_width,
        StreetClass::Avenue => cfg.avenue_width,
        StreetClass::Boulevard => cfg.boulevard_width,
    };
    let center = StreetLine {
        center: 0.0,
        width: cfg.boulevard_width,
        class: StreetClass::Boulevard,
    };
    let mut lines = vec![center];
    for dir in [1.0, -1.0] {
        let mut prev = center;
        for n in 1.. {
            let class = if n % cfg.avenue_every == 0 {
                StreetClass::Avenue
            } else {
                StreetClass::Street
            };
            let w = width(class);
            let block = rng.range(cfg.block_min, cfg.block_max);
            let c = prev.center + dir * (prev.width / 2.0 + block + w / 2.0);
            if c.abs() > cfg.grid_half_extent {
                break;
            }
            prev = StreetLine {
                center: c,
                width: w,
                class,
            };
            lines.push(prev);
        }
    }
    lines.sort_by(|a, b| a.center.total_cmp(&b.center));
    lines
}

fn zone_at(cfg: &CityConfig, center: Vec2, rng: &mut Rng) -> Zone {
    let r = center.length() / cfg.city_radius + rng.range(-cfg.zone_noise, cfg.zone_noise);
    if r < cfg.downtown_radius {
        Zone::Downtown
    } else if r < cfg.midtown_radius {
        if rng.chance(0.05) {
            Zone::Park
        } else {
            Zone::Midtown
        }
    } else if r < 1.0 {
        if rng.chance(0.04) {
            Zone::Park
        } else {
            Zone::Suburb
        }
    } else {
        Zone::Rural
    }
}

// Calm palette (sRGB).
const OFFICE_TINTS: [[f32; 3]; 4] = [
    [0.80, 0.85, 0.90],
    [0.88, 0.88, 0.86],
    [0.76, 0.80, 0.85],
    [0.92, 0.89, 0.83],
];
const RESIDENTIAL_WALLS: [[f32; 3]; 6] = [
    [0.90, 0.87, 0.80],
    [0.85, 0.79, 0.68],
    [0.86, 0.75, 0.65],
    [0.80, 0.80, 0.78],
    [0.87, 0.85, 0.76],
    [0.74, 0.77, 0.80],
];
const FLAT_ROOFS: [[f32; 3]; 3] = [[0.42, 0.42, 0.43], [0.50, 0.49, 0.47], [0.36, 0.37, 0.39]];
const PITCHED_ROOFS: [[f32; 3]; 3] = [[0.62, 0.30, 0.22], [0.36, 0.36, 0.39], [0.48, 0.32, 0.24]];
const PLANT_GREY: [f32; 3] = [0.58, 0.58, 0.57];
const BARN_RED: [f32; 3] = [0.55, 0.22, 0.16];
const SILO_WHITE: [f32; 3] = [0.78, 0.78, 0.75];
const CROPS: [[f32; 3]; 4] = [
    [0.76, 0.68, 0.40],
    [0.48, 0.62, 0.30],
    [0.47, 0.37, 0.27],
    [0.80, 0.76, 0.36],
];

/// Snaps `v` down to a multiple of `step`, at least one step.
fn snap(v: f32, step: f32) -> f32 {
    ((v / step).floor() * step).max(step)
}

/// Splits `rect` into a grid of lots close to `target` edge length.
fn lots(rect: Rect, target: Vec2) -> Vec<Rect> {
    let size = rect.size();
    let n = (size / target).round().max(Vec2::ONE);
    let (nx, nz) = (n.x as usize, n.y as usize);
    let step = size / n;
    let mut out = Vec::with_capacity(nx * nz);
    for j in 0..nz {
        for i in 0..nx {
            let min = rect.min + step * Vec2::new(i as f32, j as f32);
            out.push(Rect::from_corners(min, min + step));
        }
    }
    out
}

/// A footprint of `want` (snapped to the bay) at a random spot in `lot`, or
/// `None` if the lot is too small.
fn footprint_in(lot: Rect, want: Vec2, rng: &mut Rng) -> Option<Rect> {
    let size = Vec2::new(
        snap(want.x.min(lot.width()), BAY),
        snap(want.y.min(lot.height()), BAY),
    );
    if size.x > lot.width() || size.y > lot.height() {
        return None;
    }
    let slack = lot.size() - size;
    let min = lot.min + slack * Vec2::new(rng.f32(), rng.f32());
    Some(Rect::from_corners(min, min + size))
}

/// Box part standing on `base_y` over `footprint`.
fn part(
    footprint: Rect,
    base_y: f32,
    height: f32,
    facade: Facade,
    wall: [f32; 3],
    roof: [f32; 3],
) -> Part {
    let c = footprint.center();
    let h = footprint.half_size();
    Part {
        center: Vec3::new(c.x, base_y + height / 2.0, c.y),
        half_size: Vec3::new(h.x, height / 2.0, h.y),
        facade,
        roof: Roof::Flat,
        wall_color: wall,
        roof_color: roof,
    }
}

/// `footprint` shrunk to `factor` around its centre, snapped to the bay.
fn shrink(footprint: Rect, factor: f32) -> Rect {
    let size = footprint.size();
    let new = Vec2::new(snap(size.x * factor, BAY), snap(size.y * factor, BAY)).min(size);
    Rect::from_center_size(footprint.center(), new)
}

fn fill_block(cfg: &CityConfig, block: &Block, rng: &mut Rng, out: &mut Vec<Building>) {
    let inner = block.rect.inflate(-cfg.setback);
    // Lots are shrunk by half a gap each, so neighbours keep `building_gap`
    // and the block edge keeps at least `setback`.
    let lot_rects = |target: Vec2| -> Vec<Rect> {
        lots(inner, target)
            .into_iter()
            .map(|l| l.inflate(-cfg.building_gap / 2.0))
            .filter(|l| !l.is_empty())
            .collect()
    };
    let r = block.rect.center().length() / cfg.city_radius;

    match block.zone {
        Zone::Downtown => {
            let t = (r / cfg.downtown_radius).clamp(0.0, 1.0);
            let cap =
                cfg.tower_height_center + (cfg.tower_height_edge - cfg.tower_height_center) * t;
            for lot in lot_rects(Vec2::splat(60.0)) {
                let want = lot.size() * Vec2::new(rng.range(0.7, 1.0), rng.range(0.7, 1.0));
                let Some(fp) = footprint_in(lot, want.max(Vec2::splat(18.0)), rng) else {
                    continue;
                };
                let height = snap(cap * rng.range(0.5, 1.0), OFFICE_FLOOR).max(OFFICE_FLOOR * 12.0);
                out.push(tower(fp, height, rng));
            }
        }
        Zone::Midtown => {
            let t = ((r - cfg.downtown_radius) / (cfg.midtown_radius - cfg.downtown_radius))
                .clamp(0.0, 1.0);
            let cap = 75.0 - 45.0 * t;
            if rng.chance(0.3) {
                slabs(inner, rng, out);
                return;
            }
            for lot in lot_rects(Vec2::splat(45.0)) {
                let want = lot.size() * Vec2::new(rng.range(0.6, 0.95), rng.range(0.6, 0.95));
                let Some(fp) = footprint_in(lot, want, rng) else {
                    continue;
                };
                let office = rng.chance(0.3);
                let (facade, floor, wall) = if office {
                    (Facade::Office, OFFICE_FLOOR, rng.pick(&OFFICE_TINTS))
                } else {
                    (
                        Facade::Residential,
                        RESIDENTIAL_FLOOR,
                        rng.pick(&RESIDENTIAL_WALLS),
                    )
                };
                let height = snap(cap * rng.range(0.55, 1.0), floor).max(floor * 5.0);
                let mut parts = vec![part(fp, 0.0, height, facade, wall, rng.pick(&FLAT_ROOFS))];
                if rng.chance(0.2) {
                    parts.push(rooftop(&parts[0], rng));
                }
                out.push(Building {
                    kind: BuildingKind::MidRise,
                    zone: Zone::Midtown,
                    parts,
                });
            }
        }
        Zone::Suburb => {
            let t = ((r - cfg.midtown_radius) / (1.0 - cfg.midtown_radius)).clamp(0.0, 1.0);
            let occupancy = 0.9 - 0.45 * t;
            for lot in lot_rects(Vec2::new(26.0, 32.0)) {
                if !rng.chance(occupancy) {
                    continue;
                }
                if t < 0.3 && rng.chance(0.15) {
                    let want = Vec2::new(rng.range(15.0, 24.0), rng.range(12.0, 18.0));
                    if let Some(fp) = footprint_in(lot, want, rng) {
                        let height = RESIDENTIAL_FLOOR * (3 + rng.below(2)) as f32;
                        let wall = rng.pick(&RESIDENTIAL_WALLS);
                        out.push(Building {
                            kind: BuildingKind::Apartment,
                            zone: Zone::Suburb,
                            parts: vec![part(
                                fp,
                                0.0,
                                height,
                                Facade::Residential,
                                wall,
                                rng.pick(&FLAT_ROOFS),
                            )],
                        });
                    }
                    continue;
                }
                let want = Vec2::new(rng.range(9.0, 16.0), rng.range(9.0, 13.0));
                let Some(fp) = footprint_in(lot, want, rng) else {
                    continue;
                };
                // Two storeys near town, mostly bungalows at the edge.
                let floors = if rng.chance(0.8 - 0.6 * t) { 2.0 } else { 1.0 };
                let walls = RESIDENTIAL_FLOOR * floors;
                let wall = rng.pick(&RESIDENTIAL_WALLS);
                let roof = rng.pick(&PITCHED_ROOFS);
                out.push(Building {
                    kind: BuildingKind::House,
                    zone: Zone::Suburb,
                    parts: vec![gabled(fp, walls, Facade::Residential, wall, roof)],
                });
            }
        }
        Zone::Park | Zone::Rural => {}
    }
}

/// Office tower, either one shaft or two to three setback tiers.
fn tower(fp: Rect, height: f32, rng: &mut Rng) -> Building {
    let tint = rng.pick(&OFFICE_TINTS);
    let roof = rng.pick(&FLAT_ROOFS);
    let mut parts = Vec::new();
    let kind = if height > 120.0 && rng.chance(0.45) {
        let tiers = 2 + rng.below(2);
        let mut base_y = 0.0;
        let mut footprint = fp;
        for i in 0..tiers {
            let remaining = height - base_y;
            let h = if i + 1 == tiers {
                remaining
            } else {
                snap(remaining * rng.range(0.4, 0.6), OFFICE_FLOOR)
            };
            parts.push(part(footprint, base_y, h, Facade::Office, tint, roof));
            base_y += h;
            footprint = shrink(footprint, rng.range(0.65, 0.85));
        }
        BuildingKind::SetbackTower
    } else {
        parts.push(part(fp, 0.0, height, Facade::Office, tint, roof));
        BuildingKind::Tower
    };
    if rng.chance(0.5) {
        let top = rooftop(parts.last().unwrap(), rng);
        parts.push(top);
    }
    Building {
        kind,
        zone: Zone::Downtown,
        parts,
    }
}

/// Small plant room on top of `below`.
fn rooftop(below: &Part, rng: &mut Rng) -> Part {
    let footprint = Rect::from_center_half_size(below.center.xz(), below.half_size.xz());
    let fp = shrink(footprint, rng.range(0.3, 0.45));
    let height = rng.range(4.0, 8.0).round();
    part(
        fp,
        below.max().y,
        height,
        Facade::Plain,
        PLANT_GREY,
        below.roof_color,
    )
}

/// Rows of long residential slabs along the block's long side.
fn slabs(inner: Rect, rng: &mut Rng, out: &mut Vec<Building>) {
    let along_x = inner.width() >= inner.height();
    let (long, short) = if along_x {
        (inner.width(), inner.height())
    } else {
        (inner.height(), inner.width())
    };
    let depth = if rng.chance(0.5) { 12.0 } else { 15.0 };
    let court = 24.0;
    let n = (((short + court) / (depth + court)).floor() as usize).max(1);
    let pitch = if n > 1 {
        (short - depth) / (n - 1) as f32
    } else {
        0.0
    };
    let length = snap(long * rng.range(0.75, 1.0), BAY).min(snap(long, BAY));
    let wall = rng.pick(&RESIDENTIAL_WALLS);
    let roof = rng.pick(&FLAT_ROOFS);
    for i in 0..n {
        let across = if n > 1 {
            -short / 2.0 + depth / 2.0 + pitch * i as f32
        } else {
            0.0
        };
        let height = RESIDENTIAL_FLOOR * (10 + rng.below(7)) as f32;
        let size = if along_x {
            Vec2::new(length, depth)
        } else {
            Vec2::new(depth, length)
        };
        let offset = if along_x {
            Vec2::new(0.0, across)
        } else {
            Vec2::new(across, 0.0)
        };
        let fp = Rect::from_center_size(inner.center() + offset, size);
        out.push(Building {
            kind: BuildingKind::Slab,
            zone: Zone::Midtown,
            parts: vec![part(fp, 0.0, height, Facade::Residential, wall, roof)],
        });
    }
}

/// Box with `walls` metres of wall and a gable roof over the shorter span.
fn gabled(fp: Rect, walls: f32, facade: Facade, wall: [f32; 3], roof: [f32; 3]) -> Part {
    let ridge_along_x = fp.width() >= fp.height();
    let span = fp.width().min(fp.height());
    let roof_height = (span * 0.35).round();
    Part {
        roof: Roof::Gable {
            ridge_along_x,
            height: roof_height,
        },
        ..part(fp, 0.0, walls + roof_height, facade, wall, roof)
    }
}

/// A farm (yard beside a field of three crop strips) centred near `center`.
fn farm_at(center: Vec2, rng: &mut Rng) -> Farm {
    let field = Vec2::new(rng.range(220.0, 360.0), rng.range(160.0, 260.0)).round();
    let yard_size = 72.0;
    let field_rect = Rect::from_center_size(center, field);
    let yard_x = if rng.chance(0.5) {
        field_rect.max.x
    } else {
        field_rect.min.x - yard_size
    };
    let yard = Rect::new(
        yard_x,
        center.y - yard_size / 2.0,
        yard_x + yard_size,
        center.y + yard_size / 2.0,
    );
    let strip = field.x / 3.0;
    let strips = (0..3)
        .map(|i| {
            let x0 = field_rect.min.x + strip * i as f32;
            let x1 = if i == 2 { field_rect.max.x } else { x0 + strip };
            (
                Rect::new(x0, field_rect.min.y, x1, field_rect.max.y),
                rng.pick(&CROPS),
            )
        })
        .collect();
    Farm { yard, strips }
}

/// Farmhouse, barn and silo in a row along the yard.
fn farm_buildings(yard: Rect, rng: &mut Rng, out: &mut Vec<Building>) {
    let m = 6.0;
    let house = Rect::new(
        yard.min.x + m,
        yard.min.y + m,
        yard.min.x + m + 15.0,
        yard.min.y + m + 9.0,
    );
    let barn = Rect::new(
        yard.min.x + m,
        yard.max.y - m - 15.0,
        yard.min.x + m + 27.0,
        yard.max.y - m,
    );
    let silo = Rect::from_center_size(
        Vec2::new(yard.max.x - m - 6.0, yard.center().y),
        Vec2::splat(6.0),
    );
    out.push(Building {
        kind: BuildingKind::Farmhouse,
        zone: Zone::Rural,
        parts: vec![gabled(
            house,
            RESIDENTIAL_FLOOR * 2.0,
            Facade::Residential,
            RESIDENTIAL_WALLS[0],
            rng.pick(&PITCHED_ROOFS),
        )],
    });
    out.push(Building {
        kind: BuildingKind::Barn,
        zone: Zone::Rural,
        parts: vec![gabled(barn, 8.0, Facade::Plain, BARN_RED, PITCHED_ROOFS[1])],
    });
    out.push(Building {
        kind: BuildingKind::Silo,
        zone: Zone::Rural,
        parts: vec![part(
            silo,
            0.0,
            18.0,
            Facade::Plain,
            SILO_WHITE,
            FLAT_ROOFS[0],
        )],
    });
}

/// SplitMix64: tiny, fast, and identical on every platform.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    /// Independent stream per `(seed, tag, a, b)`, so changing one block
    /// does not reshuffle the rest of the city.
    pub(crate) fn stream(seed: u64, tag: u64, a: i64, b: i64) -> Self {
        let mut r = Rng(seed ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let x = r.next_u64() ^ (a as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        let mut r = Rng(x);
        let y = r.next_u64() ^ (b as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
        Rng(y)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub(crate) fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    pub(crate) fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// Uniform in `0..n`.
    pub(crate) fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n)) as u32
    }

    pub(crate) fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u32) as usize]
    }
}
