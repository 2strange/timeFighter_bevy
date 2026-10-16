//! Headless tests of the city generator and its meshes (no window, no renderer).

use bevy::{math::Rect, prelude::*};

use super::{
    CityPlugin, Lighting, Target, WEB_CAMERA_FAR, WorldRenderConfig,
    layout::{
        Bridge, BuildingKind, CityConfig, CityLayout, MAX_LINES_PER_SIDE, RiverConfig, Roof,
        StreetClass, Zone, street_lines,
    },
    max_fog_visibility,
    meshes::{self, Surface},
    rng::Rng,
};

fn city() -> CityLayout {
    CityLayout::generate(&CityConfig::default())
}

fn footprint(lo: Vec3, hi: Vec3) -> Rect {
    Rect::new(lo.x, lo.z, hi.x, hi.z)
}

#[test]
fn same_seed_same_city_other_seed_other_city() {
    let cfg = CityConfig::default();
    assert_eq!(CityLayout::generate(&cfg), CityLayout::generate(&cfg));
    let other = CityLayout::generate(&CityConfig {
        seed: cfg.seed + 1,
        ..cfg.clone()
    });
    assert_ne!(other.buildings, city().buildings);
}

#[test]
fn grid_has_boulevards_through_the_centre_and_regular_avenues() {
    let layout = city();
    let cfg = &layout.config;
    for lines in &layout.lines {
        assert!(
            lines.windows(2).all(|w| w[0].max() < w[1].min()),
            "corridors overlap"
        );
        let centre = lines
            .iter()
            .find(|l| l.center == 0.0)
            .expect("no centre line");
        assert_eq!(centre.class, StreetClass::Boulevard);
        let avenues = lines
            .iter()
            .filter(|l| l.class == StreetClass::Avenue)
            .count();
        assert!(avenues >= 6, "only {avenues} avenues");
        for l in lines {
            let want = match l.class {
                StreetClass::Street => cfg.street_width,
                StreetClass::Avenue => cfg.avenue_width,
                StreetClass::Boulevard => cfg.boulevard_width,
            };
            assert_eq!(l.width, want);
        }
        for w in lines.windows(2) {
            let block = w[1].min() - w[0].max();
            assert!(
                (cfg.block_min - 1e-3..=cfg.block_max + 1e-3).contains(&block),
                "block of {block} m"
            );
        }
    }
}

#[test]
fn streets_are_free_of_buildings() {
    assert_streets_free(&city());
}

#[test]
fn buildings_do_not_overlap_and_keep_their_gap() {
    assert_no_overlap(&city());
}

#[test]
fn invariants_hold_for_other_seeds() {
    for seed in 1..=4 {
        let layout = CityLayout::generate(&CityConfig { seed, ..default() });
        assert_streets_free(&layout);
        assert_no_overlap(&layout);
        assert!(
            layout.farms.len() >= 4,
            "seed {seed}: {} farms",
            layout.farms.len()
        );
    }
}

fn assert_streets_free(layout: &CityLayout) {
    let streets = layout.street_rects();
    assert!(streets.len() > 500, "only {} street rects", streets.len());
    for b in &layout.buildings {
        for p in &b.parts {
            let fp = footprint(p.min(), p.max());
            // Includes the setback: the gap between facades across a street
            // is at least the street width plus twice the setback.
            let grown = fp.inflate(layout.config.setback - 1e-3);
            for s in &streets {
                assert!(
                    grown.intersect(*s).is_empty(),
                    "{:?} at {:?} reaches into street {s:?}",
                    b.kind,
                    p.center
                );
            }
        }
    }
}

fn assert_no_overlap(layout: &CityLayout) {
    let gap = layout.config.building_gap;
    let rects: Vec<Rect> = layout
        .buildings
        .iter()
        .map(|b| {
            let (lo, hi) = b.bounds();
            footprint(lo, hi)
        })
        .collect();
    // Sort by x so only nearby candidates are compared.
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|&a, &b| rects[a].min.x.total_cmp(&rects[b].min.x));
    for (n, &a) in order.iter().enumerate() {
        for &b in &order[n + 1..] {
            if rects[b].min.x > rects[a].max.x + gap {
                break;
            }
            let near = rects[a]
                .inflate(gap / 2.0 - 1e-3)
                .intersect(rects[b].inflate(gap / 2.0 - 1e-3));
            assert!(
                near.is_empty(),
                "{:?} {:?} and {:?} {:?} closer than {gap} m",
                layout.buildings[a].kind,
                rects[a],
                layout.buildings[b].kind,
                rects[b]
            );
        }
    }
}

