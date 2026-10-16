//! Coarse height map of everything an aircraft can fly into, for the AI's
//! terrain look-ahead. Not a collision model: each cell holds the highest
//! roof that touches it, which is conservative. Nothing counts below street
//! level: the sunken river channel reads as 0 m, so pilots never dive into it.

use bevy::prelude::*;

use crate::world::CityLayout;

/// Highest obstacle per grid cell (m above the ground plane). Outside the
/// grid, and without a city, the ground is flat at 0.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ObstacleMap {
    cell: f32,
    /// World x/z of the grid's minimum corner.
    origin: Vec2,
    nx: usize,
    nz: usize,
    heights: Vec<f32>,
}

impl ObstacleMap {
    /// Cell edge length (m). Smaller than a block, larger than a street.
    pub const CELL: f32 = 50.0;

    /// Rasterises `boxes` (min, max corners) into cells of `cell` metres.
    pub fn from_boxes(cell: f32, boxes: impl IntoIterator<Item = (Vec3, Vec3)>) -> Self {
        let boxes: Vec<(Vec3, Vec3)> = boxes.into_iter().collect();
        let Some((lo, hi)) = boxes
            .iter()
            .map(|(lo, hi)| (lo.xz(), hi.xz()))
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
        else {
            return Self::default();
        };
        let origin = (lo / cell).floor() * cell;
        let nx = ((hi.x - origin.x) / cell).ceil().max(1.0) as usize;
        let nz = ((hi.y - origin.y) / cell).ceil().max(1.0) as usize;
        let mut map = Self {
            cell,
            origin,
            nx,
            nz,
            heights: vec![0.0; nx * nz],
        };
        for (lo, hi) in boxes {
            let (x0, z0) = map.cell_of(lo.x, lo.z);
            let (x1, z1) = map.cell_of(hi.x, hi.z);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let h = &mut map.heights[z * nx + x];
                    *h = h.max(hi.y);
                }
            }
        }
        map
    }

    /// Buildings and bridges of the city.
    pub fn from_layout(layout: &CityLayout) -> Self {
        let buildings = layout.buildings.iter().map(|b| b.bounds());
        let bridges = layout
            .bridges
            .iter()
            .flat_map(|b| b.parts.iter().map(|p| (p.min(), p.max())));
        Self::from_boxes(Self::CELL, buildings.chain(bridges))
    }

    /// Clamped cell index of a world point.
    fn cell_of(&self, x: f32, z: f32) -> (usize, usize) {
        let cx = ((x - self.origin.x) / self.cell).floor();
        let cz = ((z - self.origin.y) / self.cell).floor();
        (
            (cx.max(0.0) as usize).min(self.nx - 1),
            (cz.max(0.0) as usize).min(self.nz - 1),
        )
    }

    /// Highest obstacle at world `x`, `z` (m); 0 over open ground.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        if self.heights.is_empty() {
            return 0.0;
        }
        let cx = ((x - self.origin.x) / self.cell).floor();
        let cz = ((z - self.origin.y) / self.cell).floor();
        if cx < 0.0 || cz < 0.0 || cx >= self.nx as f32 || cz >= self.nz as f32 {
            return 0.0;
        }
        self.heights[cz as usize * self.nx + cx as usize]
    }

    /// Tallest obstacle anywhere.
    pub fn max_height(&self) -> f32 {
        self.heights.iter().copied().fold(0.0, f32::max)
    }
}

pub(super) fn rebuild_obstacles(mut commands: Commands, layout: Res<CityLayout>) {
    commands.insert_resource(ObstacleMap::from_layout(&layout));
}
