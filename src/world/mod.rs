//! The world: a procedural city on a flat ground plane, sun, sky and fog.
//!
//! The city is generated as plain data first ([`CityLayout`], seeded and
//! deterministic) and then rendered from it: the ground is one flat mesh
//! with streets painted in as vertex colours (plus one for the river),
//! buildings and bridges are merged per 1 km chunk and facade material, so the whole city takes a few dozen draw
//! calls and is frustum-culled chunk by chunk. Each building also carries
//! its collision boxes for the physics phase.
//!
//! Why merged chunks instead of GPU instancing: instancing a unit cube
//! scaled per building would stretch the window raster (UVs are per mesh,
//! not per instance), and fixing that needs a custom world-space shader.
//! Merged chunks keep metre-true UVs with the stock `StandardMaterial`, give
//! a similar draw-call count (a few hundred meshes, 3 materials) and cull per chunk.
//! Trade-off: removing a destroyed building means rebuilding its chunk mesh
//! (about 70 buildings, milliseconds).

mod buildings;
pub mod layout;
pub mod meshes;
mod rng;
pub mod textures;

#[cfg(test)]
mod tests;

use bevy::{
    camera::visibility::VisibilityRange,
    light::{CascadeShadowConfigBuilder, NotShadowCaster},
    prelude::*,
};

pub use layout::{Building, CityConfig, CityLayout, Zone};
use meshes::Surface;

/// Generates the [`CityLayout`] resource from [`CityConfig`]. Data only, no
/// rendering; works headless with `MinimalPlugins`.
pub struct CityPlugin;

impl Plugin for CityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CityConfig>()
            .add_systems(PreStartup, generate_city);
    }
}

/// Render-cost knobs of the world, tuned on Austin's frame-rate measurements.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct WorldRenderConfig {
    /// Shadows are drawn up to this distance from the camera (m). Shadow
    /// passes re-draw every caster per cascade, so this is the main lever.
    pub shadow_distance: f32,
    /// Building chunks fade out between these distances from the camera to
    /// the chunk centre (m); just short of where the day fog (9 km
    /// visibility) has swallowed them anyway.
    pub draw_fade: std::ops::Range<f32>,
}

impl Default for WorldRenderConfig {
    fn default() -> Self {
        Self {
            shadow_distance: 1_500.0,
            draw_fade: 7_500.0..8_000.0,
        }
    }
}

impl WorldRenderConfig {
    /// Nearest cascade boundary; must stay below `shadow_distance`.
    const FIRST_CASCADE: f32 = 150.0;

    pub fn validate(&self) -> Result<(), String> {
        if !(self.shadow_distance.is_finite() && self.shadow_distance > Self::FIRST_CASCADE) {
            return Err(format!(
                "shadow_distance must exceed {} m, got {}",
                Self::FIRST_CASCADE,
                self.shadow_distance
            ));
        }
        let (a, b) = (self.draw_fade.start, self.draw_fade.end);
        if !(a.is_finite() && b.is_finite() && 0.0 < a && a <= b) {
            return Err(format!(
                "draw_fade must satisfy 0 < start <= end, got {a}..{b}"
            ));
        }
        Ok(())
    }

    /// Visibility range shared by all chunks (one entry in Bevy's range
    /// table, well within WebGL2's limit of 64).
    pub fn chunk_visibility(&self) -> VisibilityRange {
        VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: self.draw_fade.clone(),
            // Chunk meshes sit at the origin in world coordinates; measure
            // the distance to the chunk's bounds centre instead.
            use_aabb: true,
        }
    }
}

/// City rendering, light, sky and fog on top of [`CityPlugin`].
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let day = LightingPreset::DAY;
        app.add_plugins(CityPlugin)
            .init_resource::<WorldRenderConfig>()
            .insert_resource(ClearColor(day.sky))
            .insert_resource(GlobalAmbientLight {
                color: day.ambient_color,
                brightness: day.ambient_brightness,
                ..default()
            })
            .init_resource::<Lighting>()
            .add_systems(Startup, (spawn_ground, spawn_buildings, spawn_sun))
            .add_systems(
                Update,
                (
                    toggle_lighting,
                    apply_lighting.run_if(resource_changed::<Lighting>),
                )
                    .chain(),
            );
    }
}

fn generate_city(mut commands: Commands, config: Res<CityConfig>) {
    let layout = CityLayout::generate(&config);
    // Compare with the native build: same seed must give the same fingerprint.
    info!(
        "City seed {:#x}: {} buildings, fingerprint {:016x}",
        config.seed,
        layout.buildings.len(),
        layout.fingerprint()
    );
    commands.insert_resource(layout);
}

/// Light and sky look. `L` toggles between them so Austin can compare on a
/// test flight; daylight is the default until the look is decided.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lighting {
    #[default]
    Day,
    Evening,
}

struct LightingPreset {
    sky: Color,
    fog_sun: Color,
    fog_visibility: f32,
    ambient_color: Color,
    ambient_brightness: f32,
    sun_color: Color,
    sun_lux: f32,
    /// Direction the sunlight travels.
    sun_direction: Vec3,
    /// Emissive strength of lit windows.
    window_glow: f32,
}

impl LightingPreset {
    const DAY: Self = Self {
        sky: Color::srgb(0.62, 0.74, 0.86),
        fog_sun: Color::srgba(1.0, 0.92, 0.75, 0.5),
        fog_visibility: 9_000.0,
        ambient_color: Color::srgb(0.75, 0.82, 1.0),
        ambient_brightness: 600.0,
        sun_color: Color::WHITE,
        sun_lux: light_consts::lux::AMBIENT_DAYLIGHT,
        sun_direction: Vec3::new(-0.4, -0.7, -0.6),
        window_glow: 0.0,
    };

