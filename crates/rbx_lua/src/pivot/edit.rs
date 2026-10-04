//! Moving a pivot without moving what it belongs to — Studio's Edit Pivot
//! tool and its Reset button (`studio/pivot-tools.md`) — and keeping a
//! model's pivot where it is when the parts it was read off are moved or
//! deleted out from under it.

use std::iter::successors;

use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{
    bounds_centre, carried, cframe, compose, pivot, primary_part, BASE_PART, CFRAME, IDENTITY,
    MODEL, PIVOT_OFFSET, PRIMARY_PART, WORLD_PIVOT,
};
use crate::datatypes::cframe::LuaCFrame;

/// Puts `reference`'s pivot on `to` and leaves the instance where it is: a
/// part's `PivotOffset`, a model's `PrimaryPart`'s `PivotOffset` while it has
/// one (its pivot *is* that part's), else the model's `WorldPivot` — the
/// three properties `studio/pivot-tools.md` says "will not move or rotate
/// the object". `None` when `reference` has no pivot.
pub fn set_pivot(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    to: &CFrameData,
) -> Option<Result<(), String>> {
    let class = dom.get(reference)?.class();
    let owner = if db.is_subclass_of(class, BASE_PART) {
        reference
    } else if db.is_subclass_of(class, MODEL) {
        match primary_part(dom, db, reference) {
            Some(primary) => primary,
            None => return Some(write(dom, reference, WORLD_PIVOT, *to)),
        }
    } else {
        return None;
    };
    let frame = cframe(dom, db, owner, CFRAME)?;
    let offset = compose(&LuaCFrame(frame).inverse().0, to);
    Some(write(dom, owner, PIVOT_OFFSET, offset))
}

/// Studio's pivot Reset: "moves the pivot point to the **center** of an
/// object or model's bounding box". A part's box is its own, so its offset
/// simply goes back to nothing; a model's box is the one round its parts
/// squared to the pivot's own axes (how `Model:GetBoundingBox` orients it),
/// and the pivot keeps facing the way it did. `None` without a pivot.
pub fn reset(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
) -> Option<Result<(), String>> {
    if db.is_subclass_of(dom.get(reference)?.class(), BASE_PART) {
        return Some(write(dom, reference, PIVOT_OFFSET, IDENTITY));
    }
    let current = pivot(dom, db, reference)?;
    let centre = bounds_centre(dom, db, reference, &current.rotation)?;
    set_pivot(dom, db, reference, &centre)
}

/// Clears `model`'s `PrimaryPart`, and — when it had one — puts its pivot
/// back on the centre of its bounding box, as [`reset`] does:
/// `studio/pivot-tools.md` has unassigning a `PrimaryPart` reset the pivot,
/// where deleting the part ([`keep_pivots`]) leaves it be. The box is squared
/// to the pivot the part gave it, which is the one on screen. Returns the
/// value cleared.
pub fn clear_primary_part(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    model: Ref,
) -> Result<Option<Variant>, String> {
    let was = primary_part(dom, db, model).and_then(|_| pivot(dom, db, model));
    let cleared = dom
        .remove_property(model, PRIMARY_PART)
        .map_err(|err| err.to_string())?;
    if let Some(was) = was {
        write(dom, model, WORLD_PIVOT, was)?;
        reset(dom, db, model).unwrap_or(Ok(()))?;
    }
    Ok(cleared)
}

/// Pins the pivot of every model whose `PrimaryPart` is about to go with
/// `doomed` (each one with its whole subtree): its `WorldPivot` takes the
/// pivot it reads now. Call it before removing them.
///
/// `studio/pivot-tools.md`: "If you **delete** the `PrimaryPart` from a
/// model, the pivot point remains in the same location and does **not**
/// revert to its previous position. This prevents a sudden "jump"". While
/// the part was primary its pivot stood in for the model's, and the
/// `WorldPivot` underneath is whatever it was before that — the very jump.
pub fn keep_pivots(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    doomed: &[Ref],
) -> Result<(), String> {
    let goes =
        |part: Ref| successors(Some(part), |&at| dom.parent(at)).any(|at| doomed.contains(&at));
    let mut writes = Vec::new();
    for &root in doomed {
        // A `PrimaryPart` is always inside its model, so a model losing one
        // is an ancestor of what goes.
        for model in successors(dom.parent(root), |&at| dom.parent(at)) {
            if goes(model) || primary_part(dom, db, model).is_none_or(|part| !goes(part)) {
                continue;
            }
            if let Some(frame) = pivot(dom, db, model) {
                writes.push((model, frame));
            }
        }
    }
    writes
        .into_iter()
        .try_for_each(|(model, frame)| write(dom, model, WORLD_PIVOT, frame))
}

/// A model whose stored `WorldPivot` has to follow its parts when they are
/// written one by one — a drag in the viewport writes each part's `CFrame`
/// and nothing else, where Studio carries a dragged model's pivot with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Follower {
    model: Ref,
    /// The part whose move the pivot copies, and where both stood.
    part: Ref,
    part_was: CFrameData,
    pivot_was: CFrameData,
}

/// What [`follow`] has to carry when the parts under `roots` move: each
/// model among them and under them with a `WorldPivot` of its own and no
/// `PrimaryPart` — a `PrimaryPart`'s pivot travels with that part already —
/// read off its first part.
pub fn followers(dom: &WeakDom, db: &ReflectionDatabase, roots: &[Ref]) -> Vec<Follower> {
    let mut pending = roots.to_vec();
    let mut found = Vec::new();
    while let Some(current) = pending.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        pending.extend_from_slice(instance.children());
        if !db.is_subclass_of(instance.class(), MODEL) || primary_part(dom, db, current).is_some() {
            continue;
        }
        let Some(Variant::CFrame(pivot_was)) = instance.properties().get(WORLD_PIVOT) else {
            continue;
        };
        if let Some((part, part_was)) = first_part(dom, db, current) {
            found.push(Follower {
                model: current,
                part,
                part_was,
                pivot_was: *pivot_was,
            });
        }
    }
    found
}

/// Carries each follower's `WorldPivot` exactly as its part moved since
/// [`followers`] read it.
///
/// ponytail: rigid only. A Scale step that resizes the parts about a point
/// moves the pivot by its part's own shift rather than scaling it about that
/// point; carrying a scale as well needs the drag's own factor here.
pub fn follow(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    followers: &[Follower],
) -> Result<(), String> {
    for follower in followers {
        let Some(now) = cframe(dom, db, follower.part, CFRAME) else {
            continue;
        };
        if now != follower.part_was {
            let frame = carried(&follower.pivot_was, &follower.part_was, &now);
            write(dom, follower.model, WORLD_PIVOT, frame)?;
        }
    }
    Ok(())
}

fn first_part(dom: &WeakDom, db: &ReflectionDatabase, model: Ref) -> Option<(Ref, CFrameData)> {
    let mut pending = vec![model];
    while let Some(current) = pending.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        if db.is_subclass_of(instance.class(), BASE_PART) {
            return Some((current, cframe(dom, db, current, CFRAME)?));
        }
        pending.extend(instance.children().iter().rev());
    }
    None
}

fn write(dom: &mut WeakDom, owner: Ref, key: &str, frame: CFrameData) -> Result<(), String> {
    dom.set_property(owner, key, Variant::CFrame(frame))
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
#[path = "edit/tests.rs"]
mod tests;
