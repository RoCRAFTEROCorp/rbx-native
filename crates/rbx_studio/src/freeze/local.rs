//! The freezes that need no upload: a `Model`'s pivot, a `Block` turned by
//! quarter turns, a `Ball`. Each is written straight into the DOM, and each
//! leaves the place holding only what Roblox would store for the same shape
//! built unturned.

use glam::Vec3;
use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{cframe_of, rehang, turned, vector3, vector_of, write_all, IDENTITY};

/// `Enum.NormalId`'s faces in value order (`creator-docs`, `NormalId.yaml`:
/// Right 0, Top 1, Back 2, Left 3, Bottom 4, Front 5): face `axis` is the
/// positive side of that axis, `axis + 3` the negative one.
const SURFACES: [&str; 6] = [
    "RightSurface",
    "TopSurface",
    "BackSurface",
    "LeftSurface",
    "BottomSurface",
    "FrontSurface",
];

/// Materials that are flat colour, with no pattern whose direction follows
/// the part's own axes.
const PLAIN: [&str; 4] = ["SmoothPlastic", "Neon", "Glass", "ForceField"];

/// For each world axis, the part axis that lies along it and whether it
/// points the same way: a rotation made only of quarter turns.
type QuarterTurns = [(usize, f32); 3];

enum Kind {
    Model,
    Block(QuarterTurns),
    Ball(Option<QuarterTurns>),
}

pub(super) fn freezable(dom: &WeakDom, database: &ReflectionDatabase, target: Ref) -> bool {
    kind(dom, database, target).is_some()
}

fn kind(dom: &WeakDom, database: &ReflectionDatabase, target: Ref) -> Option<Kind> {
    let instance = dom.get(target)?;
    let class = instance.class();
    // `Workspace` is a `Model` too; its pivot is not a thing anyone turns.
    if database.is_subclass_of(class, "Model") && !database.is_subclass_of(class, "WorldRoot") {
        let pivot = rbx_lua::pivot::pivot(dom, database, target)?;
        return turned(&pivot).then_some(Kind::Model);
    }
    if !database.is_subclass_of(class, "Part") {
        return None;
    }
    let frame = cframe_of(database, instance, "CFrame").filter(turned)?;
    let quarter = quarter_turns(&frame);
    let shape = match database.stored_or_default(instance, "Shape")? {
        (_, Variant::Enum(value)) => database.enum_name("PartType", *value)?,
        _ => return None,
    };
    match shape {
        "Block" => quarter.map(Kind::Block),
        // A ball is a sphere whatever its turn, but a decal on it sits on a
        // face, and only quarter turns move faces onto faces.
        "Ball" if quarter.is_some() || !has_faced_children(dom, target) => {
            Some(Kind::Ball(quarter))
        }
        _ => None,
    }
}

