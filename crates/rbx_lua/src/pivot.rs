//! Where a part's or model's pivot stands in the world, and moving an
//! instance so its pivot lands somewhere else: `PVInstance:GetPivot` and
//! `PVInstance:PivotTo`. Shared by Luau (see `instance`) and the editor's
//! Properties panel, whose `Origin` row reads and writes the same thing.
//!
//! A part's pivot is its `CFrame` times its `PivotOffset`. A model's is its
//! `PrimaryPart`'s when it has one, else its `WorldPivot`, else — a file from
//! before pivots, which stores none — the centre of its parts' bounds, where
//! Studio's pivot Reset puts it.

use std::collections::HashSet;

use glam::{Affine3A, Mat3, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

const CFRAME: &str = "CFrame";
const PIVOT_OFFSET: &str = "PivotOffset";
const WORLD_PIVOT: &str = "WorldPivot";
const PRIMARY_PART: &str = "PrimaryPart";
const SIZE: &str = "Size";
const BASE_PART: &str = "BasePart";
const MODEL: &str = "Model";

/// `reference`'s pivot in world space, if it is a part or a model.
pub fn pivot(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref) -> Option<CFrameData> {
    let instance = dom.get(reference)?;
    let class = instance.class();
    if db.is_subclass_of(class, BASE_PART) {
        let frame = cframe(dom, db, reference, CFRAME)?;
        return Some(match cframe(dom, db, reference, PIVOT_OFFSET) {
            Some(offset) if offset != IDENTITY => to_data(affine(&frame) * affine(&offset)),
            _ => frame,
        });
    }
    if !db.is_subclass_of(class, MODEL) {
        return None;
    }
    if let Some(Variant::Ref(primary)) = instance.properties().get(PRIMARY_PART) {
        if dom
            .get(*primary)
            .is_some_and(|part| db.is_subclass_of(part.class(), BASE_PART))
        {
            return pivot(dom, db, *primary);
        }
    }
    if let Some(Variant::CFrame(world)) = instance.properties().get(WORLD_PIVOT) {
        return Some(*world);
    }
    bounds_centre(dom, db, reference)
}

/// `PivotTo`: moves `reference` so its pivot lands on `to`, carrying every
/// part under a model, and the model's own `WorldPivot`, by the same move.
/// `None` when `reference` has no pivot (it is neither a part nor a model).
pub fn pivot_to(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    to: &CFrameData,
) -> Option<Result<(), String>> {
    let from = pivot(dom, db, reference)?;
    Some(move_to(dom, db, reference, &from, to, &mut HashSet::new()))
}

/// Carries `reference` — a part, or every part under a model and the
/// model's own `WorldPivot` — by what takes its pivot from `from` to `to`.
pub fn move_to(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    from: &CFrameData,
    to: &CFrameData,
    moved: &mut HashSet<Ref>,
) -> Result<(), String> {
    let carry = |frame: &CFrameData| carried(frame, from, to);
    let mut parts: Vec<Ref> = vec![reference];
    let mut index = 0;
    while let Some(&current) = parts.get(index) {
        index += 1;
        if let Some(instance) = dom.get(current) {
            parts.extend_from_slice(instance.children());
        }
    }
    for part in parts {
        let Some(instance) = dom.get(part) else {
            continue;
        };
        let class = instance.class();
        let key = if db.is_subclass_of(class, BASE_PART) {
            CFRAME
        } else if db.is_subclass_of(class, MODEL) && part == reference {
            WORLD_PIVOT
        } else {
            continue;
        };
        if !moved.insert(part) {
            continue;
        }
        let Some((key, Variant::CFrame(frame))) = db.stored_or_default(instance, key) else {
            continue;
        };
        let (key, frame) = (key.to_owned(), carry(frame));
        dom.set_property(part, &key, Variant::CFrame(frame))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// `frame` carried along with a pivot moving from `from` to `to`. Only the
/// position changing — the panel leaves a rotation it did not change
/// byte-identical — moves `frame` by the difference alone, so no rotation
/// passes through an inverse and comes back a rounding error off.
fn carried(frame: &CFrameData, from: &CFrameData, to: &CFrameData) -> CFrameData {
    if from.rotation == to.rotation {
        let shift = vec3(&to.position) - vec3(&from.position);
        return CFrameData {
            position: vector(vec3(&frame.position) + shift),
            rotation: frame.rotation,
        };
    }
    to_data(affine(to) * affine(from).inverse() * affine(frame))
}

/// The centre of the world bounds of every part under `reference`, facing
/// the world's axes.
fn bounds_centre(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref) -> Option<CFrameData> {
    let mut pending = vec![reference];
    let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    while let Some(current) = pending.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        pending.extend_from_slice(instance.children());
        if !db.is_subclass_of(instance.class(), BASE_PART) {
            continue;
        }
        let (Some(frame), Some((_, Variant::Vector3(size)))) = (
            cframe(dom, db, current, CFRAME),
            db.stored_or_default(instance, SIZE),
        ) else {
            continue;
        };
        let frame = affine(&frame);
        let half = vec3(size) * 0.5;
        // A box's world extent along each axis is its half-size projected
        // through the absolute rotation.
        let reach = Mat3::from(frame.matrix3).abs() * half;
        let centre = Vec3::from(frame.translation);
        low = low.min(centre - reach);
        high = high.max(centre + reach);
    }
    (low.x <= high.x).then(|| CFrameData {
        position: vector((low + high) * 0.5),
        rotation: IDENTITY.rotation,
    })
}

fn cframe(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    name: &str,
) -> Option<CFrameData> {
    match db.stored_or_default(dom.get(reference)?, name)? {
        (_, Variant::CFrame(frame)) => Some(*frame),
        _ => None,
    }
}

const IDENTITY: CFrameData = CFrameData {
    position: Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
};

/// A `CFrame` as an affine map; its rotation is stored row by row.
fn affine(frame: &CFrameData) -> Affine3A {
    let rows = Mat3::from_cols_array(&frame.rotation);
    Affine3A::from_mat3_translation(rows.transpose(), vec3(&frame.position))
}

fn to_data(map: Affine3A) -> CFrameData {
    CFrameData {
        position: vector(map.translation.into()),
        rotation: Mat3::from(map.matrix3).transpose().to_cols_array(),
    }
}

fn vec3(value: &Vector3Data) -> Vec3 {
    Vec3::new(value.x, value.y, value.z)
}

fn vector(value: Vec3) -> Vector3Data {
    Vector3Data {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::Instance;

    use super::*;

    fn at(x: f32, y: f32, z: f32) -> CFrameData {
        CFrameData {
            position: Vector3Data { x, y, z },
            rotation: IDENTITY.rotation,
        }
    }

    #[test]
    fn pivot_to_moves_a_part_by_its_pivot_not_its_centre() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let part = Ref::new(1);
        let mut instance = Instance::new(part, "Part", "Part");
        instance
            .properties_mut()
            .insert(CFRAME.into(), Variant::CFrame(at(0.0, 0.0, 0.0)));
        instance
            .properties_mut()
            .insert(PIVOT_OFFSET.into(), Variant::CFrame(at(0.0, -2.0, 0.0)));
        dom.insert(instance);

        assert_eq!(pivot(&dom, &db, part), Some(at(0.0, -2.0, 0.0)));
        pivot_to(&mut dom, &db, part, &at(5.0, 0.0, 0.0))
            .expect("a part has a pivot")
            .unwrap();
        assert_eq!(pivot(&dom, &db, part), Some(at(5.0, 0.0, 0.0)));
        assert_eq!(
            dom.get(part).unwrap().properties().get(CFRAME),
            Some(&Variant::CFrame(at(5.0, 2.0, 0.0)))
        );
    }

    #[test]
    fn something_that_is_neither_part_nor_model_has_no_pivot() {
        let db = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let folder = dom.new_instance("Folder", "Folder", None);
        assert_eq!(pivot(&dom, &db, folder), None);
        assert!(pivot_to(&mut dom, &db, folder, &at(1.0, 1.0, 1.0)).is_none());
    }
}
