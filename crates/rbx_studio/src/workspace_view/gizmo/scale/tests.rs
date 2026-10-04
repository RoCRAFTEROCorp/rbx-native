use glam::Vec3;
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::Pose;

use super::*;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// Looking straight down -Z from thirty studs out.
fn eye() -> Pose {
    Pose {
        position: Vec3::new(0.0, 0.0, 30.0),
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 25.0,
    }
}

fn looking_at(x: f32, y: f32) -> Ray {
    Ray::new(Vec3::new(x, y, 30.0), Vec3::NEG_Z)
}

fn free() -> Landing<'static> {
    Landing {
        grid: 0.0,
        angle: 0.0,
        snaps: &[],
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn cframe(at: Vec3, rotation: [f32; 9]) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: at.x,
            y: at.y,
            z: at.z,
        },
        rotation,
    })
}

/// A 2-stud cube standing at `at`.
fn cube(dom: &mut WeakDom, parent: Option<Ref>, at: Vec3) -> Ref {
    let part = dom.new_instance("Part", "Part", parent);
    let _ = dom.set_property(part, "CFrame", cframe(at, IDENTITY));
    let two = Vector3Data {
        x: 2.0,
        y: 2.0,
        z: 2.0,
    };
    let _ = dom.set_property(part, "size", Variant::Vector3(two));
    part
}

/// A model of two 2-stud cubes, at x = 0 and x = 6 — its parts span x -1..7
/// and y, z -1..1 — with its `WorldPivot` at `pivot`, turned by `rotation`.
fn model(pivot: Vec3, rotation: [f32; 9]) -> Targets {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    cube(&mut dom, Some(model), Vec3::ZERO);
    cube(&mut dom, Some(model), Vec3::new(6.0, 0.0, 0.0));
    let _ = dom.set_property(model, "WorldPivot", cframe(pivot, rotation));
    Targets::read(&dom, &ReflectionDatabase::embedded(), &[model])
}

/// The pivot four studs above the middle of the parts — outside them.
fn raised() -> Targets {
    model(Vec3::new(3.0, 4.0, 0.0), IDENTITY)
}

fn faces(targets: &Targets, local: bool) -> Faces {
    Faces::new(targets.scale_box(local).unwrap(), eye(), false)
}

/// The +X ball of `raised`'s world box, grabbed where it stands.
fn grabbed_x(targets: &Targets, centred: bool) -> Drag {
    let ray = looking_at(7.0, 1.5);
    let pivot = targets.scale_centre(false).unwrap();
    grab_box(&faces(targets, false), ray, pivot, centred).expect("the +X ball")
}

#[test]
fn a_models_box_is_framed_by_its_pivot_and_holds_it() {
    // A quarter turn about Y, row by row: +X goes to -Z.
    let turned = model(
        Vec3::new(3.0, 4.0, 0.0),
        [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
    );
    let pivot = turned.pivot().unwrap();
    let local = turned.scale_box(true).unwrap();
    for column in 0..3 {
        let along = local.col(column).truncate().normalize();
        assert!(close(along, pivot.col(column).truncate()), "{column}");
    }
    let inside = local.inverse().transform_point3(pivot.w_axis.truncate());
    assert!(inside.abs().max_element() <= 0.5 + 1e-5, "{inside}");

    // Square to the world, anchored on the pivot: the parts' box grown up to
    // the pivot's y = 4, so y runs -1..4.
    let world = turned.scale_box(false).unwrap();
    assert!(close(world.w_axis.truncate(), Vec3::new(3.0, 1.5, 0.0)));
    assert!(close(world.x_axis.truncate(), Vec3::X * 8.0));
    assert!(close(world.y_axis.truncate(), Vec3::Y * 5.0));
    assert!(!turned.lone_part());
}

#[test]
fn a_lone_part_scales_on_its_own_box_whatever_its_pivot_offset() {
    let mut dom = WeakDom::new();
    let part = cube(&mut dom, None, Vec3::new(1.0, 2.0, 3.0));
    let offset = cframe(Vec3::new(0.0, 10.0, 0.0), IDENTITY);
    let _ = dom.set_property(part, "PivotOffset", offset);
    let targets = Targets::read(&dom, &ReflectionDatabase::embedded(), &[part]);

    assert!(targets.lone_part());
    assert!(close(
        targets.pivot().unwrap().w_axis.truncate(),
        Vec3::new(1.0, 12.0, 3.0)
    ));
    let own = targets.anchor().unwrap().model;
    assert_eq!(targets.scale_box(true), Some(own));
    assert_eq!(targets.scale_box(false), Some(own));
    // `Ctrl` holds its middle, not the offset pivot.
    assert_eq!(targets.scale_centre(true), Some(Vec3::new(1.0, 2.0, 3.0)));
}

#[test]
fn a_model_of_one_part_still_scales_as_a_model() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    cube(&mut dom, Some(model), Vec3::ZERO);
    let targets = Targets::read(&dom, &ReflectionDatabase::embedded(), &[model]);
    assert!(!targets.lone_part());
}

