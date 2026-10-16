//! Enemy models: the normalised glTF of each enemy type, spawned below
//! the enemy entity (no correction transform, see [`crate::aircraft`]).

use bevy::prelude::*;

use super::Enemy;

pub struct EnemyVisualsPlugin;

impl Plugin for EnemyVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, dress_enemies);
    }
}

fn dress_enemies(
    mut commands: Commands,
    assets: Res<AssetServer>,
    added: Query<(Entity, &Enemy), Added<Enemy>>,
) {
    for (entity, enemy) in &added {
        commands
            .entity(entity)
            .insert((Visibility::default(), enemy.kind.model().scene(&assets)));
    }
}
