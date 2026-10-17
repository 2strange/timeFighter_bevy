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
use bevy_rapier3d::{
    prelude::{ColliderDisabled, RapierRigidBodyHandle, RigidBodyDisabled, WriteRapierContext},
    rapier::math::Pose,
};

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

/// A fresh round re-arms the jet with full health (and puts it at the
/// spawn point): show it again.
///
/// The physics body is moved to the spawn point right here, before its
/// collider is switched back on. Otherwise Rapier would still hold it at the
/// wreck's pose, inside the ground or building it hit: a kinematic body only
/// follows its transform with the next simulated step, and while the result
/// screen pauses the game there is none. Re-enabled there, the collider would
/// touch the obstacle again and the new round would start with a second crash.
pub(super) fn restore_player(
    mut commands: Commands,
    mut rapier: WriteRapierContext,
    player: Query<(Entity, &Health, &Transform, Option<&RapierRigidBodyHandle>), PlayerWreck>,
) {
    for (jet, health, transform, handle) in &player {
        if health.is_dead() {
            continue;
        }
        if let (Ok(mut context), Some(handle)) = (rapier.single_mut(), handle)
            && let Some(body) = context.rigidbody_set.bodies.get_mut(handle.0)
        {
            let pose = Pose::from_parts(transform.translation, transform.rotation);
            body.set_position(pose, true);
            body.set_next_kinematic_position(pose);
        }
        commands
            .entity(jet)
            .remove::<(Wrecked, ColliderDisabled, RigidBodyDisabled)>()
            .insert(Visibility::Inherited);
    }
}
