use glam::{Vec2, Vec3};
use rbx_viewer::gizmo;
use rbx_viewer::pick::Ray;

use super::onto_edges;
use crate::dragger::surface::{SurfaceFrame, TargetKind};

/// The top face of a 4×6 block, cornered at its `(0, 1, 0)` corner, running
/// 4 studs along +X and 6 along +Z.
fn top(kind: TargetKind) -> SurfaceFrame {
    SurfaceFrame {
        corner: Vec3::new(0.0, 1.0, 0.0),
        x: Vec3::X,
        y: Vec3::Y,
        z: Vec3::Z,
        size: Vec2::new(4.0, 6.0),
        kind,
        part: None,
    }
}

#[test]
fn a_point_well_inside_the_face_stays_put() {
    let hit = Vec3::new(2.0, 1.0, 3.0);
    assert_eq!(onto_edges(&top(TargetKind::Polygon), hit, 0.5), hit);
}

#[test]
fn a_point_near_one_edge_snaps_onto_that_edge() {
    let frame = top(TargetKind::Polygon);
    assert_eq!(
        onto_edges(&frame, Vec3::new(3.7, 1.0, 3.0), 0.5),
        Vec3::new(4.0, 1.0, 3.0)
    );
    assert_eq!(
        onto_edges(&frame, Vec3::new(2.0, 1.0, 0.2), 0.5),
        Vec3::new(2.0, 1.0, 0.0)
    );
}

#[test]
fn a_point_near_a_corner_snaps_onto_the_vertex() {
    assert_eq!(
        onto_edges(&top(TargetKind::Polygon), Vec3::new(0.3, 1.0, 5.8), 0.5),
        Vec3::new(0.0, 1.0, 6.0)
    );
}

/// The frame's axes may point out of the face, the far edge then sitting at
/// a negative coordinate.
#[test]
fn an_axis_pointing_out_of_the_face_still_finds_the_far_edge() {
    let frame = SurfaceFrame {
        x: -Vec3::X,
        ..top(TargetKind::Polygon)
    };
    assert_eq!(
        onto_edges(&frame, Vec3::new(-3.8, 1.0, 3.0), 0.5),
        Vec3::new(-4.0, 1.0, 3.0)
    );
}

#[test]
fn a_curved_surface_has_no_edges_to_snap_to() {
    let hit = Vec3::new(0.1, 1.0, 0.1);
    for kind in [TargetKind::Sphere, TargetKind::Cylinder, TargetKind::Round] {
        assert_eq!(onto_edges(&top(kind), hit, 0.5), hit);
    }
}

/// What `measure_from_handle` relies on: a ray moved by the offset between
/// two parallel lines meets the first where the unmoved ray meets the
/// second — and, under perspective, not where it meets the first line
/// itself, which is the error moving it corrects.
#[test]
fn a_moved_ray_measures_against_the_part_as_the_real_one_against_the_handle() {
    let (part, handle, axis) = (Vec3::ZERO, Vec3::new(0.0, 6.0, -9.0), Vec3::X);
    let ray = Ray::new(Vec3::new(3.0, 20.0, 30.0), Vec3::new(-0.1, -0.4, -1.0));
    let moved = Ray {
        origin: ray.origin + (part - handle),
        ..ray
    };

    let on_handle = gizmo::along_axis(handle, axis, ray).unwrap();
    let measured = gizmo::along_axis(part, axis, moved).unwrap();
    let naive = gizmo::along_axis(part, axis, ray).unwrap();
    assert!((on_handle - measured).abs() < 1e-4);
    assert!((on_handle - naive).abs() > 0.1);
}
