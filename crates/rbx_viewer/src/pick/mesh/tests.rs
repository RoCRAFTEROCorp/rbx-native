use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_mesh::{Aabb, Mesh, Vertex};

use super::*;
use crate::pick::tests::corner_tetrahedron;

fn toward_neg_z(x: f32, y: f32) -> Ray {
    Ray::new(Vec3::new(x, y, 10.0), Vec3::NEG_Z)
}

#[track_caller]
fn assert_close(actual: Option<f32>, expected: f32) {
    let actual = actual.expect("the ray meets the mesh");
    assert!(
        (actual - expected).abs() < 1e-4,
        "hit at {actual}, expected {expected}"
    );
}

#[test]
fn a_ray_through_the_empty_part_of_the_mesh_box_misses() {
    // The tetrahedron fills only the (-, -, -) corner of its box: this ray
    // passes through the box's opposite corner, where there is nothing.
    let ray = toward_neg_z(0.4, 0.4);
    assert!(crate::pick::ray_hits_box(ray, Mat4::IDENTITY).is_some());
    assert!(hit(&corner_tetrahedron(), Mat4::IDENTITY, ray).is_none());
}

#[test]
fn a_ray_meets_the_nearest_triangle() {
    // The slanted face x + y + z = -0.5 stands at z = 0.3 above (-0.4, -0.4);
    // the box face at z = 0.5 is not where the mesh is.
    let ray = toward_neg_z(-0.4, -0.4);
    assert_close(hit(&corner_tetrahedron(), Mat4::IDENTITY, ray), 9.7);
}

#[test]
fn the_mesh_follows_its_model_matrix() {
    let model =
        Mat4::from_translation(Vec3::new(0.0, 0.0, -20.0)) * Mat4::from_scale(Vec3::splat(2.0));
    let ray = Ray::new(Vec3::new(-0.8, -0.8, 0.0), Vec3::NEG_Z);
    // The same slanted-face hit as above, doubled and moved 20 studs away.
    assert_close(hit(&corner_tetrahedron(), model, ray), 19.4);
}

#[test]
fn a_ray_starting_inside_meets_the_far_side() {
    let inside = Ray::new(Vec3::splat(-0.45), Vec3::NEG_Z);
    // Out through the z = -0.5 face, a twentieth of a stud away.
    assert_close(hit(&corner_tetrahedron(), Mat4::IDENTITY, inside), 0.05);
}

#[test]
fn a_mesh_behind_the_ray_is_not_hit() {
    let away = Ray::new(Vec3::new(-0.4, -0.4, 10.0), Vec3::Z);
    assert!(hit(&corner_tetrahedron(), Mat4::IDENTITY, away).is_none());
}

#[test]
fn a_degenerate_or_dangling_triangle_is_skipped() {
    let vertex = |x: f32, y: f32, z: f32| Vertex {
        position: [x, y, z],
        normal: [0.0; 3],
        uv: [0.0; 2],
        color: [255; 4],
    };
    let mesh = Mesh {
        version: (4, 1),
        vertices: vec![
            vertex(0.0, 0.0, 0.0),
            vertex(1.0, 0.0, 0.0),
            vertex(2.0, 0.0, 0.0),
        ],
        // Three collinear points, then an index past the end.
        indices: vec![0, 1, 2, 0, 1, 9],
        lods: Vec::new(),
        bounds: Aabb {
            min: [0.0; 3],
            max: [2.0, 0.0, 0.0],
        },
    };
    assert!(hit(&mesh, Mat4::IDENTITY, toward_neg_z(1.0, 0.0)).is_none());
}

#[test]
fn meshes_are_empty_by_default_and_shared_once_built() {
    let asset = AssetRef::Id(7);
    assert!(Meshes::default().get(&asset).is_none());

    let mesh = Arc::new(corner_tetrahedron());
    let meshes = Meshes::new(HashMap::from([(asset.clone(), mesh.clone())]));
    let found = meshes.get(&asset).expect("the mesh that was put in");
    assert!(Arc::ptr_eq(found, &mesh), "no copy was made");
    // A clone of the handle still reads the same map.
    assert!(Arc::ptr_eq(
        meshes.clone().get(&asset).expect("shared"),
        &mesh
    ));
}

