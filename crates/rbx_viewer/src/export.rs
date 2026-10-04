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
//! to put beside the triangles.
//!
//! Units are studs, unscaled, in Roblox's own right-handed Y-up frame, which
//! is also glTF's.

mod gltf;
mod obj;

use std::collections::HashSet;

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::assets::Image;
use crate::pick::Meshes;
use crate::scene::{
    cframe_matrix, descendants_of, file_mesh_fit, is_drawable, resolve_shape, srgb_to_linear,
    union_fit, unit_mesh, FALLBACK_COLOR,
};

pub use gltf::gltf;
pub use obj::{mtl, obj, obj_files};

/// One part's surface in world space, ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Linear RGB, and `1 - Transparency` as alpha. Multiplies `texture`,
    /// as the viewport does.
    pub color: [f32; 4],
    /// One per position, top-left origin as Roblox and glTF have it; empty
    /// for a procedural solid, which has no image to map.
    pub uvs: Vec<[f32; 2]>,
    /// The image the mesh is drawn with, as a PNG file's bytes.
    pub texture: Option<Vec<u8>>,
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
) -> Vec<ExportMesh> {
    let mut seen = HashSet::new();
    roots
        .iter()
        .flat_map(|&root| descendants_of(dom, root))
        .filter(|&referent| seen.insert(referent) && is_drawable(dom, database, referent))
        .filter_map(|referent| export_part(dom, database, meshes, referent))
        .collect()
}

fn export_part(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
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
    let (positions, normals, indices, model) = match file_mesh.or_else(union) {
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
    Some(ExportMesh {
        name: instance.name().to_owned(),
        positions: transform_points(model, positions),
        normals: transform_normals(model, normals),
        indices,
        color: [color[0], color[1], color[2], alpha],
        texture: meshes
            .texture(referent)
            .filter(|_| !uvs.is_empty())
            .and_then(|image| png(image)),
        uvs,
    })
}

// ponytail: encodes once per part, so a texture shared by many parts is
// encoded (and written) once each; dedupe by image if exports get large.
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
