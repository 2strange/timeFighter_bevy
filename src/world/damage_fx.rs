//! What damage looks like: rebuilt chunk meshes with ruins, smoke columns
//! over destroyed buildings, dark craters where bombs hit open ground.
//! Everything is a handful of shared low-poly meshes, cheap on WebGL2.

use std::collections::BTreeSet;

use bevy::{light::NotShadowCaster, platform::time::Instant, prelude::*};

use super::{
    CityLayout,
    damage::{BuildingDestroyed, CityDamage, CityDamageSystems, DamageArea, RepairCity},
    meshes::{self, SURFACES, Surface},
};

/// Most smoke columns and craters at once; the oldest go first.
const MAX_SMOKE: usize = 48;
const MAX_CRATERS: usize = 64;
/// Puffs per smoke column, how high it reaches and how fast puffs rise.
const PUFFS: usize = 5;
const SMOKE_HEIGHT: f32 = 70.0;
const SMOKE_RISE: f32 = 6.0;
/// Chunk rebuilds per frame (~2 ms each natively); a big raid spreads its
/// rebuilds over a few frames instead of stalling one.
const REBUILDS_PER_FRAME: usize = 2;
/// Bombs hitting closer to the ground than this leave a crater.
const CRATER_MAX_HEIGHT: f32 = 15.0;

/// One render chunk entity: which chunk and surface its mesh holds.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityChunk {
    pub chunk: (i32, i32),
    pub surface: Surface,
}

#[derive(Component)]
struct SmokeColumn {
    /// Spawn order, to remove the oldest first.
    serial: u64,
}

#[derive(Component)]
struct Puff {
    /// Offset (m) up the column at time 0.
    phase: f32,
    drift: Vec2,
}

#[derive(Component)]
struct Crater {
    serial: u64,
}

#[derive(Resource)]
struct FxAssets {
    puff_mesh: Handle<Mesh>,
    puff_material: Handle<StandardMaterial>,
    crater_mesh: Handle<Mesh>,
    crater_material: Handle<StandardMaterial>,
}

/// Chunks that currently show ruins (rebuilt on repair) and a spawn counter.
#[derive(Resource, Default)]
struct FxState {
    ruined_chunks: BTreeSet<(i32, i32)>,
    /// Chunks waiting for their rebuild.
    pending: BTreeSet<(i32, i32)>,
    serial: u64,
}

pub struct DamageFxPlugin;

impl Plugin for DamageFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FxState>()
            .add_systems(Startup, load_fx_assets)
            .add_systems(
                Update,
                (rebuild_chunks, spawn_smoke, spawn_craters, repair_fx).after(CityDamageSystems),
            )
            .add_systems(Update, rise_smoke);
    }
}

fn load_fx_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(FxAssets {
        // 80 triangles per puff.
        puff_mesh: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
        puff_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.19, 0.19),
            perceptual_roughness: 1.0,
            ..default()
        }),
        // Octagonal disc, 0.8 m thick, sunk 0.2 m: its top is never in the
        // ground plane, so it cannot z-fight.
        crater_mesh: meshes.add(Cylinder::new(1.0, 0.8).mesh().resolution(8)),
        crater_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.11, 0.10, 0.09),
            perceptual_roughness: 1.0,
            ..default()
        }),
    });
}

/// Rebuilds the meshes of every chunk that lost a building this frame.
fn rebuild_chunks(
    layout: Res<CityLayout>,
    damage: Res<CityDamage>,
    mut destroyed: MessageReader<BuildingDestroyed>,
    mut state: ResMut<FxState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(&CityChunk, &Mesh3d, &mut Visibility)>,
) {
    let newly: Vec<_> = destroyed
        .read()
        .map(|d| meshes::chunk_of(&layout, &layout.buildings[d.index].parts))
        .collect();
    state.pending.extend(newly);
    let dirty: BTreeSet<(i32, i32)> = (0..REBUILDS_PER_FRAME)
        .map_while(|_| state.pending.pop_first())
        .collect();
    if dirty.is_empty() {
        return;
    }
    let started = Instant::now();
    rebuild(&layout, &damage.destroyed, &dirty, &mut meshes, &mut chunks);
    debug!(
        "rebuilt {} city chunk(s) in {:.2} ms",
        dirty.len(),
        started.elapsed().as_secs_f64() * 1e3
    );
    state.ruined_chunks.extend(dirty);
}

fn rebuild(
    layout: &CityLayout,
    destroyed: &[bool],
    dirty: &BTreeSet<(i32, i32)>,
    meshes: &mut Assets<Mesh>,
    chunks: &mut Query<(&CityChunk, &Mesh3d, &mut Visibility)>,
) {
    for &chunk in dirty {
        let mut data = meshes::chunk_meshes(layout, destroyed, chunk);
        for (c, handle, mut visibility) in chunks.iter_mut() {
            if c.chunk != chunk {
                continue;
            }
            let i = SURFACES.iter().position(|&s| s == c.surface).unwrap();
            let surface = std::mem::take(&mut data[i]);
            // A surface can run empty (all its buildings in ruins): hide it
            // instead of uploading an empty mesh.
            if surface.is_empty() {
                *visibility = Visibility::Hidden;
            } else {
                *visibility = Visibility::Inherited;
                let _ = meshes.insert(&handle.0, surface.into_mesh());
            }
        }
    }
}

