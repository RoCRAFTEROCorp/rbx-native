use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::transform;

const QUARTER: f32 = std::f32::consts::FRAC_PI_2;

fn placed(turn: Mat3, at: Vec3) -> CFrameData {
    let mut placement = Mat4::from_mat3(turn);
    placement.w_axis = at.extend(1.0);
    transform::cframe(placement)
}

fn v3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn part(dom: &mut WeakDom, shape: u32, turn: Mat3, size: Variant) -> Ref {
    let part = dom.new_instance("Part", "Plank", None);
    let _ = dom.set_property(part, "shape", Variant::Enum(shape));
    let _ = dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(placed(turn, Vec3::new(3.0, 4.0, 5.0))),
    );
    let _ = dom.set_property(part, "size", size);
    part
}

fn get<'a>(dom: &'a WeakDom, referent: Ref, key: &str) -> &'a Variant {
    dom.get(referent).unwrap().properties().get(key).unwrap()
}

/// Every corner of a `size` box placed by `frame`, in world space, sorted
/// so two boxes covering the same space compare equal.
fn corners(frame: &CFrameData, size: Vec3) -> Vec<[i32; 3]> {
    let mut out: Vec<[i32; 3]> = (0..8)
        .map(|i| {
            let local = Vec3::new(
                if i & 1 == 0 { -0.5 } else { 0.5 },
                if i & 2 == 0 { -0.5 } else { 0.5 },
                if i & 4 == 0 { -0.5 } else { 0.5 },
            ) * size;
            let world = transform::rigid(frame).transform_point3(local);
            (world * 1000.0).round().as_ivec3().to_array()
        })
        .collect();
    out.sort();
    out
}

#[test]
fn normal_id_has_the_documented_values() {
    let database = ReflectionDatabase::embedded();
    for (value, name) in ["Right", "Top", "Back", "Left", "Bottom", "Front"]
        .iter()
        .enumerate()
    {
        assert_eq!(database.enum_name("NormalId", value as u32), Some(*name));
        assert_eq!(SURFACES[value], format!("{name}Surface"));
    }
    assert_eq!(database.enum_name("PartType", 1), Some("Block"));
    assert_eq!(database.enum_name("PartType", 0), Some("Ball"));
}

#[test]
fn a_quarter_turned_block_covers_the_same_space_unturned() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let turn = Mat3::from_rotation_y(QUARTER) * Mat3::from_rotation_x(-QUARTER);
    let block = part(&mut dom, 1, turn, v3(4.0, 1.0, 2.0));
    let Variant::CFrame(old) = *get(&dom, block, "CFrame") else {
        panic!()
    };
    let before = corners(&old, Vec3::new(4.0, 1.0, 2.0));

    freeze_local(&mut dom, &database, block).unwrap();

    let Variant::CFrame(new) = *get(&dom, block, "CFrame") else {
        panic!()
    };
    assert_eq!(new.rotation, IDENTITY);
    let Variant::Vector3(size) = *get(&dom, block, "size") else {
        panic!()
    };
    assert!(dom.get(block).unwrap().properties().get("Size").is_none());
    assert_eq!(corners(&new, Vec3::new(size.x, size.y, size.z)), before);
    assert!(!freezable(&dom, &database, block), "frozen means unturned");
}

#[test]
fn surfaces_and_decals_follow_their_faces() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    // A quarter turn about X: the part's top now faces the world's back.
    let block = part(
        &mut dom,
        1,
        Mat3::from_rotation_x(QUARTER),
        v3(2.0, 2.0, 2.0),
    );
    let studs = Variant::Enum(3);
    let _ = dom.set_property(block, "TopSurface", studs.clone());
    let _ = dom.set_property(block, "BottomSurface", Variant::Enum(0));
    let decal = dom.new_instance("Decal", "Sign", Some(block));
    let _ = dom.set_property(decal, "Face", Variant::Enum(1));

    let notes = freeze_local(&mut dom, &database, block).unwrap();

    assert_eq!(get(&dom, block, "BackSurface"), &studs);
    assert_eq!(
        get(&dom, block, "TopSurface"),
        &Variant::Enum(0),
        "the old front"
    );
    assert_eq!(
        get(&dom, decal, "Face"),
        &Variant::Enum(2),
        "Top became Back"
    );
    assert!(notes.iter().any(|n| n.contains("Sign")), "{notes:?}");
}