#[test]
fn parts_are_stacked_inside_the_base_footprint() {
    for b in &city().buildings {
        let base = b.parts[0];
        assert_eq!(base.min().y, 0.0, "{:?} floats", b.kind);
        let base_fp = footprint(base.min(), base.max());
        let mut top = base.max().y;
        for p in &b.parts[1..] {
            assert!(p.half_size.min_element() > 0.0);
            assert!(
                (p.min().y - top).abs() < 1e-3,
                "{:?} part not on top of the last",
                b.kind
            );
            let fp = footprint(p.min(), p.max());
            assert_eq!(base_fp.union(fp), base_fp, "{:?} part overhangs", b.kind);
            top = p.max().y;
        }
        if let Roof::Gable { height, .. } = base.roof {
            assert!(height > 0.0 && height < base.half_size.y * 2.0);
        }
    }
}

#[test]
fn height_falls_from_centre_to_suburbs() {
    let layout = city();
    let mean = |zone: Zone| {
        let h: Vec<f32> = layout
            .buildings
            .iter()
            .filter(|b| b.zone == zone)
            .map(|b| b.height())
            .collect();
        assert!(!h.is_empty(), "no buildings in {zone:?}");
        h.iter().sum::<f32>() / h.len() as f32
    };
    let (down, mid, sub) = (
        mean(Zone::Downtown),
        mean(Zone::Midtown),
        mean(Zone::Suburb),
    );
    assert!(
        down > 2.0 * mid && mid > 2.0 * sub,
        "{down} / {mid} / {sub}"
    );

    // Also by distance rings, independent of the zone labels.
    let ring = 0.2 * layout.config.city_radius;
    let mut rings = [(0.0_f32, 0_u32); 5];
    for b in layout.buildings.iter().filter(|b| b.zone != Zone::Rural) {
        let (lo, hi) = b.bounds();
        let r = ((lo.xz() + hi.xz()) / 2.0).length();
        let k = ((r / ring) as usize).min(4);
        rings[k].0 += b.height();
        rings[k].1 += 1;
    }
    let means: Vec<f32> = rings.iter().map(|(s, n)| s / *n as f32).collect();
    assert!(
        means.windows(2).all(|w| w[0] > w[1]),
        "ring means {means:?}"
    );
}

#[test]
fn every_zone_and_farm_kind_appears() {
    let layout = city();
    for kind in [
        BuildingKind::Tower,
        BuildingKind::SetbackTower,
        BuildingKind::MidRise,
        BuildingKind::Slab,
        BuildingKind::Apartment,
        BuildingKind::House,
        BuildingKind::Farmhouse,
        BuildingKind::Barn,
        BuildingKind::Silo,
    ] {
        assert!(
            layout.buildings.iter().any(|b| b.kind == kind),
            "no {kind:?}"
        );
    }
    assert_eq!(layout.farms.len(), layout.config.farm_count);
    assert!(layout.blocks.iter().any(|b| b.zone == Zone::Park));
}

#[test]
fn farms_lie_outside_the_city_and_off_the_roads() {
    let layout = city();
    let streets = layout.street_rects();
    for farm in &layout.farms {
        let r = farm.rect();
        assert!(
            streets.iter().all(|s| r.intersect(*s).is_empty()),
            "farm {r:?} on a road"
        );
        for w in layout.river_rects() {
            assert!(r.intersect(w).is_empty(), "farm {r:?} in the river");
        }
        for b in layout.blocks.iter().filter(|b| b.zone.is_urban()) {
            assert!(
                r.intersect(b.rect).is_empty(),
                "farm {r:?} in a {:?} block",
                b.zone
            );
        }
    }
    for (i, a) in layout.farms.iter().enumerate() {
        for b in &layout.farms[i + 1..] {
            assert!(a.rect().intersect(b.rect()).is_empty());
        }
    }
}

