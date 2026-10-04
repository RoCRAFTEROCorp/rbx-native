//! The boxes a selection occupies: where one gizmo stands for all of it, the
//! box its outline and Edit Pivot's hotspots are drawn on, and the box
//! Scale's balls stand on.

use glam::{Mat3, Mat4, Vec3};

/// Where one gizmo goes for a whole selection: the centre of the world-axis
/// -aligned box that contains every one of `models`, the oriented boxes its
/// parts occupy. `None` for an empty selection.
///
/// The centre of the *bounds*, not the mean of the parts' own centres — those
/// differ as soon as the selection is lopsided (three small parts at one end
/// and one large at the other), and the bounds are what the user sees the
/// selection occupying. `creator-docs` never states where the gizmo sits for a
/// multi-object selection, but it is explicit that this is what Studio means
/// by the centre of an aggregate: the pivot tool's **Reset** "moves the pivot
/// point to the **center** of an object or model's bounding box"
/// (`studio/pivot-tools.md`).
///
/// One part is the same answer as before — its own bounding box is centred on
/// it — so this needs no special case for a single selection.
pub fn centre_of(models: impl IntoIterator<Item = Mat4>) -> Option<Vec3> {
    bounds_of(models).map(|(min, max)| (min + max) * 0.5)
}

/// The box the Scale tool's handles stand on, as the `Mat4` `part_model`
/// would give a box-shaped part of that size at that centre. Shared by the
/// renderer and the editor's hit test for the same reason [`centre_of`] is:
/// two derivations of "where the handles are" are two things that can
/// disagree.
///
/// Follows Studio's own `ScaleDragger` (its `DraggerSchemaCore`
/// `SelectionInfo` and `ExtrudeHandlesImplementation:getBoundingBox`, read
/// from the client's disassembled built-in plugin):
///
/// - A lone part (`pivot` `None`, one model) stands on its own box,
///   `PivotOffset` or not.
/// - A model, or anything else with one `pivot` (rigid, no `Size` in its
///   columns), stands on the box round its parts squared to the pivot's own
///   axes with `local` on, or to the world's anchored on the pivot with it
///   off — then grown until it holds the pivot, so a pivot standing outside
///   the parts still lies on or inside the box the drag scales.
/// - Several things with no one pivot stand on the world-aligned box round
///   them all. Studio squares that one to the first item's pivot with
///   `local` on; this editor keeps no pivot for a selection of several (see
///   `rbxstudio`'s `transform::Targets::pivot`), so it stays world-aligned.
pub fn scale_box(
    models: impl IntoIterator<Item = Mat4>,
    pivot: Option<Mat4>,
    local: bool,
) -> Option<Mat4> {
    let Some(pivot) = pivot else {
        return box_along(models, Mat3::IDENTITY);
    };
    let axes = if local {
        Mat3::from_mat4(pivot)
    } else {
        Mat3::IDENTITY
    };
    let into = Mat4::from_mat3(axes).transpose();
    let (min, max) = bounds_of(models.into_iter().map(|model| into * model))?;
    let at = into.transform_point3(pivot.w_axis.truncate());
    Some(squared(axes, min.min(at), max.max(at)))
}

/// A lone part's own box, or a group's squared to `axes` (a rotation) rather
/// than to the world's — how `Model:GetBoundingBox` squares a model's box to
/// its pivot, and `rbx_lua::pivot::reset` with it. What Edit Pivot's
/// hotspots stand on, so they turn with the pivot.
pub fn box_along(models: impl IntoIterator<Item = Mat4>, axes: Mat3) -> Option<Mat4> {
    let models: Vec<Mat4> = models.into_iter().collect();
    match models.as_slice() {
        [only] => Some(*only),
        many => bounds_along(many.iter().copied(), axes),
    }
}

/// The box round every one of `models` squared to `axes`, even for just one
/// — `Model:GetBoundingBox`, which the selection outline of a model is
/// ("matches the selection box rendered in Studio when the model is
/// selected"). `None` for none.
pub fn bounds_along(models: impl IntoIterator<Item = Mat4>, axes: Mat3) -> Option<Mat4> {
    let into = Mat4::from_mat3(axes).transpose();
    let (min, max) = bounds_of(models.into_iter().map(|model| into * model))?;
    Some(squared(axes, min, max))
}

/// The box from `min` to `max` along `axes`, back in the world.
fn squared(axes: Mat3, min: Vec3, max: Vec3) -> Mat4 {
    Mat4::from_mat3(axes) * Mat4::from_translation((min + max) * 0.5) * Mat4::from_scale(max - min)
}

/// The world-axis-aligned box containing every one of `models`, as its minimum
/// and maximum corner. `None` for an empty selection.
///
/// Split out of [`centre_of`] because a selected `Model` needs the whole box
/// and not just its middle: the outline drawn around a container is exactly
/// this extent, and deriving it a second time somewhere else is how the box
/// the user sees and the point the gizmo stands on start to disagree.
pub fn bounds_of(models: impl IntoIterator<Item = Mat4>) -> Option<(Vec3, Vec3)> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for model in models {
        let centre = model.w_axis.truncate();
        // A box turned off the world axes still has to be contained by them:
        // each world-axis half-extent is the sum of the absolute projections
        // of the three (already `Size`-scaled) columns onto that axis.
        let half = 0.5
            * Vec3::new(
                model.x_axis.x.abs() + model.y_axis.x.abs() + model.z_axis.x.abs(),
                model.x_axis.y.abs() + model.y_axis.y.abs() + model.z_axis.y.abs(),
                model.x_axis.z.abs() + model.y_axis.z.abs() + model.z_axis.z.abs(),
            );
        bounds = Some(match bounds {
            None => (centre - half, centre + half),
            Some((min, max)) => (min.min(centre - half), max.max(centre + half)),
        });
    }
    bounds
}

#[cfg(test)]
#[path = "bounds/tests.rs"]
mod tests;
