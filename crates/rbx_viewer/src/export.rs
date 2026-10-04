//! Instances as plain triangle meshes, written out as Wavefront `.obj` or
//! glTF 2.0 for another 3D tool to open.
//!
//! Every part is exported as the geometry the viewport draws it with — the
//! same `scene::shape::resolve` precedence and unit meshes for the procedural
//! solids, the same fitted `MeshId` triangles for a `MeshPart` or a
//! `SpecialMesh` FileMesh (see [`crate::pick`], which resolves a click the same
//! way) — baked into world space, one mesh per part. A legacy
//! `UnionOperation` exports the boolean `scene::union` carved from its
//! original parts (never Roblox's own baked `MeshData`). A file mesh or union
//! that has not downloaded, or whose boolean failed, exports as the box it is
//! drawn as.
//!
//! A mesh drawn with an image (`MeshPart.TextureID`, a FileMesh's
//! `TextureId`) carries that image, PNG-encoded, and its UVs, for the writer
//! to put beside the triangles; a `MeshPart` wearing a `SurfaceAppearance`
//! carries its four maps instead, with its tint and alpha mode folded in the
//! way the viewport shades them (see [`wear`]). Images are pooled in [`Export::textures`]:
//! one the whole place shares is encoded and written once, and every part
//! wearing it names the same entry.
//!
//! Units are studs, unscaled, in Roblox's own right-handed Y-up frame, which
//! is also glTF's.

mod gltf;
mod obj;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::pick::{Meshes, Surface};
use crate::scene::{
    cframe_matrix, descendants_of, file_mesh_fit, is_drawable, linear_to_srgb, resolve_shape,
    srgb_to_linear, union_fit, unit_mesh, AlphaMode, FALLBACK_COLOR,
};

pub use gltf::gltf;
pub use obj::{mtl, obj, obj_files};

/// Everything an export writes: each part's surface, and the PNGs their
/// materials point into by index, each image once however many parts wear it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Export {
    pub meshes: Vec<ExportMesh>,
    pub textures: Vec<Vec<u8>>,
}

/// One part's surface in world space, ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Linear RGB, and `1 - Transparency` as alpha. Multiplies the colour
    /// map, as the viewport does.
    pub color: [f32; 4],
    /// One per position, top-left origin as Roblox and glTF have it; empty
    /// for a procedural solid, which has no image to map.
    pub uvs: Vec<[f32; 2]>,
    pub maps: Maps,
    /// Whether the surface is see-through anywhere: its `Transparency`, or
    /// a colour map whose alpha the viewport blends with.
    pub blend: bool,
}

/// The images a part is drawn with, each an index into [`Export::textures`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Maps {
    pub color: Option<usize>,
    /// Tangent space, green up the image, as glTF and Blender read one.
    pub normal: Option<usize>,
    /// Greyscale in red, as `SurfaceAppearance` authors them; for OBJ's
    /// `map_Pm`/`map_Pr`.
    pub metalness: Option<usize>,
    pub roughness: Option<usize>,
    /// The two packed the way glTF's `metallicRoughnessTexture` wants them:
    /// roughness in green, metalness in blue.
    pub metallic_roughness: Option<usize>,
}

/// Every drawable part in `roots`' subtrees, each once even where one root
/// sits under another, depth first. Unlike
/// [`crate::pick::parts_of`], a part's child parts come along: an export is of
/// the subtree, not of what one click would move.
pub fn meshes_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    roots: &[Ref],
) -> Export {
    let mut seen = HashSet::new();
    let mut textures = Textures::default();
    let meshes = roots
        .iter()
        .flat_map(|&root| descendants_of(dom, root))
        .filter(|&referent| seen.insert(referent) && is_drawable(dom, database, referent))
        .filter_map(|referent| export_part(dom, database, meshes, &mut textures, referent))
        .collect();
    Export {
        meshes,
        textures: textures.pngs,
    }
}

/// The PNGs encoded so far, by what they were made from: every part wearing
/// one asset holds the very same `Arc` (see [`crate::pick::Meshes`]), so its
/// address is the asset's identity here.
#[derive(Default)]
struct Textures {
    pngs: Vec<Vec<u8>>,
    by_key: HashMap<Key, Option<usize>>,
}

