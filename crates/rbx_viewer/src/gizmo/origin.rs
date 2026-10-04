//! The small free-drag ball at the Move gizmo's own origin.
//!
//! Grabbing it is the same gesture as grabbing a selected part's body —
//! the selection follows the cursor and settles onto whatever is under it —
//! held from a fixed point on the gizmo instead of from wherever the click
//! met the part. That matters once `Tab` can summon the handles somewhere
//! the part itself is not (see [`super::Gizmo::summon`]). Studio's own Move
//! gizmo has no such ball; this is this editor's own addition.

use crate::pick::Ray;

use super::{Handles, SHAFT_START};

/// The ball's drawn radius, in arm lengths. Well inside [`SHAFT_START`] so
/// it never reads as part of an arrow.
pub(crate) const ORIGIN_RADIUS: f32 = 0.08;
/// How far off the origin a ray still counts as grabbing the ball, in arm
/// lengths — wider than the ball for the same reason every other handle's
/// pick is, and still short of where an arm's own pick begins.
const ORIGIN_PICK: f32 = 0.11;

const _: () = assert!(ORIGIN_RADIUS < ORIGIN_PICK && ORIGIN_PICK < SHAFT_START);

impl Handles {
    /// Whether `ray` is pointing at the free-drag ball at the gizmo's origin.
    pub fn grab_origin(&self, ray: Ray) -> bool {
        let (distance, offset) = ray.nearest(self.origin);
        distance > 0.0 && offset <= ORIGIN_PICK * self.arm
    }
}
