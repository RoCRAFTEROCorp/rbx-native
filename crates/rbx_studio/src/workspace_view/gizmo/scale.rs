//! The Scale tool's balls: which one a press holds and the drag that opens,
//! and what one step of that drag does — a lone part's `Size` along one of
//! its own axes, or a model or group scaled whole by one factor. Either
//! scales from the face opposite the one pulled, or, with `Ctrl` held,
//! about the pivot.
//!
//! Follows Studio's own `ScaleDragger` (`DraggerFramework`'s
//! `ExtrudeHandles` and `DraggerSchemaCore`'s `ExtrudeHandlesImplementation`,
//! read from the client's disassembled built-in plugin): the box the balls
//! stand on is `rbx_viewer::gizmo::scale_box`, and a model scales the way
//! `Model:ScaleTo` does, its pivot moving as one more scaled point.

use glam::Vec3;
use gpui_kit::Modifiers;
use rbx_viewer::gizmo::{self, Axis, Faces};
use rbx_viewer::pick::Ray;

use crate::transform::{Target, Targets};

use super::{snapped, Change, Drag, Landing, WorkspaceView, MAX_SIZE, MIN_SIZE};

/// Whether a Scale drag scales about the pivot rather than from the face
/// opposite the grabbed one: Studio's `shouldScaleFromCenter` is its
/// `isCtrlKeyDown`. `Cmd` stands in for it on macOS as it does for this
/// editor's other `Ctrl` chords — the disassembly does not say which key
/// Studio reads there.
pub(super) fn from_pivot(modifiers: Modifiers) -> bool {
    modifiers.control || modifiers.platform
}

impl WorkspaceView {
    /// The Scale ball `ray` grabs, if any, and the drag it opens: a lone
    /// part's own resize, or the whole selection's scale.
    pub(super) fn grab_scale(&self, ray: Ray, lock_shape: bool, centred: bool) -> Option<Drag> {
        let faces = self.faces()?;
        if self.targets.lone_part() {
            grab_face(&faces, self.targets.anchor()?, ray, lock_shape, centred)
        } else {
            let pivot = self.targets.scale_centre(self.transform.local)?;
            grab_box(&faces, ray, pivot, centred)
        }
    }

    /// `drag` again, re-measured from where the selection stands now when
    /// `Ctrl` has gone down or up since it was last read: Studio's
    /// `ExtrudeHandles` re-reads `shouldScaleFromCenter` on every move and,
    /// when it changes, keeps the scale so far and measures afresh from
    /// there (`_updateExtrudeMode`, `_refreshDrag`), so letting go of `Ctrl`
    /// mid-drag never makes the selection jump. Any other drag, or one whose
    /// `Ctrl` has not changed, comes back as it was.
    pub(super) fn rescale(&mut self, drag: Drag, ray: Ray, centred: bool) -> Drag {
        let (Drag::Size { centred: was, .. } | Drag::Box { centred: was, .. }) = drag else {
            return drag;
        };
        if was == centred {
            return drag;
        }
        let local = self.transform.local;
        let Some(rebased) = self
            .faces()
            .and_then(|faces| regrab(drag, &self.targets, &faces, local, ray, centred))
        else {
            return drag;
        };
        // A group's scale is a factor of the selection as it was held; that
        // is the selection as it stands now from here on.
        self.held = self.targets.clone();
        rebased
    }
}

/// A Scale `drag` measured afresh from `targets` as they stand now, its
/// balls on `faces`, with `Ctrl` now `centred` (see
/// [`WorkspaceView::rescale`]). `None` for any other drag.
fn regrab(
    drag: Drag,
    targets: &Targets,
    faces: &Faces,
    local: bool,
    ray: Ray,
    centred: bool,
) -> Option<Drag> {
    Some(match drag {
        Drag::Size {
            axis,
            component,
            sphere,
            cylinder,
            ..
        } => {
            let anchor = targets.anchor()?;
            let origin = anchor.position();
            Drag::Size {
                origin,
                axis,
                grabbed: gizmo::along_axis(origin, axis, ray)?,
                size: anchor.size(),
                component,
                sphere,
                cylinder,
                centred,
            }
        }
        Drag::Box { axis, .. } => {
            let along = |each: Axis| faces.direction(each).dot(axis).abs();
            let pulled = Axis::ALL
                .into_iter()
                .max_by(|&a, &b| along(a).total_cmp(&along(b)))?;
            let origin = faces.centre();
            boxed(
                origin,
                axis,
                gizmo::along_axis(origin, axis, ray)?,
                faces.extent(pulled),
                targets.scale_centre(local)?,
                centred,
            )
        }
        _ => return None,
    })
}