#[derive(PartialEq, Eq, Hash)]
enum Key {
    Image(*const Image),
    /// A colour map baked over a part colour (see [`overlay`]), by the
    /// colour's bits.
    Overlay(*const Image, [u32; 3]),
    /// Metalness and roughness packed (see [`pack`]), null for a map the set
    /// does not carry.
    Packed(*const Image, *const Image),
}

impl Textures {
    fn add(&mut self, key: Key, make: impl FnOnce() -> Option<Vec<u8>>) -> Option<usize> {
        *self.by_key.entry(key).or_insert_with(|| {
            self.pngs.push(make()?);
            Some(self.pngs.len() - 1)
        })
    }

    fn of(&mut self, image: &Arc<Image>) -> Option<usize> {
        self.add(Key::Image(Arc::as_ptr(image)), || png(image))
    }
}

fn export_part(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    textures: &mut Textures,
    referent: Ref,
) -> Option<ExportMesh> {
    let instance = dom.get(referent)?;
    let properties = instance.properties();
    let file_mesh = file_mesh_fit(dom, database, referent).and_then(|(asset, fit)| {
        let mesh = meshes.get(&asset)?;
        Some((mesh, fit.transform(mesh)))
    });
    let union = || {
        let (asset, model) = union_fit(dom, database, referent)?;
        Some((meshes.get(&asset)?, model))
    };
    let mut uvs = Vec::new();
    let (mut positions, mut normals, mut indices, model) = match file_mesh.or_else(union) {
        Some((mesh, model)) => {
            uvs = mesh.vertices.iter().map(|v| v.uv).collect();
            (
                mesh.vertices.iter().map(|v| v.position).collect(),
                mesh.vertices.iter().map(|v| v.normal).collect(),
                mesh.lod0().to_vec(),
                model,
            )
        }
        None => {
            let (Variant::CFrame(cframe), Variant::Vector3(size)) =
                (properties.get("CFrame")?, properties.get("size")?)
            else {
                return None;
            };
            let size = Vec3::new(size.x, size.y, size.z);
            let geometry = resolve_shape(dom, database, instance, size);
            let unit = unit_mesh(geometry.kind);
            (
                unit.positions,
                unit.normals,
                unit.indices,
                geometry.model(cframe_matrix(cframe)),
            )
        }
    };
    if indices.is_empty() {
        return None;
    }
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => FALLBACK_COLOR,
    }
    .map(|channel| srgb_to_linear(f32::from(channel) / 255.0));
    let alpha = match properties.get("Transparency") {
        Some(&Variant::Float32(t)) => 1.0 - t.clamp(0.0, 1.0),
        _ => 1.0,
    };
    let surface = meshes.surface(referent).filter(|_| !uvs.is_empty());
    let image = meshes.texture(referent).filter(|_| !uvs.is_empty());
    // ponytail: a textured mesh whose `Material` has a pack too is drawn with
    // both, the image in its own UVs and the pack projected; one UV set per
    // part holds one of them, so it exports with its image alone.
    let pack = meshes
        .material(referent)
        .filter(|_| surface.is_none() && image.is_none());
    if let Some(pack) = pack {
        (positions, normals, indices, uvs) =
            project(&positions, &normals, &indices, model, pack.studs_per_tile);
    }
    let mut mesh = ExportMesh {
        name: instance.name().to_owned(),
        positions: transform_points(model, positions),
        normals: transform_normals(model, normals),
        indices,
        color: [color[0], color[1], color[2], alpha],
        uvs,
        maps: Maps::default(),
        blend: alpha < 1.0,
    };
    if let Some(surface) = surface {
        wear(&mut mesh, surface, textures);
    } else if let Some(image) = image {
        mesh.maps.color = textures.of(image);
        // The viewport multiplies the image's alpha into the part's.
        mesh.blend |= image.has_alpha();
    } else if let Some(pack) = pack {
        // The part's colour times the colour map, as `sample_axis` shades it.
        let [color, normal, metalness, roughness] = &pack.maps;
        mesh.maps.color = color.as_ref().and_then(|map| textures.of(map));
        data_maps(&mut mesh, [normal, metalness, roughness], textures);
    }
    Some(mesh)
}

