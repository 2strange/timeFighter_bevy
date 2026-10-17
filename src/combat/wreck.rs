//! The player's jet is destroyed: gone in the same frame.
//!
//! On [`Destroyed`] for the player (crash, shoot-down, ramming) the jet gets
//! [`Wrecked`] (the flight model stops moving it), is hidden with its
//! flames, loses its collider and a big explosion starts where it died.
//! This runs right after the crash detection in `PostUpdate` and again after
//! hits are resolved in `Update`, so whichever kills the jet, the frame
//! that shows the kill already shows the explosion and no jet.
//! A new round (full [`Health`] again) brings the jet back.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{ColliderDisabled, RigidBodyDisabled};

use super::{Destroyed, Health, Team, effects};
use crate::flight::{PlayerAircraft, Wrecked};

/// Where the player's wreck is handled; effect visuals are dressed after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WreckSystems;

/// Hides and freezes the player's jet and starts the explosion.
pub(super) fn wreck_player(
    mut commands: Commands,
    mut destroyed: MessageReader<Destroyed>,
    player: Query<Has<Wrecked>, With<PlayerAircraft>>,
) {
    for event in destroyed.read().filter(|d| d.team == Team::Player) {
        // Runs twice per frame (see module docs): only the first one acts.
        if player.get(event.entity) != Ok(false) {
            continue;
        }
        commands.entity(event.entity).insert((
            Wrecked,
            Visibility::Hidden,
            ColliderDisabled,
            RigidBodyDisabled,
        ));
        effects::spawn(
            &mut commands,
            effects::EffectKind::BigExplosion,
            event.position,
        );
    }
}

type PlayerWreck = (With<PlayerAircraft>, With<Wrecked>);

/// A fresh round re-arms the jet with full health: show it again.
pub(super) fn restore_player(
    mut commands: Commands,
    player: Query<(Entity, &Health), PlayerWreck>,
) {
    for (jet, health) in &player {
        if !health.is_dead() {
            commands
                .entity(jet)
                .remove::<(Wrecked, ColliderDisabled, RigidBodyDisabled)>()
                .insert(Visibility::Inherited);
        }
    }
}
