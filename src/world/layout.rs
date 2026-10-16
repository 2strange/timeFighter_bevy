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

use super::{
    buildings::{farm_at, farm_buildings, fill_block},
    rng::Rng,
};

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
    // - Street 28 m: four ~3.8 m lanes plus 6 m sidewalks each side, a
    //   normal four-lane city street. With the 3 m setback the facades are ≥ 34 m
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

impl CityConfig {
    /// Checks that the generator terminates and produces a sane city.
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            ("grid_half_extent", self.grid_half_extent),
            ("city_radius", self.city_radius),
            ("block_min", self.block_min),
            ("block_max", self.block_max),
            ("street_width", self.street_width),
            ("avenue_width", self.avenue_width),
            ("boulevard_width", self.boulevard_width),
            ("tower_height_center", self.tower_height_center),
            ("tower_height_edge", self.tower_height_edge),
            ("ground_half_extent", self.ground_half_extent),
            ("inner_half_extent", self.inner_half_extent),
            ("patch_size", self.patch_size),
            ("chunk_size", self.chunk_size),
        ];
        let non_negative = [
            ("sidewalk_width", self.sidewalk_width),
            ("setback", self.setback),
            ("building_gap", self.building_gap),
            ("zone_noise", self.zone_noise),
        ];
        for (name, v) in positive {
            if !(v.is_finite() && v > 0.0) {
                return Err(format!("{name} must be a positive number, got {v}"));
            }
        }
        for (name, v) in non_negative {
            if !(v.is_finite() && v >= 0.0) {
                return Err(format!("{name} must be zero or positive, got {v}"));
            }
        }
        if self.avenue_every == 0 {
            return Err("avenue_every must be at least 1".into());
        }
        if self.block_min > self.block_max {
            return Err(format!(
                "block_min {} exceeds block_max {}",
                self.block_min, self.block_max
            ));
        }
        for class in [
            StreetClass::Street,
            StreetClass::Avenue,
            StreetClass::Boulevard,
        ] {
            let width = self.width(class);
            let paved = 2.0 * self.sidewalk_width + class.median_width();
            if width <= paved {
                return Err(format!(
                    "{class:?} width {width} leaves no lanes beside sidewalks and median ({paved} m)"
                ));
            }
        }
        if !(0.0 < self.downtown_radius
            && self.downtown_radius < self.midtown_radius
            && self.midtown_radius < 1.0)
        {
            return Err(format!(
                "zone radii must satisfy 0 < downtown ({}) < midtown ({}) < 1",
                self.downtown_radius, self.midtown_radius
            ));
        }
        if self.grid_half_extent >= self.inner_half_extent
            || self.inner_half_extent > self.ground_half_extent
        {
            return Err(format!(
                "extents must satisfy grid ({}) < inner ({}) <= ground ({})",
                self.grid_half_extent, self.inner_half_extent, self.ground_half_extent
            ));
        }
        let tiles = self.inner_half_extent / self.patch_size;
        if (tiles - tiles.round()).abs() > 1e-3 {
            return Err(format!(
                "inner_half_extent {} must be a multiple of patch_size {}",
                self.inner_half_extent, self.patch_size
            ));
        }
        // The ground mesh gets a breakpoint per patch edge; keep it bounded.
        if (self.ground_half_extent - self.inner_half_extent) / self.patch_size > 200.0 {
            return Err(format!(
                "patch_size {} is too small for the ground",
                self.patch_size
            ));
        }
        Ok(())
    }

    /// Corridor width of a street class.
    pub fn width(&self, class: StreetClass) -> f32 {
        match class {
            StreetClass::Street => self.street_width,
            StreetClass::Avenue => self.avenue_width,
            StreetClass::Boulevard => self.boulevard_width,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StreetClass {
    Street,
    Avenue,
    Boulevard,
}

impl StreetClass {
    /// Painted centre line (streets) or green median (wider classes).
    pub fn median_width(self) -> f32 {
        match self {
            StreetClass::Street => 0.6,
            StreetClass::Avenue => 4.0,
            StreetClass::Boulevard => 8.0,
        }
    }
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
    /// Generates the city; panics with the reason if `config` is invalid.
    pub fn generate(config: &CityConfig) -> Self {
        Self::try_generate(config).unwrap_or_else(|e| panic!("invalid CityConfig: {e}"))
    }

    pub fn try_generate(config: &CityConfig) -> Result<Self, String> {
        config.validate()?;
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
        Ok(layout)
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
        let mut rects: Vec<Rect> = self.street_segments().map(|(_, _, r)| r).collect();
        for (i, xl) in self.lines[0].iter().enumerate() {
            for (k, zl) in self.lines[1].iter().enumerate() {
                if self.crossing_active(0, i, k) || self.crossing_active(1, k, i) {
                    rects.push(Rect::new(xl.min(), zl.min(), xl.max(), zl.max()));
                }
            }
        }
        rects
    }

    /// Paved stretches between crossings as `(axis, line index, rect)`.
    pub fn street_segments(&self) -> impl Iterator<Item = (usize, usize, Rect)> + '_ {
        (0..2).flat_map(move |axis| {
            let other = 1 - axis;
            self.lines[axis]
                .iter()
                .enumerate()
                .flat_map(move |(i, line)| {
                    (0..=self.lines[other].len())
                        .filter(move |&g| self.segment_active(axis, i, g))
                        .map(move |g| {
                            let across = self.gap_range(other, g);
                            (axis, i, axis_rect(axis, (line.min(), line.max()), across))
                        })
                })
        })
    }

    /// Stable 64-bit hash (FNV-1a) of the generated data, independent of
    /// the platform and the Rust version. Native and WASM builds of the same
    /// seed must print the same value.
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv::default();
        for lines in &self.lines {
            h.usize(lines.len());
            for l in lines {
                h.f32s(&[l.center, l.width]);
                h.usize(l.class as usize);
            }
        }
        for b in &self.blocks {
            h.f32s(&[b.rect.min.x, b.rect.min.y, b.rect.max.x, b.rect.max.y]);
            h.usize(b.zone as usize);
        }
        h.usize(self.buildings.len());
        for b in &self.buildings {
            h.usize(b.kind as usize);
            h.usize(b.zone as usize);
            h.usize(b.parts.len());
            for p in &b.parts {
                h.f32s(&p.center.to_array());
                h.f32s(&p.half_size.to_array());
                h.usize(p.facade as usize);
                match p.roof {
                    Roof::Flat => h.usize(0),
                    Roof::Gable {
                        ridge_along_x,
                        height,
                    } => {
                        h.usize(1 + usize::from(ridge_along_x));
                        h.f32s(&[height]);
                    }
                }
                h.f32s(&p.wall_color);
                h.f32s(&p.roof_color);
            }
        }
        h.usize(self.farms.len());
        for f in &self.farms {
            let rects = std::iter::once((f.yard, [0.0; 3])).chain(f.strips.iter().copied());
            for (r, color) in rects {
                h.f32s(&[r.min.x, r.min.y, r.max.x, r.max.y]);
                h.f32s(&color);
            }
        }
        h.0
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
    let width = |class| cfg.width(class);
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

/// FNV-1a over little-endian bytes.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn usize(&mut self, v: usize) {
        self.bytes(&(v as u64).to_le_bytes());
    }
    fn f32s(&mut self, vs: &[f32]) {
        for v in vs {
            self.bytes(&v.to_bits().to_le_bytes());
        }
    }
}
