use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_mesh::{Aabb, Mesh, Vertex};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::transform;

/// A box 2 × 1 × 1 studs, as its eight corners, plus a vertex no triangle
/// uses (a coarser LOD's, say) far outside it.
fn bar() -> Mesh {
    let mut vertices: Vec<Vertex> = (0..8)
        .map(|i| Vertex {
            position: [
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -0.5 } else { 0.5 },
                if i & 4 == 0 { -0.5 } else { 0.5 },
            ],
            normal: [1.0, 0.0, 0.0],
            uv: [i as f32 / 8.0, 0.0],
            color: [255; 4],
        })
        .collect();
    vertices.push(Vertex {
        position: [50.0, 0.0, 0.0],
        ..vertices[0]
    });
    Mesh {
        version: (2, 0),
        vertices,
        indices: vec![0, 1, 2, 1, 3, 2, 4, 5, 6, 5, 7, 6, 0, 4, 1, 4, 5, 1],
        lods: Vec::new(),
        bounds: Aabb {
            min: [-1.0, -0.5, -0.5],
            max: [50.0, 0.5, 0.5],
        },
    }
}

fn placed(turn: Mat3, at: Vec3) -> CFrameData {
    let mut placement = Mat4::from_mat3(turn);
    placement.w_axis = at.extend(1.0);
    transform::cframe(placement)
}

fn v3(v: Vec3) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v.x,
        y: v.y,
        z: v.z,
    })
}

/// A `MeshPart` drawing [`bar`] stretched to 4 × 2 × 2 and turned a quarter
/// about Y, a third about X, at (10, 5, -3).
fn scene() -> (WeakDom, Ref, CFrameData) {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("MeshPart", "Rock", None);
    let turn = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2)
        * Mat3::from_rotation_x(std::f32::consts::FRAC_PI_3);
    let old = placed(turn, Vec3::new(10.0, 5.0, -3.0));
    let _ = dom.set_property(part, "CFrame", Variant::CFrame(old));
    let _ = dom.set_property(part, "size", v3(Vec3::new(4.0, 2.0, 2.0)));
    let _ = dom.set_property(part, "InitialSize", v3(Vec3::new(2.0, 1.0, 1.0)));
    let _ = dom.set_property(
        part,
        "MeshId",
        Variant::Content(rbx_dom::Content::Uri("rbxassetid://123".into())),
    );
    (dom, part, old)
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn every_vertex_stays_where_it_was_drawn() {
    let (dom, part, old) = scene();
    let database = ReflectionDatabase::embedded();
    let mesh = bar();
    let plan = plan(&dom, &database, part, &mesh).unwrap();

    let new = transform::rigid(&plan.frame);
    assert!(close(Mat3::from_mat4(new) * Vec3::X, Vec3::X), "unturned");
    let baked = &plan.export.meshes[0];
    assert_eq!(baked.positions.len(), 8, "the unused vertex is dropped");
    let drawn = transform::rigid(&old) * Mat4::from_scale(Vec3::splat(2.0));
    for (index, corner) in mesh.vertices[..8].iter().enumerate() {
        let before = drawn.transform_point3(Vec3::from(corner.position));
        let after = new.transform_point3(Vec3::from(baked.positions[index]));
        assert!(close(before, after), "{before} vs {after}");
    }
    // The stretch is in the vertices now; nothing scales them further.
    let extent = baked
        .positions
        .iter()
        .fold(Vec3::ZERO, |m, p| m.max(Vec3::from(*p).abs()));
    assert!(close(plan.size, extent * 2.0));
}

#[test]
fn normals_turn_with_the_mesh() {
    let (dom, part, old) = scene();
    let plan = plan(&dom, &ReflectionDatabase::embedded(), part, &bar()).unwrap();
    let expected = Mat3::from_mat4(transform::rigid(&old)) * Vec3::X;
    assert!(close(
        Vec3::from(plan.export.meshes[0].normals[0]),
        expected
    ));
}

#[test]
fn what_hangs_off_the_part_keeps_its_place_in_the_world() {
    let (mut dom, part, old) = scene();
    let database = ReflectionDatabase::embedded();
    let offset = placed(Mat3::from_rotation_z(0.4), Vec3::new(1.0, 2.0, 3.0));
    let _ = dom.set_property(part, "PivotOffset", Variant::CFrame(offset));
    let attachment = dom.new_instance("Attachment", "Grip", Some(part));
    let _ = dom.set_property(attachment, "CFrame", Variant::CFrame(offset));
    let other = dom.new_instance("Part", "Other", None);
    let weld = dom.new_instance("Weld", "Weld", Some(other));
    let _ = dom.set_property(weld, "Part1", Variant::Ref(part));
    let _ = dom.set_property(weld, "C1", Variant::CFrame(offset));

    let plan = plan(&dom, &database, part, &bar()).unwrap();
    apply(&mut dom, &database, &plan, 999, Vec3::new(0.3, 0.6, 0.9)).unwrap();

    let world =
        |frame: &CFrameData, on: &CFrameData| transform::rigid(frame) * transform::rigid(on);
    let before = world(&old, &offset);
    let new = match dom.get(part).unwrap().properties().get("CFrame") {
        Some(Variant::CFrame(f)) => *f,
        other => panic!("{other:?}"),
    };
    for (owner, key) in [(attachment, "CFrame"), (weld, "C1")] {
        let Some(Variant::CFrame(local)) = dom.get(owner).unwrap().properties().get(key) else {
            panic!("{key} not written");
        };
        let after = world(&new, local);
        assert!(
            after.abs_diff_eq(before, 1e-4),
            "{key}: {before} vs {after}"
        );
    }
    // The pivot is the origin Blender's Apply clears: it keeps its place
    // and loses its turn.
    let Some(Variant::CFrame(local)) = dom.get(part).unwrap().properties().get("PivotOffset")
    else {
        panic!("PivotOffset not written");
    };
    let pivot = world(&new, local);
    assert!(close(pivot.w_axis.truncate(), before.w_axis.truncate()));
    assert!(Mat3::from_mat4(pivot).abs_diff_eq(Mat3::IDENTITY, 1e-5));
}