#[test]
fn ground_is_one_flat_layer_without_gaps_or_overlaps() {
    let layout = city();
    let quads = meshes::ground_quads(&layout);
    let edge = 2.0 * layout.config.ground_half_extent;
    // f64: summing many small areas in f32 loses too much precision.
    let area: f64 = quads
        .iter()
        .map(|q| f64::from(q.rect.width()) * f64::from(q.rect.height()))
        .sum();
    let rel = (area - f64::from(edge) * f64::from(edge)).abs() / f64::from(edge * edge);
    assert!(rel < 1e-6, "ground area off by {rel}");

    // No two quads share any area. With the exact area sum above this makes
    // the quads a true partition. Quads are bucketed on a coarse grid so only
    // neighbours are compared.
    const BUCKET: f32 = 500.0;
    let mut buckets: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
    for (n, q) in quads.iter().enumerate() {
        let lo = (q.rect.min / BUCKET).floor().as_ivec2();
        let hi = ((q.rect.max / BUCKET).ceil() - 1.0).as_ivec2();
        for bx in lo.x..=hi.x {
            for bz in lo.y..=hi.y {
                buckets.entry((bx, bz)).or_default().push(n);
            }
        }
    }
    for members in buckets.values() {
        for (k, &a) in members.iter().enumerate() {
            for &b in &members[k + 1..] {
                assert!(
                    quads[a].rect.intersect(quads[b].rect).is_empty(),
                    "ground quads overlap: {:?} and {:?}",
                    quads[a].rect,
                    quads[b].rect
                );
            }
        }
    }
    assert!(
        quads
            .iter()
            .all(|q| q.rect.width() > 0.0 && q.rect.height() > 0.0)
    );

    let data = meshes::ground_mesh_data(&quads);
    assert!(
        data.positions.iter().all(|p| p[1] == 0.0),
        "ground not flat"
    );
    for t in data.indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(data.positions[t[k] as usize]));
        assert!((b - a).cross(c - a).y > 0.0, "ground triangle faces down");
    }
}