#[test]
fn pulling_a_face_scales_from_the_opposite_one_carrying_the_pivot() {
    let held = raised();
    let drag = grabbed_x(&held, false);
    // Pulled from x = 7 out to 15: the 8-long box is 16 now.
    let (_, change) = advance(drag, looking_at(15.0, 1.5), free()).unwrap();
    let far = Vec3::new(-1.0, 1.5, 0.0);
    assert_eq!(
        change,
        Change::Scaled {
            pivot: far,
            factor: 2.0
        }
    );

    let mut targets = held.clone();
    targets.scale_about(&held, far, 2.0);
    // The pivot is one more point scaled about the far face.
    let pivot = targets.pivot().unwrap().w_axis.truncate();
    assert!(close(pivot, far + (Vec3::new(3.0, 4.0, 0.0) - far) * 2.0));
    // And the -X face held still.
    let (min, _) = gizmo::bounds_of(targets.iter().map(|target| target.model)).unwrap();
    assert!((min.x + 1.0).abs() < 1e-4);
}

#[test]
fn ctrl_scales_about_the_pivot_twice_as_fast() {
    let held = raised();
    let drag = grabbed_x(&held, true);
    // Two studs of pull is four of growth, the box's two ends moving
    // together: 12 over 8.
    let (_, change) = advance(drag, looking_at(9.0, 1.5), free()).unwrap();
    let pivot = Vec3::new(3.0, 4.0, 0.0);
    assert_eq!(change, Change::Scaled { pivot, factor: 1.5 });

    let mut targets = held.clone();
    targets.scale_about(&held, pivot, 1.5);
    assert!(close(targets.pivot().unwrap().w_axis.truncate(), pivot));
}

#[test]
fn ctrl_resizes_a_lone_part_about_its_middle() {
    let drag = Drag::Size {
        origin: Vec3::ZERO,
        axis: Vec3::X,
        grabbed: 1.0,
        size: Vec3::new(2.0, 1.0, 4.0),
        component: 0,
        sphere: false,
        cylinder: false,
        centred: true,
    };
    let (_, change) = advance(drag, looking_at(2.0, 0.0), free()).unwrap();
    assert_eq!(
        change,
        Change::Size {
            size: Vec3::new(4.0, 1.0, 4.0),
            position: Vec3::ZERO,
        }
    );
}

#[test]
fn letting_go_of_ctrl_mid_drag_carries_on_from_where_the_selection_stands() {
    let held = raised();
    let drag = grabbed_x(&held, false);
    let ray = looking_at(15.0, 1.5);
    let (drag, _) = advance(drag, ray, free()).unwrap();
    let far = Vec3::new(-1.0, 1.5, 0.0);
    let mut targets = held.clone();
    targets.scale_about(&held, far, 2.0);

    // `Ctrl` goes down with the cursor where it was: measured afresh from
    // the doubled box, the same ray is no change at all...
    let local = false;
    let again = regrab(drag, &targets, &faces(&targets, local), local, ray, true).unwrap();
    let pivot = targets.pivot().unwrap().w_axis.truncate();
    let (_, change) = advance(again, ray, free()).unwrap();
    assert_eq!(change, Change::Scaled { pivot, factor: 1.0 });
    // ...and a further stud of pull grows the 16-long box by two, about the
    // pivot where the first half of the drag left it.
    let (_, change) = advance(again, looking_at(16.0, 1.5), free()).unwrap();
    assert_eq!(
        change,
        Change::Scaled {
            pivot,
            factor: 18.0 / 16.0
        }
    );
}

#[test]
fn a_lone_part_regrabbed_keeps_its_face_and_its_new_size() {
    let mut dom = WeakDom::new();
    let part = cube(&mut dom, None, Vec3::new(1.0, 0.0, 0.0));
    let targets = Targets::read(&dom, &ReflectionDatabase::embedded(), &[part]);
    let ray = looking_at(2.0, 0.0);
    let local = true;
    let faces = faces(&targets, local);
    let drag = grab_face(&faces, targets.anchor().unwrap(), ray, false, false).unwrap();
    let again = regrab(drag, &targets, &faces, local, ray, true).unwrap();
    let Drag::Size {
        origin,
        component,
        size,
        centred,
        ..
    } = again
    else {
        panic!("still a resize: {again:?}");
    };
    assert_eq!(
        (origin, component, size, centred),
        (Vec3::X, 0, Vec3::splat(2.0), true)
    );
    // The ray it was regrabbed at is no change.
    let (_, change) = advance(again, ray, free()).unwrap();
    assert_eq!(
        change,
        Change::Size {
            size,
            position: origin
        }
    );
}
