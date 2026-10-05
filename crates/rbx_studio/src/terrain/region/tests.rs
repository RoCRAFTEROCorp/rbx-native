use super::*;

fn pose() -> Pose {
    Pose {
        position: Vec3::new(0.0, 0.0, 100.0),
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 50.0,
    }
}

fn region() -> StudBox {
    StudBox::from_center_size([0.0, 0.0, 0.0], [16.0, 16.0, 16.0])
}

fn ray_to(from: Vec3, to: Vec3) -> Ray {
    Ray {
        origin: from,
        direction: (to - from).normalize(),
    }
}

#[test]
fn dragging_a_face_ball_moves_only_that_face() {
    let region = region();
    let (faces, _) = region_handles(&region, Mat3::IDENTITY, pose(), false);
    let ball = faces.handle(Axis::X, 1.0);
    let eye = Vec3::new(0.0, 0.0, 100.0);
    let mut drag = RegionDrag::press(
        &region,
        Mat3::IDENTITY,
        (pose(), false),
        ray_to(eye, ball),
        false,
        Vec3::ZERO,
    );
    assert!(matches!(drag.grab, RegionGrab::Face { axis: Axis::X, .. }));
    let (grown, _) = drag.step(ray_to(eye, ball + Vec3::X * 8.0), false, false, false);
    assert!((grown.max[0] - 16.0).abs() < 1e-3, "{grown:?}");
    assert!((grown.min[0] + 8.0).abs() < 1e-3, "the far face holds");
    let (both, _) = drag.step(ray_to(eye, ball + Vec3::X * 8.0), false, true, false);
    assert!((both.min[0] + 16.0).abs() < 1e-3 && (both.max[0] - 16.0).abs() < 1e-3);
    let (even, _) = drag.step(ray_to(eye, ball + Vec3::X * 8.0), true, false, false);
    assert!(
        (even.size()[1] - 24.0).abs() < 1e-3,
        "shift scales every axis"
    );
}

#[test]
fn pressing_off_the_handles_draws_a_new_region() {
    let region = region();
    let eye = Vec3::new(200.0, 200.0, 200.0);
    let anchor = Vec3::new(100.0, 0.0, 100.0);
    let mut drag = RegionDrag::press(
        &region,
        Mat3::IDENTITY,
        (pose(), false),
        ray_to(eye, anchor),
        false,
        anchor,
    );
    assert_eq!(drag.grab, RegionGrab::Draw { anchor });
    let (drawn, _) = drag.step(
        ray_to(eye, Vec3::new(130.0, 0.0, 110.0)),
        false,
        false,
        true,
    );
    assert_eq!(drawn.min[0], 100.0);
    assert_eq!(drawn.max[0], 132.0, "snapped out to whole voxels");
    assert!(drawn.min[1] < 0.0 && drawn.max[1] > 0.0);
}

#[test]
fn transform_arrows_and_rings_move_and_turn() {
    let region = region();
    let (_, handles) = region_handles(&region, Mat3::IDENTITY, pose(), false);
    let eye = Vec3::new(0.0, 0.0, 100.0);
    let tip = handles.origin() + Vec3::Y * handles.arm() * 0.8;
    let mut drag = RegionDrag::press(
        &region,
        Mat3::IDENTITY,
        (pose(), false),
        ray_to(eye, tip),
        true,
        Vec3::ZERO,
    );
    assert!(matches!(drag.grab, RegionGrab::Arrow { axis: Axis::Y, .. }));
    let (moved, turn) = drag.step(ray_to(eye, tip + Vec3::Y * 10.0), false, false, false);
    assert!((moved.center()[1] - 10.0).abs() < 1e-2);
    assert_eq!(turn, Mat3::IDENTITY);
}
