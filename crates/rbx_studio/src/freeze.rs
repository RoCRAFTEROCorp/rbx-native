//! Freeze Rotation: an instance's turn cleared to `0, 0, 0` while it keeps
//! looking exactly as it did — Blender's Apply › Rotation ("rotation values
//! are cleared to zero … the geometry itself is adjusted so that the object
//! continues to appear unchanged"). Its origin stays where it was, unturned:
//! the pivot keeps its place in the world and loses its turn, as a Blender
//! object's origin does. Everything else hanging off the instance's own
//! frame — child `Attachment`s, the `C0`/`C1` of joints naming it — keeps
//! its whole world frame, as Blender adjusts children.
//!
//! Format-honest: what a place stores after a freeze is only ever what
//! Roblox itself stores. Three cases qualify (see [`route`]):
//!
//! - A `Model`: it has no geometry of its own, its turn is its pivot's, and
//!   clearing that is a pivot write ([`local`]).
//! - A `Block` part turned by quarter turns, or a `Ball`: the same shape can
//!   be described unturned — a block by swapping its `Size` axes and moving
//!   its surfaces and decals to the faces now pointing their way, a ball as
//!   it is ([`local`]). A block at any other angle, a cylinder or a wedge
//!   cannot: its shape comes from `Shape` and `Size` alone, axis-aligned.
//! - A `MeshPart`: its triangles are baked into a new mesh that is uploaded
//!   as a real Roblox asset ([`mesh`]).

use glam::{Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::transform;

mod local;
mod mesh;

pub(crate) use local::freeze_local;
pub(crate) use mesh::{apply, mesh_asset_id, mesh_uri, plan, same_shape, Plan};

const JOINT: &str = "JointInstance";

/// Below this a rotation counts as none: nothing to freeze.
const UNTURNED: f32 = 1e-5;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// How a freezable instance is frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// A `MeshPart`: bake, upload, then [`apply`].
    Upload,
    /// A `Model`, a quarter-turned block or a ball: [`freeze_local`] at once.
    Local,
}

/// How `target` can be frozen, or `None` when it cannot: unturned already,
/// or a shape no unturned `Size` describes.
pub(crate) fn route(dom: &WeakDom, database: &ReflectionDatabase, target: Ref) -> Option<Route> {
    if mesh::freezable(dom, database, target) {
        return Some(Route::Upload);
    }
    local::freezable(dom, database, target).then_some(Route::Local)
}

pub(crate) fn freezable(dom: &WeakDom, database: &ReflectionDatabase, target: Ref) -> bool {
    route(dom, database, target).is_some()
}

fn turned(frame: &CFrameData) -> bool {
    frame
        .rotation
        .iter()
        .zip(IDENTITY)
        .any(|(a, b)| (a - b).abs() > UNTURNED)
}

/// What has to be rewritten so nothing hanging off `target` moves when its
/// frame goes from `old` to `new`: its pivot (kept in place, its turn
/// cleared), child attachments and joint offsets (kept whole).
fn rehang(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    target: Ref,
    old: &CFrameData,
    new: &CFrameData,
) -> Vec<(Ref, &'static str, Variant)> {
    let Some(instance) = dom.get(target) else {
        return Vec::new();
    };
    let (old, new) = (transform::rigid(old), transform::rigid(new));
    // `new⁻¹ · old`: what takes a frame relative to the old part to the same
    // world frame relative to the new one.
    let carry = new.inverse() * old;
    let rewrite =
        |local: &CFrameData| Variant::CFrame(transform::cframe(carry * transform::rigid(local)));
    let mut rewrites = Vec::new();

    let offset = cframe_of(database, instance, "PivotOffset")
        .map_or(Mat4::IDENTITY, |offset| transform::rigid(&offset));
    let pivot = (old * offset).w_axis.truncate();
    let kept = transform::cframe(new.inverse() * Mat4::from_translation(pivot));
    let stored = instance.properties().contains_key("PivotOffset");
    if stored
        || turned(&kept)
        || Vec3::new(kept.position.x, kept.position.y, kept.position.z).length() > UNTURNED
    {
        rewrites.push((target, "PivotOffset", Variant::CFrame(kept)));
    }

    for &child in instance.children() {
        let Some(attachment) = dom.get(child).filter(|c| c.class() == "Attachment") else {
            continue;
        };
        if let Some(local) = cframe_of(database, attachment, "CFrame") {
            rewrites.push((child, "CFrame", rewrite(&local)));
        }
    }
    for (joint, key) in joints_naming(dom, database, target) {
        if let Some(local) = dom.get(joint).and_then(|j| cframe_of(database, j, key)) {
            rewrites.push((joint, key, rewrite(&local)));
        }
    }
    rewrites
}

/// Writes every `(instance, property, value)`, each under the spelling its
/// instance already stores it as (`size` stays `size`).
fn write_all(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    writes: Vec<(Ref, &str, Variant)>,
) -> Result<(), String> {
    let keyed: Vec<(Ref, String, Variant)> = writes
        .into_iter()
        .filter_map(|(referent, name, value)| {
            let owner = dom.get(referent)?;
            let key = database
                .stored_or_default(owner, name)
                .map_or(name, |(key, _)| key);
            Some((referent, key.to_string(), value))
        })
        .collect();
    for (referent, key, value) in keyed {
        dom.set_property(referent, &key, value)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Every joint anywhere in the place whose `Part0` or `Part1` is `part`,
/// with the offset (`C0` or `C1`) that is relative to it.
fn joints_naming(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    part: Ref,
) -> Vec<(Ref, &'static str)> {
    let mut found = Vec::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        stack.extend_from_slice(instance.children());
        if !database.is_subclass_of(instance.class(), JOINT) {
            continue;
        }
        for (side, offset) in [("Part0", "C0"), ("Part1", "C1")] {
            if matches!(instance.properties().get(side), Some(Variant::Ref(r)) if *r == part) {
                found.push((referent, offset));
            }
        }
    }
    found
}

fn cframe_of(
    database: &ReflectionDatabase,
    instance: &rbx_dom::Instance,
    name: &str,
) -> Option<CFrameData> {
    match database.stored_or_default(instance, name)? {
        (_, Variant::CFrame(frame)) => Some(*frame),
        _ => None,
    }
}

fn vector_of(
    database: &ReflectionDatabase,
    instance: &rbx_dom::Instance,
    name: &str,
) -> Option<Vec3> {
    match database.stored_or_default(instance, name)? {
        (_, Variant::Vector3(v)) => Some(Vec3::new(v.x, v.y, v.z)),
        _ => None,
    }
}

fn vector3(v: Vec3) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v.x,
        y: v.y,
        z: v.z,
    })
}

#[cfg(test)]
#[path = "freeze/tests.rs"]
mod tests;
