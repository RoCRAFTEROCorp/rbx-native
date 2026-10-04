use glam::{Mat3, Mat4, Quat, Vec3};
use rbx_viewer::gizmo::{basis, Faces};
use rbx_viewer::pick::Ray;
use rbx_viewer::Pose;

use super::*;

const EYE: Vec3 = Vec3::new(0.0, 0.0, 10.0);

fn toward(point: Vec3) -> Ray {
    Ray {
        origin: EYE,
        direction: (point - EYE).normalize(),
    }
}

/// A pivot at the origin turned a quarter about +Y, and its handles one
/// stud long, in the world's axes so the test can aim at them.
fn pivot() -> Mat4 {
    Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2)
}

fn handles() -> Handles {
    Handles::new(Vec3::ZERO, basis(None), 1.0)
}

fn no_grid() -> Landing<'static> {
    Landing {
        grid: 0.0,
        angle: 0.0,
        snaps: &[],
    }
}

/// A 4-stud cube at the origin, seen from [`EYE`].
fn cube() -> Faces {
    let pose = Pose {
        position: EYE,
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 25.0,
    };
    let model = Mat4::from_scale_rotation_translation(Vec3::splat(4.0), Quat::IDENTITY, Vec3::ZERO);
    Faces::new(model, pose, false)
}

#[test]
fn the_ball_at_the_pivot_opens_a_free_drag_of_the_pivot_itself() {
    let drag = grab(&handles(), pivot(), toward(Vec3::ZERO));
    assert_eq!(
        drag,
        Some(Drag::Plane {
            point: Vec3::ZERO,
            // Square to the view, facing the camera.
            normal: Vec3::Z,
            offset: Vec3::ZERO,
        })
    );
}

#[test]
fn an_arrow_slides_the_pivot_along_its_axis() {
    let drag = grab(&handles(), pivot(), toward(Vec3::new(0.5, 0.0, 0.0))).unwrap();
    let Drag::Axis { axis, .. } = drag else {
        panic!("{drag:?}");
    };
    assert_eq!(axis, Vec3::X);

    let (_, change) = advance(drag, toward(Vec3::new(3.0, 0.0, 0.0)), no_grid()).unwrap();
    let (moved, snapped) = stepped(pivot(), change, None, toward(Vec3::ZERO));
    assert!((moved.w_axis.truncate() - Vec3::new(2.5, 0.0, 0.0)).length() < 1e-3);
    // Only where it stands: the pivot keeps its turn.
    assert_eq!(Mat3::from_mat4(moved), Mat3::from_mat4(pivot()));
    assert_eq!(snapped, None);
}

#[test]
fn a_ring_turns_the_pivot_and_leaves_it_standing() {
    let on_ring = Vec3::new(1.0, 1.0, 0.0).normalize();
    let drag = grab(&handles(), pivot(), toward(on_ring)).unwrap();
    let Drag::Ring { orientation, .. } = drag else {
        panic!("{drag:?}");
    };
    assert_eq!(
        orientation,
        Mat3::from_mat4(pivot()),
        "turned from where it is"
    );

    let quarter = Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let change = Change::Orientation(quarter);
    let (moved, _) = stepped(pivot(), change, None, toward(Vec3::ZERO));
    assert_eq!(Mat3::from_mat4(moved), quarter);
    assert_eq!(moved.w_axis, pivot().w_axis);
}

#[test]
fn a_free_drag_with_snap_lands_on_the_nearest_hotspot() {
    let cube = cube();
    let near_corner = toward(Vec3::new(2.05, 1.95, 2.0));
    let change = Change::Position(Vec3::new(2.05, 1.95, 0.0));

    let (moved, snapped) = stepped(pivot(), change, Some(&cube), near_corner);
    assert_eq!(snapped, Some(Vec3::splat(2.0)));
    assert_eq!(moved.w_axis.truncate(), Vec3::splat(2.0));

    // Snap off (no box handed in): wherever the cursor put it.
    let (moved, snapped) = stepped(pivot(), change, None, near_corner);
    assert_eq!(snapped, None);
    assert_eq!(moved.w_axis.truncate(), Vec3::new(2.05, 1.95, 0.0));
}

#[test]
fn a_free_drag_away_from_every_hotspot_goes_where_the_cursor_is() {
    let between = toward(Vec3::new(1.0, 0.0, 2.0));
    let change = Change::Position(Vec3::new(1.0, 0.0, 0.0));
    let (moved, snapped) = stepped(pivot(), change, Some(&cube()), between);
    assert_eq!(snapped, None);
    assert_eq!(moved.w_axis.truncate(), Vec3::new(1.0, 0.0, 0.0));
}
