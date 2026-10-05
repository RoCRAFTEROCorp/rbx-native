//! A `MeshPart`'s turn baked into a new mesh. [`plan`] bakes the drawn
//! triangles into a one-mesh glTF; the shell uploads it as a `Model` (the
//! Assets API's only route for geometry not downloaded from Roblox, see
//! `rbx_cloud::create_asset`) and reads back the `MeshId` Roblox's importer
//! gave it; [`apply`] points the part at that mesh.
//!
//! The bake carries the part's `Size` too, not only its turn: a stretched
//! mesh that is then turned cannot be stretched back along the new axes, so
//! the new mesh holds the shape as drawn and its `InitialSize` is read off
//! the uploaded mesh itself — Blender's Apply › Rotation & Scale, Roblox
//! having no scale channel apart from `Size`.

use std::collections::BTreeMap;

use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Content, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::export::{Export, ExportMesh, ExportNode};

use super::{cframe_of, rehang, turned, vector3, vector_of, write_all};
use crate::transform;

const MESH_PART: &str = "MeshPart";

/// How far the uploaded mesh's proportions may stray from the bake's before
/// it is refused. The importer may rescale (glTF is metres, Roblox studs) but
/// a uniform scale leaves proportions alone; anything past rounding means it
/// changed the shape, and repointing the part at it would be a lie.
const PROPORTION_TOLERANCE: f32 = 0.01;

/// Everything [`apply`] writes once the upload is back, worked out up front
/// from the part as it stood when the command ran.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub(crate) target: Ref,
    pub(crate) name: String,
    /// The baked mesh, ready for `rbx_viewer::export::gltf`.
    pub(crate) export: Export,
    /// The part's new `CFrame`: unturned, on the centre of the baked bounds.
    pub(super) frame: CFrameData,
    /// The baked bounds, the part's new `Size`.
    pub(crate) size: Vec3,
    /// Pivot, attachments and joints, re-expressed for the new frame.
    rewrites: Vec<(Ref, &'static str, Variant)>,
}

/// A turned `MeshPart` with a mesh.
pub(super) fn freezable(dom: &WeakDom, database: &ReflectionDatabase, target: Ref) -> bool {
    let Some(instance) = dom.get(target) else {
        return false;
    };
    database.is_subclass_of(instance.class(), MESH_PART)
        && mesh_uri(instance.properties()).is_some()
        && cframe_of(database, instance, "CFrame").is_some_and(|frame| turned(&frame))
}

/// The bake for `target` drawn with `mesh`, or why there is none.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    target: Ref,
    mesh: &rbx_mesh::Mesh,
) -> Result<Plan, String> {
    let instance = dom.get(target).ok_or("the part is gone")?;
    if !freezable(dom, database, target) {
        return Err(format!("{} is not a turned MeshPart", instance.name()));
    }
    if has_bones(dom, target) {
        // A skinned mesh's vertices are bound to its bones' rest poses;
        // turning them alone would tear the skin from the rig.
        return Err(format!(
            "{} is skinned (it has Bones); freezing would break its rig",
            instance.name()
        ));
    }
    let old = cframe_of(database, instance, "CFrame").ok_or("the part has no CFrame")?;
    let size = vector_of(database, instance, "Size").ok_or("the part has no Size")?;
    // Only a stored `InitialSize` counts, as in `rbx_viewer`'s fit: a class
    // default is not the extent of any particular mesh.
    let native = match instance.properties().get("InitialSize") {
        Some(Variant::Vector3(v)) => Some(Vec3::new(v.x, v.y, v.z)),
        _ => None,
    }
    .filter(|v| v.min_element() > f32::EPSILON)
    .unwrap_or_else(|| Vec3::from(mesh.bounds.size()))
    .max(Vec3::splat(f32::EPSILON));
    let stretch = size / native;
    let turn = Mat3::from_mat4(transform::rigid(&old));
    let (vertices, indices) = lod0(mesh);
    let positions: Vec<Vec3> = vertices
        .iter()
        .map(|v| turn * (Vec3::from(v.position) * stretch))
        .collect();
    let (min, max) = positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), &p| (lo.min(p), hi.max(p)),
    );
    if !min.is_finite() {
        return Err("the mesh has no vertices".to_string());
    }
    let centre = (min + max) / 2.0;
    let baked = ExportMesh {
        name: instance.name().to_string(),
        positions: positions.iter().map(|&p| (p - centre).to_array()).collect(),
        // Normals go through the inverse transpose: the stretch is diagonal,
        // so that is dividing by it, then the turn.
        normals: vertices
            .iter()
            .map(|v| {
                (turn * (Vec3::from(v.normal) / stretch))
                    .normalize_or_zero()
                    .to_array()
            })
            .collect(),
        tangents: Vec::new(),
        indices,
        color: [1.0; 4],
        uvs: vertices.iter().map(|v| v.uv).collect(),
        maps: Default::default(),
        blend: false,
        finish: Default::default(),
    };
    let export = Export {
        meshes: vec![baked],
        textures: Vec::new(),
        nodes: vec![ExportNode {
            name: instance.name().to_string(),
            class: MESH_PART.to_string(),
            parent: None,
            placement: None,
            meshes: vec![0],
            material: None,
        }],
    };

    let position = Vec3::new(old.position.x, old.position.y, old.position.z) + centre;
    let frame = transform::cframe(Mat4::from_translation(position));
    Ok(Plan {
        target,
        name: instance.name().to_string(),
        export,
        frame,
        size: max - min,
        rewrites: rehang(dom, database, target, &old, &frame),
    })
}

