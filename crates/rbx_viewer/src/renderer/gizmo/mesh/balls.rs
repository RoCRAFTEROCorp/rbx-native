//! The gizmo's balls: Scale's six and the free-drag one at Move's origin.

use glam::Vec3;

use super::{push, Vertex, BALL_BANDS, BALL_SEGMENTS, BALL_VERTICES};
use crate::gizmo::{Axis, Faces, Handles, ORIGIN_RADIUS};

/// The free-drag ball's colour: a light neutral grey, already linearized
/// (from sRGB `0.92`), so it reads as no axis's own. A colour of this
/// editor's choosing — the ball is its own addition, not Studio's.
pub(super) const ORIGIN_COLOR: [f32; 3] = [0.83, 0.83, 0.83];

/// The free-drag ball at the gizmo's origin (see `crate::gizmo::origin`).
pub(super) fn origin_ball(vertices: &mut Vec<Vertex>, handles: &Handles) {
    ball(
        vertices,
        handles.origin(),
        ORIGIN_RADIUS * handles.arm(),
        ORIGIN_COLOR,
    );
}

/// The six Scale balls, furthest from `eye` first.
///
/// Sorted by ball for the same reason [`arms`] sorts by arm: six convex pieces
/// that only meet when a part is small enough for opposite faces to touch, so
/// a painter's order over them is exact wherever it matters.
pub(super) fn balls(faces: &Faces, eye: Vec3, shown: impl Fn(Axis, f32) -> bool) -> Vec<Vertex> {
    let mut order: Vec<(f32, Axis, f32)> = faces
        .all()
        .filter(|&(axis, sign)| shown(axis, sign))
        .map(|(axis, sign)| ((faces.handle(axis, sign) - eye).length(), axis, sign))
        .collect();
    order.sort_by(|(a, ..), (b, ..)| b.total_cmp(a));

    let mut vertices = Vec::with_capacity(BALL_VERTICES);
    for (_, axis, sign) in order {
        ball(
            &mut vertices,
            faces.handle(axis, sign),
            faces.radius(axis, sign),
            axis.color(),
        );
    }
    vertices
}

/// One Scale handle: a ball centred on the middle of the face it resizes.
///
/// `creator-docs` states the shape for the engine's own resize handles —
/// `Enum.HandlesStyle.Resize` "renders `Class.Handles` as sphere shapes for
/// resizing an adornee along its face axes" — but publishes no proportions for
/// the Studio tool's, so the size ([`crate::gizmo::Faces::radius`]) is chosen
/// to read at the weight of a Move arrowhead rather than measured from
/// anything.
///
/// Swept about the world's own Y: a ball has no orientation for the part's
/// frame to disagree with, so there is nothing to build it from the face's
/// normal for.
pub(super) fn ball(vertices: &mut Vec<Vertex>, centre: Vec3, radius: f32, color: [f32; 3]) {
    let point = |band: usize, segment: usize| {
        let down = std::f32::consts::PI * band as f32 / BALL_BANDS as f32;
        let round = std::f32::consts::TAU * segment as f32 / BALL_SEGMENTS as f32;
        let (sine, cosine) = (down.sin(), down.cos());
        centre + Vec3::new(sine * round.cos(), cosine, sine * round.sin()) * radius
    };

    for band in 0..BALL_BANDS {
        for segment in 0..BALL_SEGMENTS {
            let (a, b) = (point(band, segment), point(band, segment + 1));
            let (c, d) = (point(band + 1, segment + 1), point(band + 1, segment));
            // Walking round the sphere and then down it comes out wound
            // outwards. At each pole one of the two rings has collapsed to a
            // point, and the triangle that would be built from it is
            // degenerate — so the band there is the other triangle alone.
            if band > 0 {
                for corner in [a, b, c] {
                    push(vertices, corner, color);
                }
            }
            if band + 1 < BALL_BANDS {
                for corner in [a, c, d] {
                    push(vertices, corner, color);
                }
            }
        }
    }
}
