//! A `SurfaceAppearance`'s maps as an export carries them, and the image
//! plumbing every map shares.

use std::sync::Arc;

use super::{ExportMesh, Key, Textures};
use crate::assets::Image;
use crate::pick::Surface;
use crate::scene::{linear_to_srgb, srgb_to_linear, AlphaMode};

/// Dresses `mesh` in a `SurfaceAppearance` the way `renderer/appearance.wgsl`
/// shades one: the colour map tinted by `Color`, its alpha either blended
/// (`Transparency`) or revealing the part's own colour (`Overlay`), and the
/// other three maps as they are.
///
/// Neither format can say "mix the part colour in by the map's alpha", so an
/// `Overlay` map that has any is baked over the part's colour into an opaque
/// image of its own. Without a colour map the renderer's neutral one is
/// clear, which leaves the part colour, tinted.
pub(super) fn wear(mesh: &mut ExportMesh, surface: &Surface, textures: &mut Textures) {
    let [color, normal, metalness, roughness] = &surface.maps;
    let [r, g, b, alpha] = mesh.color;
    let [tr, tg, tb] = surface.tint;
    match color {
        None => mesh.color = [r * tr, g * tg, b * tb, alpha],
        Some(map) => {
            mesh.color = [tr, tg, tb, alpha];
            mesh.maps.color = match surface.alpha_mode {
                AlphaMode::Transparency => {
                    mesh.blend |= map.has_alpha();
                    textures.of(map)
                }
                AlphaMode::Overlay if map.has_alpha() => {
                    let part = [r, g, b];
                    textures.add(
                        Key::Overlay(Arc::as_ptr(map), part.map(f32::to_bits)),
                        || png(&overlay(map, part)),
                    )
                }
                AlphaMode::Overlay => textures.of(map),
            };
        }
    }
    data_maps(mesh, [normal, metalness, roughness], textures);
}

/// The three maps that are data rather than colour, as authored, plus the
/// metalness and roughness packed for glTF.
pub(super) fn data_maps(
    mesh: &mut ExportMesh,
    [normal, metalness, roughness]: [&Option<Arc<Image>>; 3],
    textures: &mut Textures,
) {
    mesh.maps.normal = normal.as_ref().and_then(|map| textures.of(map));
    mesh.maps.metalness = metalness.as_ref().and_then(|map| textures.of(map));
    mesh.maps.roughness = roughness.as_ref().and_then(|map| textures.of(map));
    if metalness.is_some() || roughness.is_some() {
        let pointer = |map: &Option<Arc<Image>>| map.as_ref().map_or(std::ptr::null(), Arc::as_ptr);
        mesh.maps.metallic_roughness = textures
            .add(Key::Packed(pointer(metalness), pointer(roughness)), || {
                png(&pack(metalness.as_deref(), roughness.as_deref()))
            });
    }
}

/// `map` laid over `part` (linear) by its own alpha, opaque: what an
/// `Overlay` colour map shows, mixed in linear light as the shader mixes it.
fn overlay(map: &Image, part: [f32; 3]) -> Image {
    let pixels = map
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|texel| {
            let a = f32::from(texel[3]) / 255.0;
            let mix = |channel: usize| {
                let painted = srgb_to_linear(f32::from(texel[channel]) / 255.0);
                let linear = part[channel] * (1.0 - a) + painted * a;
                (linear_to_srgb(linear).clamp(0.0, 1.0) * 255.0).round() as u8
            };
            [mix(0), mix(1), mix(2), u8::MAX]
        })
        .collect();
    Image {
        width: map.width,
        height: map.height,
        pixels,
    }
}

/// glTF's metallic-roughness layout: roughness in green, metalness in blue,
/// each read from its map's red channel, at the larger map's size (the
/// smaller sampled nearest). A missing map is the renderer's neutral for it
/// (see `renderer::filemesh::appearance::neutral`).
pub(super) fn pack(metalness: Option<&Image>, roughness: Option<&Image>) -> Image {
    let (width, height) = [metalness, roughness]
        .into_iter()
        .flatten()
        .fold((1, 1), |(w, h), map| (w.max(map.width), h.max(map.height)));
    let red = |map: Option<&Image>, x: u32, y: u32, neutral: u8| {
        map.map_or(neutral, |map| {
            let (mx, my) = (x * map.width / width, y * map.height / height);
            map.pixels[4 * (my * map.width + mx) as usize]
        })
    };
    let pixels = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                [
                    u8::MAX,
                    red(roughness, x, y, 230),
                    red(metalness, x, y, 0),
                    u8::MAX,
                ]
            })
        })
        .collect();
    Image {
        width,
        height,
        pixels,
    }
}

pub(super) fn png(image: &Image) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&image.pixels).ok()?;
    writer.finish().ok()?;
    Some(bytes)
}
