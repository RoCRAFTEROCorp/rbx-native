//! Studio's Edit Pivot tool in the 3D view: its arrows, rings and free-drag
//! ball move and turn the selection's pivot, never the selection itself
//! (`creator-docs`, `studio/pivot-tools.md`).
//!
//! The gestures are Move's and Rotate's own ([`Drag::Axis`], [`Drag::Ring`],
//! and the free-drag ball's [`Drag::Plane`]), measured by the same
//! [`gizmo::advance`]; only what a step changes differs — the pivot, which
//! `Shell` writes as a `PivotOffset` or a `WorldPivot` (see
//! `rbx_lua::pivot::set_pivot`). With Snap on, a free drag lands on the
//! nearest of the selection's hotspots instead of wherever the cursor is
//! ("hotspots such as corners, edges, or centers"), drawn as the magenta
//! points Studio draws.

use glam::{Mat3, Mat4, Vec3};
use gpui_kit::Modifiers;
use rbx_viewer::gizmo::{self, Faces, Handles, Hotspots};
use rbx_viewer::pick::Ray;

use super::gizmo::{advance, Change, Drag, Landing};
use super::{ViewportAction, WorkspaceView};

impl WorkspaceView {
    /// What `ray` grabs of Edit Pivot's handles: the ball at the pivot
    /// first (it sits innermost), then an arrow, then a ring.
    pub(super) fn grab_pivot(&self, ray: Ray) -> Option<Drag> {
        let pivot = self.targets.pivot()?;
        let handles = self.handles()?;
        grab(&handles, pivot, ray)
    }

    /// One step of an Edit Pivot drag: the pivot's new placement, kept here
    /// so the handles follow the cursor, and sent to `Shell` to write.
    pub(super) fn pivot_step(
        &mut self,
        drag: Drag,
        ray: Ray,
        modifiers: Modifiers,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let Some(pivot) = self.targets.pivot() else {
            return;
        };
        // No soft snaps: they pull a part's faces onto its neighbours',
        // which means nothing for a point.
        let landing = Landing {
            grid: self.transform.translate.grid(modifiers.shift),
            angle: self.transform.rotate.grid(modifiers.shift).to_radians(),
            snaps: &[],
        };
        let Some((drag, change)) = advance(drag, ray, landing) else {
            return;
        };
        self.drag = Some(drag);
        let hotspots =
            matches!(drag, Drag::Plane { .. }) && self.transform.pivot_snap && !modifiers.shift;
        let spots = self.hotspots().filter(|_| hotspots);
        let (moved, snapped) = stepped(pivot, change, spots.as_ref(), ray);
        self.snapped = snapped;
        if moved == pivot {
            self.refresh_gizmo();
            return;
        }
        self.targets.set_pivot(moved);
        self.refresh_gizmo();
        let first = !std::mem::replace(&mut self.dragged, true);
        cx.emit(ViewportAction::Pivot { to: moved, first });
    }

    /// The hotspots, as the renderer draws them: the selection's box,
    /// squared to its pivot (see `transform::Targets::pivot_box`), and each
    /// part's own.
    fn hotspots(&self) -> Option<Hotspots> {
        let view = self.view?;
        let faces = |model| Faces::new(model, view, self.orthographic);
        let parts = self.targets.iter().map(|target| faces(target.model));
        Some(Hotspots::new(faces(self.targets.pivot_box()?), parts))
    }
}

/// The handle `ray` is on, as the drag it opens on the pivot.
fn grab(handles: &Handles, pivot: Mat4, ray: Ray) -> Option<Drag> {
    let origin = handles.origin();
    if handles.grab_origin(ray) {
        return Some(Drag::Plane {
            point: origin,
            normal: -ray.direction,
            offset: Vec3::ZERO,
        });
    }
    if let Some(axis) = handles.grab(ray) {
        let axis = handles.direction(axis);
        return Some(Drag::Axis {
            origin,
            axis,
            grabbed: gizmo::along_axis(origin, axis, ray)?,
        });
    }
    let axis = handles.grab_ring(ray)?;
    let frame = handles.ring_frame(axis);
    let (angle, ..) = gizmo::ring_crossing(origin, frame, ray)?;
    Some(Drag::Ring {
        origin,
        frame,
        orientation: Mat3::from_mat4(pivot),
        last: angle,
        turned: 0.0,
    })
}

/// The pivot as `change` leaves it, and the hotspot it snapped onto: a
/// free drag (`Change::Position` with `hotspots` given) lands on the one
/// nearest the cursor's `ray` when one is in reach. A move keeps the
/// pivot's turn, a turn keeps where it stands.
fn stepped(
    pivot: Mat4,
    change: Change,
    hotspots: Option<&Hotspots>,
    ray: Ray,
) -> (Mat4, Option<Vec3>) {
    let mut moved = pivot;
    match change {
        Change::Position(position) => {
            let snapped = hotspots.and_then(|hotspots| hotspots.nearest(ray));
            moved.w_axis = snapped.unwrap_or(position).extend(1.0);
            return (moved, snapped);
        }
        Change::Orientation(orientation) => {
            moved.x_axis = orientation.x_axis.extend(0.0);
            moved.y_axis = orientation.y_axis.extend(0.0);
            moved.z_axis = orientation.z_axis.extend(0.0);
        }
        // No handle of Edit Pivot's resizes anything.
        Change::Size { .. } | Change::Scaled { .. } => {}
    }
    (moved, None)
}

#[cfg(test)]
#[path = "pivot/tests.rs"]
mod tests;
