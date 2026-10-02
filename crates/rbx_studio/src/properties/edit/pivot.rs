//! `Origin`: the row Studio's Properties window shows for where an instance's
//! pivot stands in the world, "the object based on its pivot point rather
//! than its bounding box" (`studio/pivot-tools.md`) — `PVInstance:GetPivot`
//! — and typing into it moves the instance there, as `PivotTo` does. The
//! pivot itself is `rbx_lua::pivot`'s, the same one Luau's `GetPivot` and
//! `PivotTo` use.

use std::collections::HashSet;

use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_lua::pivot::move_to;
pub(crate) use rbx_lua::pivot::pivot;
use rbx_reflection::ReflectionDatabase;

use super::many::fill_blanks;
use super::parse;

pub(crate) const ORIGIN: &str = "Origin";

/// Moves every instance in `selection` so its pivot lands where `text` puts
/// it, as one edit. Where the pivots differ the row showed blank, and what
/// was left blank keeps each instance's own (see `many::commit_all`).
pub(super) fn pivot_all(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    selection: &[Ref],
    text: &str,
) -> Result<(), String> {
    let currents: Vec<CFrameData> = selection
        .iter()
        .map(|&reference| pivot(dom, db, reference).ok_or_else(|| "has no pivot".to_string()))
        .collect::<Result<_, _>>()?;
    let mixed = currents.windows(2).any(|pair| pair[0] != pair[1]);

    // Every value parsed before any is written, so one rejected value moves
    // nothing.
    let mut moves = Vec::with_capacity(selection.len());
    for (&reference, current) in selection.iter().zip(&currents) {
        let value = Variant::CFrame(*current);
        let text = if mixed {
            fill_blanks(&value, text)
        } else {
            text.to_owned()
        };
        let Variant::CFrame(target) = parse(&value, db, "", ORIGIN, &text)? else {
            return Err(format!("{ORIGIN} takes a CFrame"));
        };
        if target != *current {
            moves.push((reference, *current, target));
        }
    }
    // A part selected along with its model moves once, with the model.
    let mut moved = HashSet::new();
    for (reference, from, to) in moves {
        move_to(dom, db, reference, &from, &to, &mut moved)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
