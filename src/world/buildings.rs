//! Building placement: what stands on a block of each zone, and farms.
//!
//! Every building is a stack of boxes with whole window bays, kept apart by
//! `building_gap` and from the street by `setback`.

use bevy::{math::Rect, prelude::*};

use super::{
    layout::{
        BAY, Block, Building, BuildingKind, CityConfig, Facade, Farm, OFFICE_FLOOR, Part,
        RESIDENTIAL_FLOOR, Roof, Zone,
    },
    rng::Rng,
};

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

pub(super) fn fill_block(cfg: &CityConfig, block: &Block, rng: &mut Rng, out: &mut Vec<Building>) {
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
pub(super) fn farm_at(center: Vec2, rng: &mut Rng) -> Farm {
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
pub(super) fn farm_buildings(yard: Rect, rng: &mut Rng, out: &mut Vec<Building>) {
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
