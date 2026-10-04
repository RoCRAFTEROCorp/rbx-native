//! Converting between `CFrameData` and the `Mat4` placements the gizmo and
//! the pivot tools work in.

use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Vector3Data};

/// A `CFrame` as the matrix glam turns points with — `CFrameData` keeps its
/// rotation row by row, glam column by column.
pub(crate) fn rigid(frame: &CFrameData) -> Mat4 {
    let position = Vec3::new(frame.position.x, frame.position.y, frame.position.z);
    placement(Mat3::from_cols_array(&frame.rotation).transpose(), position)
}

/// [`rigid`] the other way round, for a pivot written back into the DOM.
pub(crate) fn cframe(pivot: Mat4) -> CFrameData {
    let position = pivot.w_axis;
    CFrameData {
        position: Vector3Data {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        rotation: Mat3::from_mat4(pivot).transpose().to_cols_array(),
    }
}

/// A rigid placement: `rotation`, standing at `position`.
pub(super) fn placement(rotation: Mat3, position: Vec3) -> Mat4 {
    Mat4::from_cols(
        rotation.x_axis.extend(0.0),
        rotation.y_axis.extend(0.0),
        rotation.z_axis.extend(0.0),
        position.extend(1.0),
    )
}