    /// Proposal for the target mood: low sun ahead of the spawn heading,
    /// warm haze, lit windows.
    const EVENING: Self = Self {
        sky: Color::srgb(0.86, 0.60, 0.45),
        fog_sun: Color::srgba(1.0, 0.70, 0.40, 0.8),
        fog_visibility: 7_000.0,
        ambient_color: Color::srgb(0.55, 0.60, 0.85),
        ambient_brightness: 350.0,
        sun_color: Color::srgb(1.0, 0.72, 0.48),
        sun_lux: 4_000.0,
        sun_direction: Vec3::new(0.25, -0.22, 0.94),
        window_glow: 6.0,
    };

    fn get(lighting: Lighting) -> &'static Self {
        match lighting {
            Lighting::Day => &Self::DAY,
            Lighting::Evening => &Self::EVENING,
        }
    }

    fn fog(&self) -> DistanceFog {
        DistanceFog {
            color: self.sky,
            directional_light_color: self.fog_sun,
            directional_light_exponent: 24.0,
            falloff: FogFalloff::from_visibility_squared(self.fog_visibility),
        }
    }
}

/// Distance fog matching the sky colour; attached to the camera.
pub fn sky_fog() -> DistanceFog {
    LightingPreset::DAY.fog()
}

#[derive(Component)]
struct Sun;

/// Facade materials, kept to switch the window glow.
#[derive(Resource)]
struct FacadeMaterials(Vec<Handle<StandardMaterial>>);

fn spawn_sun(mut commands: Commands, render: Res<WorldRenderConfig>) {
    render
        .validate()
        .unwrap_or_else(|e| panic!("invalid WorldRenderConfig: {e}"));
    let day = &LightingPreset::DAY;
    commands.spawn((
        Name::new("Sun"),
        Sun,
        DirectionalLight {
            illuminance: day.sun_lux,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(day.sun_direction, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: WorldRenderConfig::FIRST_CASCADE,
            maximum_distance: render.shadow_distance,
            ..default()
        }
        .build(),
    ));
}

fn spawn_ground(
    mut commands: Commands,
    layout: Res<CityLayout>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (water, land): (Vec<_>, Vec<_>) = meshes::ground_quads(&layout)
        .into_iter()
        .partition(meshes::GroundQuad::is_water);
    // Land and water are disjoint quads in the same plane: two materials,
    // still no overlap.
    let surfaces = [("Ground", land, 0.95, 0.5), ("River", water, 0.12, 0.6)];
    for (name, quads, roughness, reflectance) in surfaces {
        if quads.is_empty() {
            continue;
        }
        commands.spawn((
            Name::new(name),
            Mesh3d(meshes.add(meshes::ground_mesh_data(&quads).into_mesh())),
            MeshMaterial3d(materials.add(StandardMaterial {
                perceptual_roughness: roughness,
                reflectance,
                ..default()
            })),
            // Flat ground never shadows anything.
            NotShadowCaster,
        ));
    }
}

fn spawn_buildings(
    mut commands: Commands,
    layout: Res<CityLayout>,
    render: Res<WorldRenderConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let visibility = render.chunk_visibility();
    let mut facade = |surface: Surface| {
        let (base, glow) = textures::facade_textures(surface);
        materials.add(StandardMaterial {
            base_color_texture: Some(images.add(base)),
            emissive_texture: Some(images.add(glow)),
            emissive: LinearRgba::BLACK,
            perceptual_roughness: if surface == Surface::Office {
                0.35
            } else {
                0.85
            },
            ..default()
        })
    };
    let office = facade(Surface::Office);
    let residential = facade(Surface::Residential);
    let plain = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.insert_resource(FacadeMaterials(vec![office.clone(), residential.clone()]));

    for ((chunk, surface), data) in meshes::building_meshes(&layout) {
        let material = match surface {
            Surface::Office => office.clone(),
            Surface::Residential => residential.clone(),
            Surface::Plain => plain.clone(),
        };
        commands.spawn((
            Name::new(format!("City chunk {chunk:?} {surface:?}")),
            Mesh3d(meshes.add(data.into_mesh())),
            MeshMaterial3d(material),
            visibility.clone(),
        ));
    }
}

fn toggle_lighting(keys: Res<ButtonInput<KeyCode>>, mut lighting: ResMut<Lighting>) {
    if keys.just_pressed(KeyCode::KeyL) {
        *lighting = match *lighting {
            Lighting::Day => Lighting::Evening,
            Lighting::Evening => Lighting::Day,
        };
    }
}

fn apply_lighting(
    lighting: Res<Lighting>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut fog: Query<&mut DistanceFog>,
    facades: Option<Res<FacadeMaterials>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let preset = LightingPreset::get(*lighting);
    clear.0 = preset.sky;
    ambient.color = preset.ambient_color;
    ambient.brightness = preset.ambient_brightness;
    for (mut light, mut transform) in &mut sun {
        light.color = preset.sun_color;
        light.illuminance = preset.sun_lux;
        *transform = Transform::default().looking_to(preset.sun_direction, Vec3::Y);
    }
    for mut f in &mut fog {
        *f = preset.fog();
    }
    for handle in facades.iter().flat_map(|f| f.0.iter()) {
        if let Some(mut material) = materials.get_mut(handle) {
            material.emissive = LinearRgba::WHITE * preset.window_glow;
        }
    }
}