#[test]
fn a_surface_hit_on_the_slanted_face_faces_back_up_the_ray() {
    // The face x + y + z = -0.5, whatever way its triangle happens to wind.
    let ray = toward_neg_z(-0.4, -0.4);
    let (point, normal) = surface(&corner_tetrahedron(), Mat4::IDENTITY, ray).unwrap();
    assert!((point - Vec3::new(-0.4, -0.4, 0.3)).length() < 1e-4);
    assert!((normal - Vec3::ONE.normalize()).length() < 1e-4, "{normal}");
}

/// A mesh of `triangles`, given by their corners, every one with vertices of
/// its own — split, as an exported mesh's are
/// at every UV seam, so the walk has to join them by position.
fn soup(triangles: &[[[f32; 3]; 3]]) -> Mesh {
    let vertices: Vec<Vertex> = triangles
        .iter()
        .flatten()
        .map(|&position| Vertex {
            position,
            normal: [0.0; 3],
            uv: [0.0; 2],
            color: [255; 4],
        })
        .collect();
    Mesh {
        version: (4, 1),
        indices: (0..vertices.len() as u32).collect(),
        vertices,
        lods: Vec::new(),
        bounds: Aabb {
            min: [-0.5; 3],
            max: [0.5; 3],
        },
    }
}

#[track_caller]
fn assert_same_points(mut actual: Vec<Vec3>, expected: &[[f32; 2]]) {
    let key = |p: &Vec3| (p.x.to_bits(), p.y.to_bits());
    actual.sort_by_key(key);
    let mut expected: Vec<Vec3> = expected
        .iter()
        .map(|&[x, y]| Vec3::new(x, y, 0.5))
        .collect();
    expected.sort_by_key(key);
    assert_eq!(actual, expected);
}

#[test]
fn a_flat_quad_of_two_triangles_is_outlined_without_its_diagonal() {
    // The top of a box, z = 0.5, split along its diagonal, and a side of it
    // standing up from one edge so the walk has a neighbour to turn down.
    let mesh = soup(&[
        [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5]],
        [[-0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]],
        [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, -0.5, -0.5]],
    ]);
    let face = flat_face(&mesh, Mat4::IDENTITY, toward_neg_z(0.3, -0.2)).unwrap();
    let corners = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
    assert_same_points(face.corners, &corners);
    assert_eq!(face.sides.len(), 4);
    for [a, b] in face.sides {
        let middle = (a + b) / 2.0;
        assert!(middle.truncate().length() > 0.4, "a diagonal: {a} {b}");
    }
}

#[test]
fn a_vertex_splitting_a_straight_side_is_no_corner() {
    // The same top as a fan out of the middle of its bottom side.
    let m = [0.0, -0.5, 0.5];
    let mesh = soup(&[
        [m, [0.5, -0.5, 0.5], [0.5, 0.5, 0.5]],
        [m, [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]],
        [m, [-0.5, 0.5, 0.5], [-0.5, -0.5, 0.5]],
    ]);
    let face = flat_face(&mesh, Mat4::IDENTITY, toward_neg_z(-0.3, 0.3)).unwrap();
    let corners = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
    assert_same_points(face.corners, &corners);
    assert_eq!(face.sides.len(), 5);
}

#[test]
fn a_curved_region_falls_back_to_the_hit_triangle() {
    // No face of the tetrahedron lies flat with another: the slanted face's
    // outline is its own triangle.
    let face = flat_face(
        &corner_tetrahedron(),
        Mat4::IDENTITY,
        toward_neg_z(-0.4, -0.4),
    )
    .unwrap();
    assert_eq!(face.corners.len(), 3);
    assert_eq!(face.sides.len(), 3);
}

#[test]
fn a_flat_face_follows_its_model_matrix() {
    let mesh = soup(&[
        [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5]],
        [[-0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]],
    ]);
    let model = Mat4::from_scale(Vec3::new(4.0, 2.0, 1.0));
    let face = flat_face(&mesh, model, toward_neg_z(1.0, 0.5)).unwrap();
    assert!(face.corners.contains(&Vec3::new(2.0, 1.0, 0.5)));
}
