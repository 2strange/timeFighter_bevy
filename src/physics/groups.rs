//! Collision layers and who collides with whom.
//!
//! | layer             | Player | Enemy | PlayerProjectile | EnemyProjectile | World |
//! |-------------------|:------:|:-----:|:----------------:|:---------------:|:-----:|
//! | Player            |        |   x   |                  |        x        |   x   |
//! | Enemy             |   x    |       |        x         |                 |   x   |
//! | PlayerProjectile  |        |   x   |                  |                 |   x   |
//! | EnemyProjectile   |   x    |       |                  |                 |   x   |
//! | World             |   x    |   x   |        x         |        x        |       |
//!
//! Own projectiles never hit their shooter's side, projectiles never hit
//! each other, enemies do not collide with each other, and the static world
//! does not collide with itself. Rapier only reports a pair when each side's
//! filter contains the other's layer; [`Layer::filter`] is symmetric.

use bevy_rapier3d::prelude::{CollisionGroups, Group};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Player,
    Enemy,
    PlayerProjectile,
    EnemyProjectile,
    World,
}

impl Layer {
    pub const ALL: [Layer; 5] = [
        Layer::Player,
        Layer::Enemy,
        Layer::PlayerProjectile,
        Layer::EnemyProjectile,
        Layer::World,
    ];

    /// The Rapier group bit of this layer.
    pub const fn group(self) -> Group {
        match self {
            Layer::Player => Group::GROUP_1,
            Layer::Enemy => Group::GROUP_2,
            Layer::PlayerProjectile => Group::GROUP_3,
            Layer::EnemyProjectile => Group::GROUP_4,
            Layer::World => Group::GROUP_5,
        }
    }

    /// Layers this layer collides with (the table above).
    pub const fn filter(self) -> Group {
        match self {
            Layer::Player => Group::GROUP_2.union(Group::GROUP_4).union(Group::GROUP_5),
            Layer::Enemy => Group::GROUP_1.union(Group::GROUP_3).union(Group::GROUP_5),
            Layer::PlayerProjectile => Group::GROUP_2.union(Group::GROUP_5),
            Layer::EnemyProjectile => Group::GROUP_1.union(Group::GROUP_5),
            Layer::World => Group::GROUP_1
                .union(Group::GROUP_2)
                .union(Group::GROUP_3)
                .union(Group::GROUP_4),
        }
    }

    /// Component for a collider on this layer.
    pub const fn groups(self) -> CollisionGroups {
        CollisionGroups::new(self.group(), self.filter())
    }

    /// Whether colliders on these two layers interact.
    pub fn collides_with(self, other: Layer) -> bool {
        self.filter().contains(other.group()) && other.filter().contains(self.group())
    }
}
