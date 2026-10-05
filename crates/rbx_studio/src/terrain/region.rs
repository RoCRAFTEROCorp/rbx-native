//! Gestures on the selection region: drawing a new one by dragging across
//! the terrain, scaling it by a face's ball, moving it by an arrow, and —
//! for the Transform tool — turning it by a ring.
//!
//! `Shift` on a scale handle scales every axis in proportion and `Ctrl`
//! grows both faces of the pulled axis at once (`terrain-editor.md`'s Select
//! shortcuts); the handle maths is the part Scale tool's own
//! (`rbx_viewer::gizmo`), so the region behaves like a part's box.

use glam::{Mat3, Vec3};
use rbx_terrain::edit::region::StudBox;
use rbx_terrain::VOXEL_STUDS;
use rbx_viewer::gizmo::{along_axis, angle_step, arm_length, ring_crossing, Axis, Faces, Handles};
use rbx_viewer::pick::Ray;
use rbx_viewer::Pose;

use super::outline::region_model;

/// The smallest region a drag can leave: one voxel.
const MIN_SIDE: f32 = VOXEL_STUDS;

/// What a press on the region took hold of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RegionGrab {
    /// A face's ball: scale along `axis` through the `sign` face.
    Face {
        axis: Axis,
        sign: f32,
        grabbed: f32,
    },
    Arrow {
        axis: Axis,
        grabbed: f32,
    },
    Ring {
        axis: Axis,
        angle: f32,
    },
    /// A new region drawn from `anchor` across the plane at its height.
    Draw {
        anchor: Vec3,
    },
}

/// A region gesture in progress, holding what it started from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionDrag {
    pub(crate) grab: RegionGrab,
    start: StudBox,
    rotation: Mat3,
    /// The handles as they stood at the press: a drag measures against the
    /// frame it began in, never the moving one.
    handles: Handles,
    turned: f32,
}

/// The handles the region shows for `pose`: face balls, and the arrows
/// and rings when `transform` (the Transform tool) is on.
pub(crate) fn region_handles(
    region: &StudBox,
    rotation: Mat3,
    pose: Pose,
    orthographic: bool,
) -> (Faces, Handles) {
    let model = region_model(region, rotation);
    let centre = Vec3::from(region.center());
    let arm = arm_length(centre, pose, orthographic);
    let basis = [rotation.x_axis, rotation.y_axis, rotation.z_axis];
    (
        Faces::new(model, pose, orthographic),
        Handles::new(centre, basis, arm),
    )
}

impl RegionDrag {
    /// What a press at `ray` grabs: a handle if it is on one, otherwise a new
    /// region drawn from `surface` (where the ray met the terrain or ground).
    pub(crate) fn press(
        region: &StudBox,
        rotation: Mat3,
        (pose, orthographic): (Pose, bool),
        ray: Ray,
        transform: bool,
        surface: Vec3,
    ) -> RegionDrag {
        let (faces, handles) = region_handles(region, rotation, pose, orthographic);
        let grab = if let Some((axis, sign)) = faces.grab(ray) {
            let point = faces.handle(axis, sign);
            RegionGrab::Face {
                axis,
                sign,
                grabbed: along_axis(point, faces.direction(axis), ray).unwrap_or(0.0),
            }
        } else if let Some(axis) = transform.then(|| handles.grab(ray)).flatten() {
            RegionGrab::Arrow {
                axis,
                grabbed: along_axis(handles.origin(), handles.direction(axis), ray).unwrap_or(0.0),
            }
        } else if let Some(axis) = transform.then(|| handles.grab_ring(ray)).flatten() {
            let (angle, ..) =
                ring_crossing(handles.origin(), handles.ring_frame(axis), ray).unwrap_or_default();
            RegionGrab::Ring { axis, angle }
        } else {
            RegionGrab::Draw { anchor: surface }
        };
        RegionDrag {
            grab,
            start: *region,
            rotation,
            handles,
            turned: 0.0,
        }
    }

    /// The region (and its turn) for the cursor at `ray`.
    pub(crate) fn step(
        &mut self,
        ray: Ray,
        shift: bool,
        ctrl: bool,
        snap: bool,
    ) -> (StudBox, Mat3) {
        let start_size = Vec3::from(self.start.size());
        let start_centre = Vec3::from(self.start.center());
        let (region, rotation) = match self.grab {
            RegionGrab::Face {
                axis,
                sign,
                grabbed,
            } => {
                let direction = self.rotation.col(axis as usize);
                let face = start_centre + direction * (start_size[axis as usize] * 0.5 * sign);
                let pulled = along_axis(face, direction, ray).map_or(0.0, |t| (t - grabbed) * sign);
                let mut size = start_size;
                let mut centre = start_centre;
                let i = axis as usize;
                let grown = if ctrl { pulled * 2.0 } else { pulled };
                let new_side = (start_size[i] + grown).max(MIN_SIDE);
                if shift {
                    size = start_size * (new_side / start_size[i].max(f32::EPSILON));
                } else {
                    size[i] = new_side;
                }
                if !ctrl {
                    // The opposite face holds still.
                    let shift_by = (size - start_size) * 0.5;
                    centre += direction * (shift_by[i] * sign);
                }
                (
                    StudBox::from_center_size(
                        centre.to_array(),
                        size.max(Vec3::splat(MIN_SIDE)).to_array(),
                    ),
                    self.rotation,
                )
            }
            RegionGrab::Arrow { axis, grabbed } => {
                let direction = self.handles.direction(axis);
                let moved =
                    along_axis(self.handles.origin(), direction, ray).map_or(0.0, |t| t - grabbed);
                let centre = start_centre + direction * moved;
                (
                    StudBox::from_center_size(centre.to_array(), start_size.to_array()),
                    self.rotation,
                )
            }
            RegionGrab::Ring { axis, angle } => {
                let frame = self.handles.ring_frame(axis);
                if let Some((now, ..)) = ring_crossing(self.handles.origin(), frame, ray) {
                    self.turned = angle_step(angle, now);
                }
                let turn = Mat3::from_axis_angle(frame.0, self.turned);
                (self.start, turn * self.rotation)
            }
            RegionGrab::Draw { anchor } => {
                let facing = ray.direction.y;
                let point = if facing.abs() > 1e-5 {
                    let t = (anchor.y - ray.origin.y) / facing;
                    if t > 0.0 {
                        ray.at(t)
                    } else {
                        anchor
                    }
                } else {
                    anchor
                };
                let span = (point - anchor).abs();
                let height = span.x.max(span.z).max(MIN_SIDE);
                let min = Vec3::new(
                    anchor.x.min(point.x),
                    anchor.y - height * 0.5,
                    anchor.z.min(point.z),
                );
                let max = Vec3::new(
                    anchor.x.max(point.x),
                    anchor.y + height * 0.5,
                    anchor.z.max(point.z),
                );
                let size = (max - min).max(Vec3::splat(MIN_SIDE));
                (
                    StudBox::from_center_size(((min + max) * 0.5).to_array(), size.to_array()),
                    self.rotation,
                )
            }
        };
        (if snap { region.snapped() } else { region }, rotation)
    }
}

#[cfg(test)]
mod tests;