#[test]
fn every_paved_rect_shows_asphalt_lanes() {
    let layout = city();
    let quads = meshes::ground_quads(&layout);
    let color_at = |p: Vec2| {
        quads
            .iter()
            .find(|q| q.rect.contains(p))
            .unwrap_or_else(|| panic!("no ground at {p}"))
            .color
    };
    // Middle of the lane band right of the centre line / median.
    let lane = |axis: usize, i: usize| {
        let l = layout.lines[axis][i];
        let inner = l.class.median_width() / 2.0;
        let outer = l.width / 2.0 - layout.config.sidewalk_width;
        l.center + (inner + outer) / 2.0
    };
    let mut checked = 0;
    for (axis, i, rect) in layout.street_segments() {
        let along = if axis == 0 {
            rect.center().y
        } else {
            rect.center().x
        };
        let p = if axis == 0 {
            Vec2::new(lane(0, i), along)
        } else {
            Vec2::new(along, lane(1, i))
        };
        checked += 1;
        if layout.on_river(p) {
            // Paved across the river: the street runs on a bridge above.
            let deck = |b: &Bridge| {
                let d = b.parts[0];
                Rect::new(d.min().x, d.min().z, d.max().x, d.max().z).contains(p)
            };
            assert!(layout.bridges.iter().any(deck), "no bridge at {p}");
            continue;
        }
        assert_eq!(color_at(p), meshes::ASPHALT, "{axis}/{i} segment at {p}");
    }
    for i in 0..layout.lines[0].len() {
        for k in 0..layout.lines[1].len() {
            if layout.crossing_active(0, i, k) || layout.crossing_active(1, k, i) {
                let p = Vec2::new(lane(0, i), lane(1, k));
                assert_eq!(color_at(p), meshes::ASPHALT, "crossing {i}/{k} at {p}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, layout.street_rects().len());
}

#[test]
fn building_faces_point_outwards() {
    let layout = city();
    for b in layout.buildings.iter().step_by(5) {
        for part in &b.parts {
            let mut single = CityLayout {
                buildings: Vec::new(),
                bridges: Vec::new(),
                ..layout.clone()
            };
            single.buildings.push(super::Building {
                parts: vec![*part],
                ..b.clone()
            });
            for data in meshes::building_meshes(&single).values() {
                for t in data.indices.chunks(3) {
                    let [p0, p1, p2] = [0, 1, 2].map(|k| Vec3::from(data.positions[t[k] as usize]));
                    let n = (p1 - p0).cross(p2 - p0);
                    let out = (p0 + p1 + p2) / 3.0 - part.center;
                    assert!(n.dot(out) > 0.0, "{:?} has an inward face", b.kind);
                    let stored = Vec3::from(data.normals[t[0] as usize]);
                    assert!(
                        stored.dot(n.normalize()) > 0.999,
                        "normal does not match winding"
                    );
                }
            }
        }
    }
}

#[test]
fn facades_use_whole_window_bays() {
    // Footprints are multiples of the bay, so the window raster ends at the
    // edges; office storeys are multiples of the office floor height.
    for b in &city().buildings {
        for p in &b.parts {
            let size = p.half_size * 2.0;
            for s in [size.x, size.z] {
                assert!(
                    (s / 3.0 - (s / 3.0).round()).abs() < 1e-3,
                    "{:?} width {s}",
                    b.kind
                );
            }
        }
    }
}

#[test]
fn city_plugin_inserts_the_layout_headless() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, CityPlugin));
    app.update();
    let layout = app.world().resource::<CityLayout>();
    assert_eq!(*layout, city());
}

/// Prints the size of the generated city (run with `--nocapture`).
/// Fixed fingerprint of the default city. If a deliberate generator change
/// moves it, update the constant; if it differs between native and WASM,
/// the generator is not platform-independent.
const DEFAULT_CITY_FINGERPRINT: u64 = 0xcea3_7e18_ab72_c89c;

#[test]
fn default_city_matches_golden_fingerprint() {
    let fp = city().fingerprint();
    assert_eq!(fp, DEFAULT_CITY_FINGERPRINT, "fingerprint is {fp:#018x}");
    let other = CityLayout::generate(&CityConfig {
        seed: 1,
        ..default()
    });
    assert_ne!(other.fingerprint(), fp);
}

#[test]
fn invalid_configs_are_rejected_with_a_reason() {
    let base = CityConfig::default();
    assert_eq!(base.validate(), Ok(()));
    let cases: [(&str, CityConfig); 9] = [
        (
            "avenue_every",
            CityConfig {
                avenue_every: 0,
                ..base.clone()
            },
        ),
        (
            "patch_size",
            CityConfig {
                patch_size: 0.0,
                ..base.clone()
            },
        ),
        (
            "patch_size",
            CityConfig {
                patch_size: -500.0,
                ..base.clone()
            },
        ),
        (
            "patch_size",
            CityConfig {
                patch_size: 1.0,
                ..base.clone()
            },
        ),
        (
            "multiple of patch_size",
            CityConfig {
                patch_size: 700.0,
                ..base.clone()
            },
        ),
        (
            "block_min",
            CityConfig {
                block_min: 300.0,
                ..base.clone()
            },
        ),
        (
            "chunk_size",
            CityConfig {
                chunk_size: f32::NAN,
                ..base.clone()
            },
        ),
        (
            "Street width",
            CityConfig {
                street_width: 12.0,
                ..base.clone()
            },
        ),
        (
            "zone radii",
            CityConfig {
                midtown_radius: 0.1,
                ..base.clone()
            },
        ),
    ];
    for (reason, cfg) in cases {
        let err = CityLayout::try_generate(&cfg).expect_err(reason);
        assert!(err.contains(reason), "expected '{reason}' in: {err}");
    }
}

#[test]
#[should_panic(expected = "invalid CityConfig: avenue_every")]
fn generate_panics_with_the_reason() {
    CityLayout::generate(&CityConfig {
        avenue_every: 0,
        ..default()
    });
}

#[test]
fn render_config_is_valid_and_rejects_nonsense() {
    for target in [Target::Native, Target::Web] {
        let cfg = WorldRenderConfig::for_target(target);
        assert_eq!(cfg.validate_for(target), Ok(()), "{target:?}");
        assert!(cfg.shadow_distance <= 1_500.0);
        let bad = [
            WorldRenderConfig {
                shadow_distance: 100.0,
                ..cfg.clone()
            },
            WorldRenderConfig {
                shadow_cascades: 0,
                ..cfg.clone()
            },
            WorldRenderConfig {
                shadow_cascades: 5,
                ..cfg.clone()
            },
        ];
        for b in bad {
            assert!(b.validate_for(target).is_err(), "{target:?}: {b:?}");
        }
    }
    let native = WorldRenderConfig::for_target(Target::Native);
    assert_eq!(native, WorldRenderConfig::default());
    // Chunks must fade out before the day fog (9 km visibility) ends.
    let fade = native.draw_fade.clone().unwrap();
    assert!(fade.end <= 9_000.0);
    assert!(native.chunk_visibility().unwrap().use_aabb);
    assert!(
        WorldRenderConfig {
            draw_fade: Some(8_000.0..7_000.0),
            ..native
        }
        .validate_for(Target::Native)
        .is_err()
    );
}

#[test]
fn web_config_has_no_visibility_range_and_one_cascade() {
    // Crossfading VisibilityRange crashes WebGL2 in Bevy 0.20 (binding 14
    // validation error); one cascade is all WebGL2 renders.
    let web = WorldRenderConfig::for_target(Target::Web);
    assert_eq!(web.draw_fade, None);
    assert!(web.chunk_visibility().is_none());
    assert_eq!(web.shadow_cascades, 1);
    let with_range = WorldRenderConfig {
        draw_fade: Some(7_500.0..8_000.0),
        ..web.clone()
    };
    let err = with_range.validate_for(Target::Web).unwrap_err();
    assert!(err.contains("VisibilityRange"), "{err}");
    assert!(
        WorldRenderConfig {
            shadow_cascades: 3,
            ..web
        }
        .validate_for(Target::Web)
        .is_err()
    );
    // Suggested far plane sits just behind the farthest fog visibility.
    assert!((9_000.0..10_000.0).contains(&WEB_CAMERA_FAR));
}

#[test]
fn max_fog_visibility_covers_every_lighting_preset() {
    let max = max_fog_visibility();
    for look in Lighting::ALL {
        assert!(look.fog_visibility() <= max, "{look:?}");
    }
    assert!(Lighting::ALL.iter().any(|l| l.fog_visibility() == max));
    assert_eq!(max, 9_000.0);
    assert!(WEB_CAMERA_FAR > max);
}

#[test]
fn city_stays_within_render_budget() {
    let layout = city();
    let parts: usize = layout.buildings.iter().map(|b| b.parts.len()).sum();
    let chunks = meshes::building_meshes(&layout);
    let tris: usize = chunks.values().map(|d| d.triangle_count()).sum();
    let verts: usize = chunks.values().map(|d| d.positions.len()).sum();
    let ground = meshes::ground_mesh_data(&meshes::ground_quads(&layout));
    let per_surface = |s: Surface| chunks.keys().filter(|k| k.1 == s).count();
    let zones = [Zone::Downtown, Zone::Midtown, Zone::Suburb, Zone::Rural]
        .map(|z| layout.buildings.iter().filter(|b| b.zone == z).count());
    let tallest = layout
        .buildings
        .iter()
        .map(|b| b.height())
        .fold(0.0, f32::max);
    println!(
        "buildings {} (downtown {}, midtown {}, suburb {}, farm {}), parts {parts}, tallest {tallest} m\n\
         building meshes {} (office {}, residential {}, plain {}), {tris} tris, {verts} verts\n\
         ground {} tris, {} verts",
        layout.buildings.len(),
        zones[0],
        zones[1],
        zones[2],
        zones[3],
        chunks.len(),
        per_surface(Surface::Office),
        per_surface(Surface::Residential),
        per_surface(Surface::Plain),
        ground.triangle_count(),
        ground.positions.len(),
    );

    println!(
        "river cells {}, bridges {}, farms {}",
        layout.river.len(),
        layout.bridges.len(),
        layout.farms.len()
    );
    // Phase 1 measured smooth on Austin's Mac at ~7k buildings / ~136k
    // triangles; the enlarged city (~12k / ~225k) keeps ~10 % headroom.
    assert!(layout.buildings.len() <= 14_000, "too many buildings");
    assert!(chunks.len() <= 224, "too many building meshes");
    assert!(tris <= 190_000, "too many building triangles");
    assert!(verts <= 400_000, "too many building vertices");
    assert!(
        ground.triangle_count() <= 64_000,
        "too many ground triangles"
    );
    assert!(
        ground.positions.len() <= 128_000,
        "too many ground vertices"
    );
}

#[test]
fn river_forks_and_keeps_buildings_out() {
    let layout = city();
    let river = layout.river_rects();
    assert!(!layout.river.is_empty());
    // Tuning-fork shape: one stem reaching the south edge, two arms the north edge.
    let nz = layout.lines[1].len();
    let south: Vec<_> = layout.river.iter().filter(|c| c.1 == nz).collect();
    let north: Vec<_> = layout.river.iter().filter(|c| c.1 == 0).collect();
    assert_eq!((south.len(), north.len()), (1, 2), "{south:?} / {north:?}");
    assert!(north[0].0 < south[0].0 && south[0].0 < north[1].0);

    for b in &layout.buildings {
        for p in &b.parts {
            let fp = footprint(p.min(), p.max());
            for w in &river {
                assert!(
                    fp.intersect(*w).is_empty(),
                    "{:?} at {:?} stands in the river",
                    b.kind,
                    p.center
                );
            }
        }
    }
    assert!(
        layout
            .blocks
            .iter()
            .filter(|b| b.zone == Zone::Water)
            .count()
            > 20
    );
}

#[test]
fn every_paved_street_across_the_river_has_one_bridge() {
    let layout = city();
    let cfg = layout.config.river.clone().unwrap();
    let crossings: Vec<_> = layout.river_crossings().collect();
    let paved: Vec<_> = crossings
        .iter()
        .filter(|&&(a, i, g)| layout.segment_active(a, i, g))
        .collect();
    // Pinned for the default seed: a generator change that adds or drops
    // bridges must show up here.
    assert_eq!(layout.bridges.len(), 72);
    assert_eq!(paved.len(), layout.bridges.len());
    for &&(axis, i, g) in &paved {
        let n = layout
            .bridges
            .iter()
            .filter(|b| (b.axis, b.line, b.gap) == (axis, i, g))
            .count();
        assert_eq!(n, 1, "line {axis}/{i} over gap {g}");
        // A street paved on one bank only ends at the quay instead.
        let line = layout.lines[axis][i];
        assert!(
            line.class == StreetClass::Boulevard
                || (layout.segment_active(axis, i, g - 1) && layout.segment_active(axis, i, g + 1))
        );
    }
    for b in &layout.bridges {
        let deck = b.parts[0];
        assert!(deck.underside);
        assert!(
            deck.min().y >= cfg.bridge_clearance - 1e-3,
            "too low to fly under"
        );
        // The deck spans the whole river gap, water and quays.
        let span = layout.gap_range(1 - b.axis, b.gap);
        let (lo, hi) = if b.axis == 0 {
            (deck.min().z, deck.max().z)
        } else {
            (deck.min().x, deck.max().x)
        };
        assert!((lo - span.0).abs() < 1e-3 && (hi - span.1).abs() < 1e-3);
        // Under the deck: open water between the abutments.
        let under = Vec2::new(deck.center.x, deck.center.z);
        assert!(layout.on_river(under));
        for p in &b.parts {
            assert!(p.half_size.min_element() > 0.0);
            assert!(p.min().y >= -1e-3);
        }
    }
}

#[test]
fn river_has_water_between_quays() {
    let layout = city();
    let quads = meshes::ground_quads(&layout);
    let water: f64 = quads
        .iter()
        .filter(|q| q.is_water())
        .map(|q| f64::from(q.rect.width()) * f64::from(q.rect.height()))
        .sum();
    assert!(water > 5.0e6, "only {water} m² of water");
    let quay = layout.config.river.as_ref().unwrap().quay_width;
    // Across the stem: quay, water, quay.
    let &(gx, gz) = layout.river.iter().max_by_key(|c| c.1).unwrap();
    let (x0, x1) = layout.gap_range(0, gx);
    let z = layout.gap_range(1, gz).0 + 50.0;
    let color_at = |x: f32| {
        quads
            .iter()
            .find(|q| q.rect.contains(Vec2::new(x, z)))
            .unwrap()
            .color
    };
    assert!(color_at(x0 + quay / 2.0) != meshes::WATER);
    assert_eq!(color_at((x0 + x1) / 2.0), meshes::WATER);
    assert!(color_at(x1 - quay / 2.0) != meshes::WATER);
}

#[test]
fn bridge_faces_point_outwards() {
    let layout = city();
    for b in layout.bridges.iter().take(10) {
        for part in &b.parts {
            let single = CityLayout {
                buildings: Vec::new(),
                bridges: vec![Bridge {
                    parts: vec![*part],
                    ..b.clone()
                }],
                ..layout.clone()
            };
            for data in meshes::building_meshes(&single).values() {
                for t in data.indices.chunks(3) {
                    let [p0, p1, p2] = [0, 1, 2].map(|k| Vec3::from(data.positions[t[k] as usize]));
                    let n = (p1 - p0).cross(p2 - p0);
                    assert!(n.dot((p0 + p1 + p2) / 3.0 - part.center) > 0.0);
                }
            }
        }
    }
}

#[test]
fn a_city_without_river_is_valid() {
    let layout = CityLayout::generate(&CityConfig {
        river: None,
        ..default()
    });
    assert!(layout.river.is_empty() && layout.bridges.is_empty());
    assert!(layout.blocks.iter().all(|b| b.zone != Zone::Water));
}

#[test]
fn invalid_river_configs_are_rejected() {
    let river = RiverConfig::default();
    let cases = [
        (
            "leave no water",
            RiverConfig {
                quay_width: 60.0,
                ..river.clone()
            },
        ),
        (
            "west_x < stem_x",
            RiverConfig {
                west_x: 2_000.0,
                ..river.clone()
            },
        ),
        (
            "outside the street grid",
            RiverConfig {
                fork_z: -9_000.0,
                ..river.clone()
            },
        ),
        (
            "bridge_clearance",
            RiverConfig {
                bridge_clearance: 0.0,
                ..river.clone()
            },
        ),
        (
            "one block between",
            RiverConfig {
                stem_x: -1_700.0,
                ..river.clone()
            },
        ),
    ];
    for (reason, river) in cases {
        let cfg = CityConfig {
            river: Some(river),
            ..default()
        };
        let err = CityLayout::try_generate(&cfg).expect_err(reason);
        assert!(err.contains(reason), "expected '{reason}' in: {err}");
    }
}

#[test]
fn street_lines_terminate_even_where_f32_stops_advancing() {
    // Far beyond any valid config: at ~3e8 m a 3 m step no longer changes
    // an f32, so an unbounded loop would spin forever.
    let cfg = CityConfig {
        grid_half_extent: 1.0e12,
        block_min: 1.0,
        block_max: 1.0,
        street_width: 1.0,
        avenue_width: 1.0,
        boulevard_width: 1.0,
        ..default()
    };
    let lines = street_lines(&cfg, &mut Rng::stream(1, 1, 0, 0));
    assert!(lines.len() <= 2 * MAX_LINES_PER_SIDE + 1);
    assert!(lines.windows(2).all(|w| w[0].center < w[1].center));
}

#[test]
fn huge_grids_are_rejected_before_generation() {
    let cfg = CityConfig {
        grid_half_extent: 1.0e9,
        inner_half_extent: 2.0e9,
        ground_half_extent: 2.0e9,
        patch_size: 1.0e7,
        ..default()
    };
    let err = CityLayout::try_generate(&cfg).expect_err("huge grid");
    assert!(err.contains("street lines"), "{err}");
}

#[test]
fn fingerprint_covers_bridge_details() {
    let layout = city();
    let mut changed = layout.clone();
    changed.bridges[0].parts[1].roof_color[0] += 0.01;
    assert_ne!(changed.fingerprint(), layout.fingerprint());
    let mut changed = layout.clone();
    changed.bridges[0].parts[0].underside = false;
    assert_ne!(changed.fingerprint(), layout.fingerprint());
    let mut changed = layout.clone();
    changed.bridges[0].parts.pop();
    assert_ne!(changed.fingerprint(), layout.fingerprint());
}