#[test]
fn a_patterned_material_is_flagged() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let block = part(
        &mut dom,
        1,
        Mat3::from_rotation_z(QUARTER),
        v3(1.0, 6.0, 1.0),
    );
    let wood = database
        .enum_items("Material")
        .unwrap()
        .iter()
        .find(|(name, _)| name == "Wood")
        .unwrap()
        .1;
    let _ = dom.set_property(block, "Material", Variant::Enum(wood));
    let notes = freeze_local(&mut dom, &database, block).unwrap();
    assert!(notes.iter().any(|n| n.contains("Wood")), "{notes:?}");
}

#[test]
fn a_block_at_any_other_angle_is_refused() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let block = part(&mut dom, 1, Mat3::from_rotation_y(0.3), v3(4.0, 1.0, 2.0));
    assert!(!freezable(&dom, &database, block));
    assert!(freeze_local(&mut dom, &database, block).is_err());
}

#[test]
fn a_ball_freezes_at_any_angle_unless_it_wears_a_decal() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let ball = part(&mut dom, 0, Mat3::from_rotation_y(0.3), v3(2.0, 2.0, 2.0));
    assert!(freezable(&dom, &database, ball));
    let decal = dom.new_instance("Decal", "Face", Some(ball));
    let _ = dom.set_property(decal, "Face", Variant::Enum(5));
    assert!(!freezable(&dom, &database, ball));
    dom.remove(decal);
    freeze_local(&mut dom, &database, ball).unwrap();
    let Variant::CFrame(new) = *get(&dom, ball, "CFrame") else {
        panic!()
    };
    assert_eq!(new.rotation, IDENTITY);
    assert_eq!(get(&dom, ball, "size"), &v3(2.0, 2.0, 2.0));
}

#[test]
fn a_models_pivot_loses_its_turn_and_its_parts_stay() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "House", None);
    let wall = part(&mut dom, 1, Mat3::from_rotation_y(0.3), v3(4.0, 1.0, 2.0));
    dom.set_parent(wall, Some(model));
    let pivot = placed(Mat3::from_rotation_y(0.7), Vec3::new(1.0, 2.0, 3.0));
    let _ = dom.set_property(model, "WorldPivot", Variant::CFrame(pivot));
    let wall_before = get(&dom, wall, "CFrame").clone();

    freeze_local(&mut dom, &database, model).unwrap();

    let after = rbx_lua::pivot::pivot(&dom, &database, model).unwrap();
    assert_eq!(after.rotation, IDENTITY);
    assert_eq!(after.position, pivot.position);
    assert_eq!(get(&dom, wall, "CFrame"), &wall_before);
    assert!(!freezable(&dom, &database, model));
}

#[test]
fn a_model_with_a_primary_part_moves_that_parts_pivot() {
    let database = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Car", None);
    let body = part(&mut dom, 1, Mat3::from_rotation_y(0.3), v3(4.0, 1.0, 2.0));
    dom.set_parent(body, Some(model));
    let _ = dom.set_property(model, "PrimaryPart", Variant::Ref(body));
    let before = get(&dom, body, "CFrame").clone();

    freeze_local(&mut dom, &database, model).unwrap();

    let turn = rbx_lua::pivot::pivot(&dom, &database, model)
        .unwrap()
        .rotation;
    assert!(
        turn.iter().zip(IDENTITY).all(|(a, b)| (a - b).abs() < 1e-5),
        "{turn:?}"
    );
    // Float noise from composing the part's frame with its offset only.
    assert!(!freezable(&dom, &database, model));
    assert_eq!(
        get(&dom, body, "CFrame"),
        &before,
        "the part itself never moves"
    );
}
