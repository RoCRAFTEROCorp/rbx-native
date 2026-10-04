use glam::{Mat4, Vec2, Vec3};
use rbx_viewer::gizmo;
use rbx_viewer::pick::{Ray, Solid};

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
    assert_eq!(onto_edges(&top(TargetKind::Polygon), hit, 0.5), None);
}

#[test]
fn a_point_near_one_edge_snaps_onto_that_edge() {
    let frame = top(TargetKind::Polygon);
    assert_eq!(
        onto_edges(&frame, Vec3::new(3.7, 1.0, 3.0), 0.5),
        Some(Vec3::new(4.0, 1.0, 3.0))
    );
    assert_eq!(
        onto_edges(&frame, Vec3::new(2.0, 1.0, 0.2), 0.5),
        Some(Vec3::new(2.0, 1.0, 0.0))
    );
}

#[test]
fn a_point_near_a_corner_snaps_onto_the_vertex() {
    assert_eq!(
        onto_edges(&top(TargetKind::Polygon), Vec3::new(0.3, 1.0, 5.8), 0.5),
        Some(Vec3::new(0.0, 1.0, 6.0))
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
        Some(Vec3::new(-4.0, 1.0, 3.0))
    );
}

#[test]
fn a_curved_surface_has_no_edges_to_snap_to() {
    let hit = Vec3::new(0.1, 1.0, 0.1);
    for kind in [TargetKind::Sphere, TargetKind::Cylinder, TargetKind::Round] {
        assert_eq!(onto_edges(&top(kind), hit, 0.5), None);
    }
}

fn close(a: Option<Vec3>, b: Vec3) -> bool {
    a.is_some_and(|a| (a - b).length() < 1e-3)
}

/// A face of `solid` with `normal`, `x` one direction in it, the frame's
/// corner and size left as the box's (which `onto_edges` must not trust).
fn on(solid: Solid, model: Mat4, normal: Vec3, x: Vec3) -> SurfaceFrame {
    SurfaceFrame {
        corner: Vec3::ZERO,
        x,
        y: normal,
        z: x.cross(normal),
        size: Vec2::new(4.0, 4.0),
        kind: TargetKind::Polygon,
        part: Some((solid, model)),
    }
}

/// A 4 × 2 × 4 wedge at the origin: its slope climbs from the bottom edge at
/// z = -2 to the top one at z = 2, and its sides are triangles.
fn wedge() -> Mat4 {
    Mat4::from_scale(Vec3::new(4.0, 2.0, 4.0))
}

#[test]
fn a_wedges_slope_snaps_onto_its_own_edges_and_corners() {
    let normal = Vec3::new(0.0, 4.0, -2.0).normalize();
    let frame = on(Solid::Wedge, wedge(), normal, Vec3::X);
    // Up the slope by its own direction: (x, -1 + 2t, -2 + 4t).
    assert!(close(
        onto_edges(&frame, Vec3::new(1.8, 0.9, 1.8), 0.5),
        Vec3::new(2.0, 1.0, 2.0)
    ));
    assert!(close(
        onto_edges(&frame, Vec3::new(1.8, 0.0, 0.0), 0.5),
        Vec3::new(2.0, 0.0, 0.0)
    ));
    assert_eq!(onto_edges(&frame, Vec3::new(0.0, 0.0, 0.0), 0.5), None);
}

/// The side's frame measures the whole 4 × 2 rectangle round the triangle;
/// a point near the empty half's top edge snaps onto the slope's edge
/// instead of up into thin air.
#[test]
fn a_wedges_triangular_side_snaps_onto_the_triangle_not_its_box() {
    let frame = on(Solid::Wedge, wedge(), Vec3::X, Vec3::Z);
    let on_triangle = |point: Vec3| (point.x - 2.0).abs() < 1e-3 && point.y <= point.z / 2.0 + 1e-3;

    let snapped = onto_edges(&frame, Vec3::new(2.0, 0.6, 1.6), 0.5);
    assert!(close(snapped, Vec3::new(2.0, 0.76, 1.52)), "{snapped:?}");
    assert!(on_triangle(snapped.unwrap()));

    let snapped = onto_edges(&frame, Vec3::new(2.0, -0.2, 0.1), 0.5);
    assert!(close(snapped, Vec3::new(2.0, 0.0, 0.0)), "{snapped:?}");
}

/// An 8-long cylinder of radius 2: its +X cap's frame is a 2-stud square
/// whose far corner is off the disc; the snap goes onto the rim.
#[test]
fn a_cylinders_cap_snaps_onto_its_rim() {
    let model = Mat4::from_scale(Vec3::new(8.0, 4.0, 4.0));
    let frame = SurfaceFrame {
        corner: Vec3::new(4.0, 2.0, 0.0),
        ..on(Solid::Cylinder, model, Vec3::X, Vec3::Z)
    };
    let snapped = onto_edges(&frame, Vec3::new(4.0, 1.3, 1.3), 0.5);
    let rim = 2.0f32.sqrt();
    assert!(close(snapped, Vec3::new(4.0, rim, rim)), "{snapped:?}");
    assert_eq!(onto_edges(&frame, Vec3::new(4.0, 0.1, 0.1), 0.5), None);
    // Near the square's far corner from (4, 2, 0) but off the cap's edge.
    assert_eq!(onto_edges(&frame, Vec3::new(4.0, 0.3, 0.9), 0.5), None);
}

#[test]
fn a_balls_pole_has_nothing_to_snap_to() {
    let frame = on(
        Solid::Ball,
        Mat4::from_scale(Vec3::splat(4.0)),
        Vec3::Y,
        Vec3::X,
    );
    assert_eq!(onto_edges(&frame, Vec3::new(0.0, 2.0, 0.0), 0.5), None);
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

/// What `summon_handles` relies on to place the handles on a fresh hover: an
/// event emitted, then a callback deferred, from the same handler — the
/// subscriber (`Shell` answering the hover) runs before the deferred
/// placement does, so the placement reads the new answer, not the stale one.
#[gpui_kit::test]
fn a_hover_asked_for_is_answered_before_the_deferred_placement(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext as _;

    struct View {
        hover: u32,
        placed_on: Option<u32>,
    }
    struct Hover;
    impl gpui_kit::EventEmitter<Hover> for View {}

    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        let view = cx.new(|_| View {
            hover: 0,
            placed_on: None,
        });
        cx.subscribe(&view, |view, _: &Hover, cx| {
            view.update(cx, |view, _| view.hover += 1);
        })
        .detach();
        view.update(cx, |_, cx| {
            cx.emit(Hover);
            cx.defer_in(window, |view, _, _| view.placed_on = Some(view.hover));
        });
        view
    });
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| view.read(cx).placed_on), Some(1));
}