/// Which of the part's own faces a Scale ball stands on, and the drag that
/// grabbing it opens.
///
/// No guessing at which axis the user meant: a ball sits *on* one of the
/// part's own faces, and `BasePart.Size` is expressed along exactly those axes
/// — so the handle names the component that stretches outright, whichever way
/// the world/local toggle stands.
///
/// `lock_shape` is `Alt` at the moment of the grab (see `press`) — free to
/// repurpose here because a click that reaches a handle at all never reads
/// `Alt` for anything else (unlike a click that falls through to a pick,
/// where it means "cycle selection").
pub(super) fn grab_face(
    faces: &Faces,
    target: Target,
    ray: Ray,
    lock_shape: bool,
    centred: bool,
) -> Option<Drag> {
    let (grabbed, sign) = faces.grab(ray)?;
    // Pointing out through the grabbed face, so dragging away from the part
    // always grows it.
    let axis = faces.direction(grabbed) * sign;

    let origin = target.position();
    Some(Drag::Size {
        origin,
        axis,
        sphere: target.sphere && lock_shape,
        cylinder: target.cylinder && lock_shape,
        grabbed: gizmo::along_axis(origin, axis, ray)?,
        size: target.size(),
        component: grabbed as usize,
        centred,
    })
}

/// Which face of a model's or group's box a Scale ball stands on, and the
/// whole-selection drag grabbing it opens: pulling the face scales every part
/// by the same factor about the opposite face, which holds still — or about
/// `pivot` with `centred`.
///
/// One factor rather than one axis: a group's parts stand at every angle to
/// the box, and stretching the box along one axis is nothing a rotated part's
/// own `Size` can express. Studio's `ScaleDragger` scales a model by one
/// factor too, through `Model:ScaleTo`.
pub(super) fn grab_box(faces: &Faces, ray: Ray, pivot: Vec3, centred: bool) -> Option<Drag> {
    let (grabbed, sign) = faces.grab(ray)?;
    let axis = faces.direction(grabbed) * sign;
    let origin = faces.centre();
    let along = gizmo::along_axis(origin, axis, ray)?;
    Some(boxed(
        origin,
        axis,
        along,
        faces.extent(grabbed),
        pivot,
        centred,
    ))
}

fn boxed(origin: Vec3, axis: Vec3, grabbed: f32, extent: f32, pivot: Vec3, centred: bool) -> Drag {
    Drag::Box {
        origin,
        axis,
        grabbed,
        extent,
        far: origin - axis * extent * 0.5,
        pivot,
        centred,
    }
}

/// One step of a Scale drag (see `super::advance`), or `None` for any other
/// drag or an axis sighted end-on.
///
/// Scaling about the middle, Studio doubles the snapped pull
/// (`ExtrudeHandles`' `_lastResizeFromCenter`), so that both faces move as
/// far as the cursor did rather than half as far each.
pub(super) fn advance(drag: Drag, ray: Ray, landing: Landing) -> Option<(Drag, Change)> {
    match drag {
        Drag::Size {
            origin,
            axis,
            grabbed,
            size,
            component,
            sphere,
            cylinder,
            centred,
        } => {
            let pull = snapped(gizmo::along_axis(origin, axis, ray)? - grabbed, landing);
            let travelled = if centred { pull * 2.0 } else { pull };
            let mut resized = size;
            resized[component] = (size[component] + travelled).clamp(MIN_SIZE, MAX_SIZE);
            // Half the growth, so the face opposite the grabbed one holds
            // still and the grabbed one follows the cursor. Taken from what
            // the size *actually* changed by rather than from the travel, so
            // a drag that has run into either end of the range stops moving
            // the part as well as stops resizing it.
            let grown = resized[component] - size[component];
            if sphere {
                // The other two axes grow by the same amount, with no
                // position term of their own: nothing anchors either of
                // their two faces the way `component`'s opposite face is
                // anchored above, so growing the size alone is what keeps
                // the ball centred on both of them. Clamped independently,
                // since a ball dragged from an already-uneven size (its
                // Y or Z started closer to `MAX_SIZE` than X did) can still
                // run out of room on one axis before another — a corner
                // case worth a comment, not a reason to hold every axis to
                // whichever one clamps first.
                for other in 0..3 {
                    if other != component {
                        resized[other] = (size[other] + grown).clamp(MIN_SIZE, MAX_SIZE);
                    }
                }
            }
            if cylinder && component != 0 {
                // `component` is 1 (Y) or 2 (Z) — the round pair, since a
                // Cylinder's length always sits on the part's own X (see
                // `Target::cylinder`'s own doc comment). Grabbing X itself
                // (`component == 0`) leaves this branch untouched, which is
                // exactly right: the length has no partner to grow with.
                let other = if component == 1 { 2 } else { 1 };
                resized[other] = (size[other] + grown).clamp(MIN_SIZE, MAX_SIZE);
            }
            // A lone part's box is its own, so its pivot for `Ctrl` is its
            // middle (Studio leaves `PivotOffset` out of it), which holds.
            let position = if centred {
                origin
            } else {
                origin + axis * grown * 0.5
            };
            Some((
                drag,
                Change::Size {
                    size: resized,
                    position,
                },
            ))
        }
        Drag::Box {
            origin,
            axis,
            grabbed,
            extent,
            far,
            pivot,
            centred,
        } => {
            let pull = snapped(gizmo::along_axis(origin, axis, ray)? - grabbed, landing);
            let travelled = if centred { pull * 2.0 } else { pull };
            // The box's own length along the pulled axis, held to the same
            // range a part's Size is; what every part scales by is how much
            // that grew or shrank in proportion.
            let pulled = (extent + travelled).clamp(MIN_SIZE, MAX_SIZE);
            Some((
                drag,
                Change::Scaled {
                    pivot: if centred { pivot } else { far },
                    factor: pulled / extent,
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "scale/tests.rs"]
mod tests;
