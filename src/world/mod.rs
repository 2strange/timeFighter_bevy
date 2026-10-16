//! Spike world: a large patterned ground plane, scattered pylons for parallax,
//! sun, sky and fog. To be replaced by the procedural city.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::CascadeShadowConfigBuilder,
    math::Affine2,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// Edge length of the ground plane (m).
const GROUND_SIZE: f32 = 40_000.0;
/// Ground covered by one repetition of the ground texture (m).
const GROUND_TILE: f32 = 1_000.0;
/// Pixels per edge of the ground texture.
const GROUND_TEXTURE_PX: u32 = 512;
/// Checker squares per texture repetition (100 m each).
const GROUND_CHECKERS: u32 = 10;

const SKY: Color = Color::srgb(0.62, 0.74, 0.86);

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.75, 0.82, 1.0),
                brightness: 600.0,
                ..default()
            })
            .add_systems(Startup, (spawn_ground, spawn_pylons, spawn_sun));
    }
}

/// Distance fog matching the sky colour; attached to the camera.
pub fn sky_fog() -> DistanceFog {
    DistanceFog {
        color: SKY,
        directional_light_color: Color::srgba(1.0, 0.92, 0.75, 0.5),
        directional_light_exponent: 24.0,
        falloff: FogFalloff::from_visibility_squared(9_000.0),
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: light_consts::lux::AMBIENT_DAYLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.4, -0.7, -0.6), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 150.0,
            maximum_distance: 2_500.0,
            ..default()
        }
        .build(),
    ));
}

fn spawn_ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let texture = images.add(ground_texture());
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(GROUND_SIZE, GROUND_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            perceptual_roughness: 0.95,
            uv_transform: Affine2::from_scale(Vec2::splat(GROUND_SIZE / GROUND_TILE)),
            ..default()
        })),
    ));
}

/// Two-tone green checkerboard with a darker line along the tile border,
/// stored with a full mip chain so it does not shimmer in the distance.
fn ground_texture() -> Image {
    let cell = GROUND_TEXTURE_PX / GROUND_CHECKERS;
    let mut base = Vec::with_capacity((GROUND_TEXTURE_PX * GROUND_TEXTURE_PX) as usize);
    for y in 0..GROUND_TEXTURE_PX {
        for x in 0..GROUND_TEXTURE_PX {
            let rgb = if x < 3 || y < 3 {
                [0.20, 0.30, 0.16]
            } else if (x / cell + y / cell).is_multiple_of(2) {
                [0.36, 0.52, 0.26]
            } else {
                [0.44, 0.58, 0.30]
            };
            base.push(rgb);
        }
    }

    // Box-filter the mip chain in linear space, then store everything as sRGB.
    let mut data = Vec::new();
    let mut level = base;
    let mut size = GROUND_TEXTURE_PX;
    let mut mip_levels = 0;
    loop {
        for rgb in &level {
            let c = Color::linear_rgb(rgb[0], rgb[1], rgb[2]).to_srgba();
            data.extend_from_slice(&[
                (c.red * 255.0).round() as u8,
                (c.green * 255.0).round() as u8,
                (c.blue * 255.0).round() as u8,
                255,
            ]);
        }
        mip_levels += 1;
        if size == 1 {
            break;
        }
        let half = size / 2;
        let at = |x: u32, y: u32| level[(y * size + x) as usize];
        let mut next = Vec::with_capacity((half * half) as usize);
        for y in 0..half {
            for x in 0..half {
                let px = [
                    at(2 * x, 2 * y),
                    at(2 * x + 1, 2 * y),
                    at(2 * x, 2 * y + 1),
                    at(2 * x + 1, 2 * y + 1),
                ];
                next.push(std::array::from_fn(|i| {
                    px.iter().map(|p| p[i]).sum::<f32>() / 4.0
                }));
            }
        }
        level = next;
        size = half;
    }

    let mut image = Image::new_uninit(
        Extent3d {
            width: GROUND_TEXTURE_PX,
            height: GROUND_TEXTURE_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mip_levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

/// A few hundred simple towers spread around the spawn area, so speed and
/// altitude are readable. One shared mesh and material, so they batch.
fn spawn_pylons(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0).mesh());
    let materials = [
        materials.add(Color::srgb(0.78, 0.74, 0.68)),
        materials.add(Color::srgb(0.62, 0.60, 0.58)),
        materials.add(Color::srgb(0.70, 0.52, 0.42)),
    ];

    // Small deterministic LCG; no extra dependency needed for a spike.
    let mut seed: u32 = 0x5eed_1234;
    let mut rand = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };

    for i in 0..400 {
        let x = (rand() - 0.5) * 12_000.0;
        let z = (rand() - 0.5) * 12_000.0;
        let width = 15.0 + rand() * 35.0;
        let depth = 15.0 + rand() * 35.0;
        let height = 20.0 + rand() * rand() * 180.0;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials[i % materials.len()].clone()),
            // Unit cube sized per instance; this is level geometry, not a model.
            Transform::from_xyz(x, height / 2.0, z).with_scale(Vec3::new(width, height, depth)),
        ));
    }
}
