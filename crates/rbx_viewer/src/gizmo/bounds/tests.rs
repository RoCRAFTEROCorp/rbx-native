use glam::{Mat3, Mat4, Vec3};

use super::*;

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

/// Two 2-stud cubes, centred at the origin and six studs along X.
fn pair() -> [Mat4; 2] {
    let cube = |at: Vec3| Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(2.0));
    [cube(Vec3::ZERO), cube(Vec3::new(6.0, 0.0, 0.0))]
}

/// A pivot standing at `at`, turned an eighth about Y.
fn turned_pivot(at: Vec3) -> Mat4 {
    Mat4::from_translation(at) * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4)
}

#[test]
fn a_local_box_is_squared_to_the_pivot_s_own_axes() {
    let pivot = turned_pivot(Vec3::new(3.0, 0.0, 0.0));
    let local = scale_box(pair(), Some(pivot), true).unwrap();
    let axes = Mat3::from_mat4(pivot);
    // Each column runs along the pivot's own axis, whatever its length.
    for column in 0..3 {
        let along = local.col(column).truncate().normalize();
        assert!(close(along, axes.col(column)), "{column}: {along}");
    }
    assert_eq!(
        local,
        bounds_along(pair(), axes).unwrap(),
        "a pivot inside the parts' box grows nothing"
    );
}

#[test]
fn a_world_box_stands_square_to_the_world_round_the_parts() {
    let pivot = turned_pivot(Vec3::new(3.0, 0.0, 0.0));
    let world = scale_box(pair(), Some(pivot), false).unwrap();
    // x from -1 to 7, y and z from -1 to 1: the pivot's turn plays no part.
    assert!(close(world.w_axis.truncate(), Vec3::new(3.0, 0.0, 0.0)));
    assert!(close(world.x_axis.truncate(), Vec3::X * 8.0));
    assert!(close(world.y_axis.truncate(), Vec3::Y * 2.0));
    assert!(close(world.z_axis.truncate(), Vec3::Z * 2.0));
}

#[test]
fn the_box_grows_to_hold_a_pivot_standing_outside_the_parts() {
    let outside = Vec3::new(3.0, 5.0, 0.0);
    let pivot = Mat4::from_translation(outside);
    let world = scale_box(pair(), Some(pivot), false).unwrap();
    // y now runs from -1 up to the pivot's 5: 6 tall, centred at 2.
    assert!(close(world.w_axis.truncate(), Vec3::new(3.0, 2.0, 0.0)));
    assert!(close(world.y_axis.truncate(), Vec3::Y * 6.0));
    // The pivot is on the box's top face.
    let local = world.inverse().transform_point3(outside);
    assert!((local.y - 0.5).abs() < 1e-5, "{local}");

    // Turned, the pivot still ends up on or inside the box.
    let turned = turned_pivot(Vec3::new(-4.0, 3.0, 2.0));
    let local = scale_box(pair(), Some(turned), true).unwrap();
    let inside = local.inverse().transform_point3(turned.w_axis.truncate());
    assert!(inside.abs().max_element() <= 0.5 + 1e-5, "{inside}");
}

#[test]
fn a_lone_part_keeps_its_own_box_and_several_get_the_world_s() {
    let part = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
        * Mat4::from_rotation_y(0.7)
        * Mat4::from_scale(Vec3::new(2.0, 4.0, 6.0));
    // No pivot is how a lone part is asked for: its `PivotOffset` plays no
    // part in the box, local or not.
    assert_eq!(scale_box([part], None, true), Some(part));
    assert_eq!(scale_box([part], None, false), Some(part));
    assert_eq!(
        scale_box(pair(), None, true),
        bounds_along(pair(), Mat3::IDENTITY)
    );
    assert_eq!(scale_box([], Some(Mat4::IDENTITY), true), None);
}

#[test]
fn a_one_part_model_is_boxed_on_its_pivot_not_on_the_part() {
    let part = Mat4::from_rotation_y(0.3) * Mat4::from_scale(Vec3::splat(2.0));
    let pivot = Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0));
    let boxed = scale_box([part], Some(pivot), true).unwrap();
    assert_ne!(boxed, part);
    assert!(boxed.y_axis.length() > 4.0);
}