/// Freezes `target` in place, returning what may still look different
/// (each line for the Output dock), or why it cannot be frozen.
pub(crate) fn freeze_local(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    target: Ref,
) -> Result<Vec<String>, String> {
    let name = dom
        .get(target)
        .map(|i| i.name().to_string())
        .ok_or("the instance is gone")?;
    let Some(kind) = kind(dom, database, target) else {
        return Err(format!(
            "{name} can\u{2019}t be frozen: only a Model, a Ball, a Block turned by quarter turns, or a MeshPart can"
        ));
    };
    let quarter = match kind {
        Kind::Model => {
            let pivot =
                rbx_lua::pivot::pivot(dom, database, target).ok_or("the model has no pivot")?;
            let unturned = CFrameData {
                rotation: IDENTITY,
                ..pivot
            };
            rbx_lua::pivot::set_pivot(dom, database, target, &unturned).unwrap_or(Ok(()))?;
            return Ok(Vec::new());
        }
        Kind::Block(quarter) => Some(quarter),
        Kind::Ball(quarter) => quarter,
    };
    let instance = dom.get(target).ok_or("the instance is gone")?;
    let old = cframe_of(database, instance, "CFrame").ok_or("the part has no CFrame")?;
    let new = CFrameData {
        rotation: IDENTITY,
        ..old
    };
    let mut writes = rehang(dom, database, target, &old, &new);
    writes.push((target, "CFrame", Variant::CFrame(new)));
    let mut notes = Vec::new();
    if let Some(quarter) = quarter {
        let size = vector_of(database, instance, "Size").ok_or("the part has no Size")?;
        writes.push((
            target,
            "Size",
            vector3(Vec3::new(
                size[quarter[0].0],
                size[quarter[1].0],
                size[quarter[2].0],
            )),
        ));
        let surfaces: Vec<Option<Variant>> = SURFACES
            .iter()
            .map(|name| {
                database
                    .stored_or_default(instance, name)
                    .map(|(_, v)| v.clone())
            })
            .collect();
        for face in 0..6 {
            if let Some(value) = surfaces[from_face(&quarter, face)].clone() {
                writes.push((target, SURFACES[face], value));
            }
        }
        for &child in instance.children() {
            if let Some(&Variant::Enum(face)) =
                dom.get(child).and_then(|c| c.properties().get("Face"))
            {
                if (face as usize) < 6 {
                    writes.push((
                        child,
                        "Face",
                        Variant::Enum(to_face(&quarter, face as usize) as u32),
                    ));
                    notes.push(format!(
                        "{name}: {} moved to the face now pointing its way; its image may be turned a quarter within it",
                        dom.get(child).map_or("a child", |c| c.name())
                    ));
                }
            }
        }
        let material = match database.stored_or_default(instance, "Material") {
            Some((_, Variant::Enum(value))) => database.enum_name("Material", *value),
            _ => None,
        };
        if material.is_some_and(|m| !PLAIN.contains(&m)) {
            notes.push(format!(
                "{name}: its {} pattern follows the part\u{2019}s own axes and may now run another way on some faces",
                material.unwrap_or_default()
            ));
        }
    }
    write_all(dom, database, writes)?;
    Ok(notes)
}

/// `frame`'s rotation as quarter turns, or `None` at any other angle.
fn quarter_turns(frame: &CFrameData) -> Option<QuarterTurns> {
    let r = frame.rotation;
    let mut out = [(0, 1.0); 3];
    for (world, slot) in out.iter_mut().enumerate() {
        let row = &r[world * 3..world * 3 + 3];
        let along = (0..3).find(|&part| (row[part].abs() - 1.0).abs() < 1e-4)?;
        if (0..3).any(|part| part != along && row[part].abs() > 1e-4) {
            return None;
        }
        *slot = (along, row[along].signum());
    }
    Some(out)
}

/// The world face that part face `face` points at once turned by `quarter`.
fn to_face(quarter: &QuarterTurns, face: usize) -> usize {
    let (axis, positive) = (face % 3, face < 3);
    let (world, &(_, sign)) = quarter
        .iter()
        .enumerate()
        .find(|(_, (part, _))| *part == axis)
        .expect("quarter turns map every axis");
    if positive == (sign > 0.0) {
        world
    } else {
        world + 3
    }
}

/// The part face that ends up pointing at world face `face`.
fn from_face(quarter: &QuarterTurns, face: usize) -> usize {
    (0..6)
        .find(|&part| to_face(quarter, part) == face)
        .expect("faces map one to one")
}

/// Whether anything under `part` is placed on one of its faces.
fn has_faced_children(dom: &WeakDom, part: Ref) -> bool {
    dom.get(part).is_some_and(|instance| {
        instance
            .children()
            .iter()
            .filter_map(|&child| dom.get(child))
            .any(|child| matches!(child.properties().get("Face"), Some(Variant::Enum(_))))
    })
}

#[cfg(test)]
#[path = "local/tests.rs"]
mod tests;