/// The mesh with a material pack's UVs laid on the way
/// `renderer/material.wgsl`'s `sample_axis` projects one: each triangle
/// along the object axis its normal leans on most, in studs along the part
/// (`model`'s scale), one tile every `studs_per_tile`.
///
/// Unshared, three vertices a triangle, so two faces meeting at an edge can
/// each have their own projection. The shader blends three projections
/// across a facet tilted well off every axis; one per triangle is the
/// nearest a single UV set gets, exact on every box face.
#[allow(clippy::type_complexity)]
fn project(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    indices: &[u32],
    model: Mat4,
    studs_per_tile: f32,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>, Vec<[f32; 2]>) {
    let extent = Vec3::new(
        model.x_axis.truncate().length(),
        model.y_axis.truncate().length(),
        model.z_axis.truncate().length(),
    );
    let tile = studs_per_tile.max(0.001);
    let (mut out_positions, mut out_normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    for triangle in indices.as_chunks::<3>().0 {
        let corners = triangle.map(|i| i as usize);
        let normal: Vec3 = corners.iter().map(|&i| Vec3::from(normals[i])).sum();
        let (u, v) = face_frame(normal);
        for i in corners {
            let studs = Vec3::from(positions[i]) * extent;
            out_positions.push(positions[i]);
            out_normals.push(normals[i]);
            uvs.push([studs.dot(u) / tile, studs.dot(v) / tile]);
        }
    }
    let indices = (0..out_positions.len() as u32).collect();
    (out_positions, out_normals, indices, uvs)
}

/// `face_frame(dominant_axis(normal))` from `renderer/material.wgsl` and
/// `lighting.wgsl`: the object axes a face's texture runs along, image right
/// then image down. Ties go to Y, then X, as there.
fn face_frame(normal: Vec3) -> (Vec3, Vec3) {
    let sign = |c: f32| if c >= 0.0 { 1.0 } else { -1.0 };
    let a = normal.abs();
    if a.y >= a.x && a.y >= a.z {
        (Vec3::X, Vec3::new(0.0, 0.0, sign(normal.y)))
    } else if a.x >= a.z {
        (Vec3::new(0.0, 0.0, -sign(normal.x)), Vec3::NEG_Y)
    } else {
        (Vec3::new(sign(normal.z), 0.0, 0.0), Vec3::NEG_Y)
    }
}

/// Dresses `mesh` in a `SurfaceAppearance` the way `renderer/appearance.wgsl`
/// shades one: the colour map tinted by `Color`, its alpha either blended
/// (`Transparency`) or revealing the part's own colour (`Overlay`), and the
/// other three maps as they are.
///
/// Neither format can say "mix the part colour in by the map's alpha", so an
/// `Overlay` map that has any is baked over the part's colour into an opaque
/// image of its own. Without a colour map the renderer's neutral one is
/// clear, which leaves the part colour, tinted.
fn wear(mesh: &mut ExportMesh, surface: &Surface, textures: &mut Textures) {
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
fn data_maps(
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
fn pack(metalness: Option<&Image>, roughness: Option<&Image>) -> Image {
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

fn png(image: &Image) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&image.pixels).ok()?;
    writer.finish().ok()?;
    Some(bytes)
}

fn transform_points(model: Mat4, points: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    points
        .into_iter()
        .map(|p| model.transform_point3(Vec3::from(p)).to_array())
        .collect()
}

/// Through the inverse transpose, so a stretched part's normals stay
/// perpendicular to its faces.
fn transform_normals(model: Mat4, normals: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let normal_matrix = model.inverse().transpose();
    normals
        .into_iter()
        .map(|n| {
            normal_matrix
                .transform_vector3(Vec3::from(n))
                .normalize_or(Vec3::Y)
                .to_array()
        })
        .collect()
}

#[cfg(test)]
mod tests;