fn spawn_smoke(
    mut commands: Commands,
    assets: Option<Res<FxAssets>>,
    mut destroyed: MessageReader<BuildingDestroyed>,
    mut state: ResMut<FxState>,
    columns: Query<(Entity, &SmokeColumn)>,
) {
    let Some(assets) = assets else { return };
    let mut existing: Vec<(u64, Entity)> = columns.iter().map(|(e, c)| (c.serial, e)).collect();
    for d in destroyed.read() {
        state.serial += 1;
        let serial = state.serial;
        // Slight per-building variation, deterministic by index.
        let k = d.index as f32;
        let drift = Vec2::new((k * 0.37).sin(), (k * 0.61).cos()) * 0.25;
        commands
            .spawn((
                Name::new("Smoke column"),
                SmokeColumn { serial },
                Transform::from_translation(d.position),
                Visibility::default(),
            ))
            .with_children(|column| {
                for p in 0..PUFFS {
                    column.spawn((
                        Puff {
                            phase: SMOKE_HEIGHT * p as f32 / PUFFS as f32,
                            drift,
                        },
                        Mesh3d(assets.puff_mesh.clone()),
                        MeshMaterial3d(assets.puff_material.clone()),
                        Transform::default(),
                        NotShadowCaster,
                    ));
                }
            });
        existing.push((serial, Entity::PLACEHOLDER));
    }
    // Keep the newest columns.
    existing.sort_unstable_by_key(|&(serial, _)| std::cmp::Reverse(serial));
    for &(_, entity) in existing.iter().skip(MAX_SMOKE) {
        if entity != Entity::PLACEHOLDER {
            commands.entity(entity).despawn();
        }
    }
}

/// Puffs rise, grow and drift with the wind, then start again at the bottom.
fn rise_smoke(time: Res<Time>, mut puffs: Query<(&Puff, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (puff, mut transform) in &mut puffs {
        let h = (puff.phase + t * SMOKE_RISE) % SMOKE_HEIGHT;
        let f = h / SMOKE_HEIGHT;
        let radius = 5.0 + 11.0 * f;
        transform.translation = Vec3::new(puff.drift.x * h, h + radius * 0.5, puff.drift.y * h);
        transform.scale = Vec3::splat(radius);
    }
}

fn spawn_craters(
    mut commands: Commands,
    layout: Res<CityLayout>,
    assets: Option<Res<FxAssets>>,
    mut areas: MessageReader<DamageArea>,
    mut state: ResMut<FxState>,
    craters: Query<(Entity, &Crater)>,
) {
    let Some(assets) = assets else { return };
    let mut existing: Vec<(u64, Entity)> = craters.iter().map(|(e, c)| (c.serial, e)).collect();
    for area in areas.read() {
        let p = area.point;
        if !p.is_finite() || p.y > CRATER_MAX_HEIGHT || layout.on_river(p.xz()) {
            continue;
        }
        state.serial += 1;
        let radius = (area.radius * 0.5).clamp(3.0, 12.0);
        commands.spawn((
            Name::new("Crater"),
            Crater {
                serial: state.serial,
            },
            Mesh3d(assets.crater_mesh.clone()),
            MeshMaterial3d(assets.crater_material.clone()),
            Transform::from_xyz(p.x, 0.2, p.z).with_scale(Vec3::new(radius, 1.0, radius)),
            NotShadowCaster,
        ));
        existing.push((state.serial, Entity::PLACEHOLDER));
    }
    existing.sort_unstable_by_key(|&(serial, _)| std::cmp::Reverse(serial));
    for &(_, entity) in existing.iter().skip(MAX_CRATERS) {
        if entity != Entity::PLACEHOLDER {
            commands.entity(entity).despawn();
        }
    }
}

type DamageFx = Or<(With<SmokeColumn>, With<Crater>)>;

/// New round: intact chunks again, no smoke, no craters.
#[allow(clippy::too_many_arguments)]
fn repair_fx(
    mut commands: Commands,
    layout: Res<CityLayout>,
    damage: Res<CityDamage>,
    mut repairs: MessageReader<RepairCity>,
    mut state: ResMut<FxState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: Query<(&CityChunk, &Mesh3d, &mut Visibility)>,
    fx: Query<Entity, DamageFx>,
) {
    if repairs.read().count() == 0 {
        return;
    }
    state.pending.clear();
    let dirty = std::mem::take(&mut state.ruined_chunks);
    rebuild(&layout, &damage.destroyed, &dirty, &mut meshes, &mut chunks);
    for entity in &fx {
        commands.entity(entity).despawn();
    }
}
