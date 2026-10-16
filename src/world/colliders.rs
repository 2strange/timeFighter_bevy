//! Static Rapier colliders for the city, built from the [`CityLayout`].
//!
//! - One fixed body per building and per bridge, carrying its index into
//!   the layout ([`CityBuilding`], [`CityBridge`]), so a hit names the
//!   building directly. Single boxes stay plain cuboids; stacked towers and
//!   bridges (deck, ramp staircases, parapets) become compounds.
//! - Ground: the land as boxes from the water level up to the streets (so
//!   the quay walls are solid) plus a half-space at the water level. Both
//!   carry [`Ground`]; flying into the river is a crash like any other.
//!
//! Everything is on [`Layer::World`] and has no `ActiveEvents`: contact
//! events come from the moving bodies. Static colliders cost nothing per
//! frame; only the moving bodies query them.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use super::{CityLayout, meshes};
use crate::physics::{Ground, Layer};

/// Index of the building in [`CityLayout::buildings`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityBuilding(pub usize);

/// Index of the bridge in [`CityLayout::bridges`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityBridge(pub usize);

/// Thickness of the land boxes when there is no river to reach down to.
const DRY_GROUND_THICKNESS: f32 = 20.0;

/// Spawns the city's colliders at startup (the layout exists from
/// `PreStartup` on). Needs no renderer.
pub struct CityCollidersPlugin;

impl Plugin for CityCollidersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_colliders);
    }
}

/// A fixed body at the centre of `boxes` with one cuboid or a compound.
fn static_body(boxes: &[(Vec3, Vec3)]) -> (RigidBody, Collider, Transform, CollisionGroups) {
    let (lo, hi) = boxes.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), (c, h)| (lo.min(*c - *h), hi.max(*c + *h)),
    );
    let origin = (lo + hi) / 2.0;
    let collider = match boxes {
        [(c, h)] if (*c - origin).length() < 1e-3 => Collider::cuboid(h.x, h.y, h.z),
        _ => Collider::compound(
            boxes
                .iter()
                .map(|(c, h)| (*c - origin, Quat::IDENTITY, Collider::cuboid(h.x, h.y, h.z)))
                .collect(),
        ),
    };
    (
        RigidBody::Fixed,
        collider,
        Transform::from_translation(origin),
        Layer::World.groups(),
    )
}

fn spawn_colliders(mut commands: Commands, layout: Res<CityLayout>) {
    for (i, building) in layout.buildings.iter().enumerate() {
        let boxes: Vec<_> = building.collision_boxes().collect();
        commands.spawn((
            Name::new("Building collider"),
            CityBuilding(i),
            static_body(&boxes),
        ));
    }
    for (i, bridge) in layout.bridges.iter().enumerate() {
        let boxes: Vec<_> = bridge.collision_boxes().collect();
        commands.spawn((
            Name::new("Bridge collider"),
            CityBridge(i),
            static_body(&boxes),
        ));
    }

    let depth = layout
        .config
        .river
        .as_ref()
        .map_or(DRY_GROUND_THICKNESS, |r| r.water_depth);
    let quads = meshes::ground_quads(&layout);
    for rect in meshes::land_rects(&layout, &quads) {
        let half = Vec3::new(rect.width() / 2.0, depth / 2.0, rect.height() / 2.0);
        let centre = Vec3::new(rect.center().x, -depth / 2.0, rect.center().y);
        commands.spawn((
            Name::new("Land collider"),
            Ground,
            static_body(&[(centre, half)]),
        ));
    }
    commands.spawn((
        Name::new("Water collider"),
        Ground,
        RigidBody::Fixed,
        Collider::halfspace(Vec3::Y).expect("Y is a unit vector"),
        Transform::from_xyz(0.0, -depth, 0.0),
        Layer::World.groups(),
    ));
}