/// Whether the uploaded mesh, whose own extent is `native`, has the bake's
/// proportions — anything but a uniform rescale is refused.
pub(crate) fn same_shape(baked: Vec3, native: Vec3) -> bool {
    if baked.min_element() <= f32::EPSILON || native.min_element() <= f32::EPSILON {
        // A flat mesh: compare only the axes that have extent.
        let flat = |v: Vec3| v.to_array().map(|c| c <= f32::EPSILON);
        if flat(baked) != flat(native) {
            return false;
        }
    }
    let ratios: Vec<f32> = (0..3)
        .filter(|&axis| baked[axis] > f32::EPSILON)
        .map(|axis| native[axis] / baked[axis])
        .collect();
    let first = ratios.first().copied().unwrap_or(1.0);
    ratios
        .iter()
        .all(|r| ((r - first) / first).abs() <= PROPORTION_TOLERANCE)
}

/// Points the part at `mesh_id` (whose own extent is `native`) and writes
/// every frame [`plan`] worked out. The part keeps the kind of value its
/// `MeshId` already held, and a `MeshContent` naming the old mesh follows.
pub(crate) fn apply(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    plan: &Plan,
    mesh_id: u64,
    native: Vec3,
) -> Result<(), String> {
    let instance = dom
        .get(plan.target)
        .ok_or("the part was deleted meanwhile")?;
    let uri = format!("rbxassetid://{mesh_id}");
    let mut writes: Vec<(Ref, &str, Variant)> = Vec::new();
    for key in ["MeshId", "MeshContent"] {
        match instance.properties().get(key) {
            Some(Variant::String(_)) => {
                writes.push((plan.target, key, Variant::String(uri.clone())))
            }
            Some(Variant::Content(Content::Uri(_))) => writes.push((
                plan.target,
                key,
                Variant::Content(Content::Uri(uri.clone())),
            )),
            _ => {}
        }
    }
    writes.push((plan.target, "Size", vector3(plan.size)));
    writes.push((plan.target, "InitialSize", vector3(native)));
    writes.push((plan.target, "CFrame", Variant::CFrame(plan.frame)));
    writes.extend(plan.rewrites.iter().cloned());
    write_all(dom, database, writes)
}

/// The asset id in a `MeshPart`'s `MeshId` (or `MeshContent`), where it is
/// an uploaded asset rather than one shipped with Studio.
pub(crate) fn mesh_asset_id(properties: &BTreeMap<String, Variant>) -> Option<u64> {
    match rbx_assets::AssetRef::parse(mesh_uri(properties)?).ok()? {
        rbx_assets::AssetRef::Id(id) => Some(id),
        _ => None,
    }
}

/// The URI of the mesh a `MeshPart` draws, empty strings counting as none.
pub(crate) fn mesh_uri(properties: &BTreeMap<String, Variant>) -> Option<&str> {
    ["MeshId", "MeshContent"].iter().find_map(|key| {
        let uri = match properties.get(*key)? {
            Variant::String(text) => text.as_str(),
            Variant::Content(Content::Uri(uri)) => uri.as_str(),
            _ => return None,
        };
        (!uri.is_empty()).then_some(uri)
    })
}

/// The vertices LOD 0 draws, renumbered: coarser LODs keep vertices of
/// their own, which would only widen the bounds of a mesh that has none.
fn lod0(mesh: &rbx_mesh::Mesh) -> (Vec<rbx_mesh::Vertex>, Vec<u32>) {
    let mut renumbered = vec![u32::MAX; mesh.vertices.len()];
    let mut vertices = Vec::new();
    let indices = mesh
        .indices
        .iter()
        .map(|&index| {
            let slot = &mut renumbered[index as usize];
            if *slot == u32::MAX {
                *slot = vertices.len() as u32;
                vertices.push(mesh.vertices[index as usize]);
            }
            *slot
        })
        .collect();
    (vertices, indices)
}

fn has_bones(dom: &WeakDom, target: Ref) -> bool {
    let mut stack = vec![target];
    while let Some(instance) = stack.pop().and_then(|r| dom.get(r)) {
        if instance.class() == "Bone" {
            return true;
        }
        stack.extend_from_slice(instance.children());
    }
    false
}