#[test]
fn an_unset_pivot_stays_on_the_old_centre() {
    let (dom, part, old) = scene();
    let database = ReflectionDatabase::embedded();
    let mut dom = dom;
    let plan = plan(&dom, &database, part, &bar()).unwrap();
    apply(&mut dom, &database, &plan, 1, Vec3::ONE).unwrap();
    let pivot = rbx_lua::pivot::pivot(&dom, &database, part).unwrap();
    assert!(close(
        Vec3::new(pivot.position.x, pivot.position.y, pivot.position.z),
        Vec3::new(old.position.x, old.position.y, old.position.z)
    ));
    assert_eq!(
        pivot.rotation,
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
    );
}

#[test]
fn apply_points_the_part_at_the_upload() {
    let (mut dom, part, _) = scene();
    let database = ReflectionDatabase::embedded();
    let plan = plan(&dom, &database, part, &bar()).unwrap();
    apply(&mut dom, &database, &plan, 4242, Vec3::new(0.5, 1.0, 1.0)).unwrap();
    let properties = dom.get(part).unwrap().properties();
    assert_eq!(
        properties.get("MeshId"),
        Some(&Variant::Content(rbx_dom::Content::Uri(
            "rbxassetid://4242".into()
        )))
    );
    assert_eq!(
        properties.get("InitialSize"),
        Some(&v3(Vec3::new(0.5, 1.0, 1.0)))
    );
    assert_eq!(properties.get("size"), Some(&v3(plan.size)));
    assert!(
        properties.get("Size").is_none(),
        "the stored spelling is kept"
    );
    assert!(!freezable(&dom, &database, part), "frozen means unturned");
}

#[test]
fn a_turned_meshpart_qualifies_and_a_cylinder_does_not() {
    let database = ReflectionDatabase::embedded();
    let (mut dom, part, _) = scene();
    assert!(freezable(&dom, &database, part));

    let cylinder = dom.new_instance("Part", "Cylinder", None);
    let _ = dom.set_property(cylinder, "Shape", Variant::Enum(2));
    let _ = dom.set_property(
        cylinder,
        "CFrame",
        Variant::CFrame(placed(Mat3::from_rotation_y(1.0), Vec3::ZERO)),
    );
    assert!(!freezable(&dom, &database, cylinder));

    let flat = dom.new_instance("MeshPart", "Flat", None);
    let _ = dom.set_property(flat, "MeshId", Variant::String("rbxassetid://1".into()));
    let _ = dom.set_property(
        flat,
        "CFrame",
        Variant::CFrame(placed(Mat3::IDENTITY, Vec3::ONE)),
    );
    assert!(!freezable(&dom, &database, flat));

    let _ = dom.set_property(part, "MeshId", Variant::String(String::new()));
    assert!(!freezable(&dom, &database, part));
}

#[test]
fn a_skinned_mesh_is_refused() {
    let (mut dom, part, _) = scene();
    dom.new_instance("Bone", "Root", Some(part));
    let err = plan(&dom, &ReflectionDatabase::embedded(), part, &bar()).unwrap_err();
    assert!(err.contains("Bones"), "{err}");
}

#[test]
fn only_a_uniform_rescale_counts_as_the_same_shape() {
    let baked = Vec3::new(2.0, 4.0, 1.0);
    assert!(same_shape(baked, baked * 0.28));
    assert!(same_shape(baked, baked * 1.005));
    assert!(!same_shape(baked, Vec3::new(2.0, 1.0, 4.0)), "axes swapped");
    assert!(same_shape(
        Vec3::new(2.0, 0.0, 1.0),
        Vec3::new(4.0, 0.0, 2.0)
    ));
    assert!(!same_shape(
        Vec3::new(2.0, 0.0, 1.0),
        Vec3::new(4.0, 2.0, 0.0)
    ));
}

#[test]
fn the_mesh_asset_id_is_read_from_either_spelling() {
    let mut properties = std::collections::BTreeMap::new();
    assert_eq!(mesh_asset_id(&properties), None);
    properties.insert("MeshId".into(), Variant::String("rbxassetid://77".into()));
    assert_eq!(mesh_asset_id(&properties), Some(77));
    properties.insert(
        "MeshId".into(),
        Variant::String("rbxasset://fonts/x.mesh".into()),
    );
    assert_eq!(mesh_asset_id(&properties), None);
}
