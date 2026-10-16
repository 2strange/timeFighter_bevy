//! Procedural facade textures (window rasters), generated at startup.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{layout::Rng, meshes::Surface};

/// Pixels per bay and storey.
const CELL_PX: u32 = 32;
/// Bays and storeys per texture repetition (matches `meshes::facade_tile`).
const CELLS: u32 = 4;

/// Window shape inside one bay x storey cell, as fractions (x0, x1, y0, y1).
fn window(surface: Surface) -> [f32; 4] {
    match surface {
        // Wide glass ribbons with thin mullions.
        Surface::Office => [0.06, 0.94, 0.16, 0.90],
        // Punched windows.
        _ => [0.28, 0.72, 0.28, 0.76],
    }
}

/// Base colour (white wall, tinted per building through vertex colours) and
/// emissive mask (warm light in some windows, used for the evening look).
pub fn facade_textures(surface: Surface) -> (Image, Image) {
    let size = CELL_PX * CELLS;
    let [x0, x1, y0, y1] = window(surface);
    // Office glass reads as blue-grey in daylight, flat windows as dark holes.
    let glass = match surface {
        Surface::Office => [0.09, 0.13, 0.19],
        _ => [0.035, 0.045, 0.065],
    };
    let mut base = Vec::with_capacity((size * size) as usize);
    let mut glow = Vec::with_capacity((size * size) as usize);
    let mut cells = Vec::new();
    let mut rng = Rng::stream(surface as u64, 7, 0, 0);
    for _ in 0..CELLS * CELLS {
        // Glass varies a little per window; about a third are lit at night.
        let shade = rng.range(0.7, 1.15);
        let lit = rng.chance(0.35).then(|| rng.range(0.6, 1.0));
        cells.push((shade, lit));
    }
    for y in 0..size {
        for x in 0..size {
            let (cx, cy) = (x / CELL_PX, y / CELL_PX);
            let fx = (x % CELL_PX) as f32 / CELL_PX as f32;
            let fy = (y % CELL_PX) as f32 / CELL_PX as f32;
            let (shade, lit) = cells[(cy * CELLS + cx) as usize];
            if (x0..x1).contains(&fx) && (y0..y1).contains(&fy) {
                base.push(glass.map(|c| c * shade));
                glow.push(lit.map_or([0.0; 3], |l| [l, 0.62 * l, 0.28 * l]));
            } else {
                base.push([0.8, 0.8, 0.8]);
                glow.push([0.0; 3]);
            }
        }
    }
    (mipmapped(size, base), mipmapped(size, glow))
}

/// Square repeating sRGB texture from linear colours, with a full mip chain
/// (box filter in linear space) so window rasters do not shimmer far away.
pub fn mipmapped(size: u32, base: Vec<[f32; 3]>) -> Image {
    let mut data = Vec::new();
    let mut level = base;
    let mut size_now = size;
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
        if size_now == 1 {
            break;
        }
        let half = size_now / 2;
        let at = |x: u32, y: u32| level[(y * size_now + x) as usize];
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
        size_now = half;
    }

    let mut image = Image::new_uninit(
        Extent3d {
            width: size,
            height: size,
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
