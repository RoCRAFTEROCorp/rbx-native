use super::*;
use crate::edit::brush::{Brush, Shape};
use crate::edit::stroke::draw_add;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn mesh_all(grid: &VoxelGrid) -> Vec<ChunkMesh> {
    meshable_chunks(grid)
        .into_iter()
        .map(|k| mesh_chunk(grid, k))
        .collect()
}

/// Every triangle's winding agrees with its vertices' normals.
fn assert_faces_out(surface: &Surface) {
    for tri in surface.indices.chunks(3) {
        let p = [0, 1, 2].map(|i| surface.positions[tri[i] as usize]);
        let face = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let normal = surface.normals[tri[0] as usize];
        assert!(dot(face, normal) > 0.0, "triangle {p:?} faces in");
    }
}

#[test]
fn a_full_slab_has_its_top_on_the_voxel_boundary() {
    let mut grid = VoxelGrid::new();
    for x in -8..8 {
        for z in -8..8 {
            grid.set([x, -1, z], Cell::full(Material::Grass));
        }
    }
    let meshes = mesh_all(&grid);
    let mut tops = 0;
    for mesh in &meshes {
        for (material, surface) in &mesh.solids {
            assert_eq!(*material, Material::Grass);
            assert_faces_out(surface);
            for (p, n) in surface.positions.iter().zip(&surface.normals) {
                if n[1] > 0.99 {
                    assert!(p[1].abs() < 1e-4, "top at {p:?}");
                    tops += 1;
                }
            }
        }
        assert!(mesh.water.is_empty());
    }
    assert!(tops > 100);
}

#[test]
fn partial_fill_raises_the_surface() {
    let mut grid = VoxelGrid::new();
    for x in -4..4 {
        for z in -4..4 {
            grid.set([x, 0, z], Cell::full(Material::Rock));
            grid.set([x, 1, z], Cell::with_fraction(Material::Rock, 0.75));
        }
    }
    let mesh = mesh_chunk(&grid, ChunkKey { x: 0, y: 0, z: 0 });
    let surface = &mesh.solids[0].1;
    let centre = surface
        .positions
        .iter()
        .find(|p| p[0] == 4.0 && p[2] == 4.0)
        .expect("a vertex over the middle");
    // Roblox puts a 3/4-full top voxel's surface 3 studs into it (y = 7);
    // the half-crossing between centres lands within a stud of that.
    assert!((centre[1] - 7.0).abs() < 1.0, "{centre:?}");
}

#[test]
fn a_ball_is_closed_and_crosses_chunk_seams_without_cracks() {
    let mut grid = VoxelGrid::new();
    // Centred on a chunk corner so all eight chunks share it.
    draw_add(
        &mut grid,
        &Brush::new(Shape::Sphere, [0.0, 0.0, 0.0], 40.0, 40.0),
        Material::Sand,
    );
    let meshes = mesh_all(&grid);
    // Gather every edge by its two world-space endpoints; in a closed
    // surface each appears exactly twice, once per neighbouring triangle.
    let mut edges = std::collections::HashMap::new();
    let key = |p: [f32; 3]| p.map(|v| (v * 1000.0).round() as i64);
    let mut triangles = 0;
    for mesh in &meshes {
        for (_, surface) in &mesh.solids {
            assert_faces_out(surface);
            for tri in surface.indices.chunks(3) {
                triangles += 1;
                for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                    let a = key(surface.positions[tri[i] as usize]);
                    let b = key(surface.positions[tri[j] as usize]);
                    *edges
                        .entry(if a < b { (a, b) } else { (b, a) })
                        .or_insert(0) += 1;
                }
            }
        }
    }
    assert!(triangles > 200);
    assert!(
        edges.values().all(|&n| n == 2),
        "open edges: {}",
        edges.values().filter(|&&n| n != 2).count()
    );
}

#[test]
fn materials_split_into_their_own_surfaces() {
    let mut grid = VoxelGrid::new();
    for x in 0..4 {
        grid.set(
            [x, 0, 0],
            Cell::full(if x < 2 {
                Material::Snow
            } else {
                Material::Rock
            }),
        );
    }
    let mesh = mesh_chunk(&grid, ChunkKey { x: 0, y: 0, z: 0 });
    let materials: Vec<Material> = mesh.solids.iter().map(|(m, _)| *m).collect();
    assert_eq!(materials, vec![Material::Rock, Material::Snow]);
}

#[test]
fn water_meshes_only_where_it_meets_air() {
    let mut grid = VoxelGrid::new();
    for x in -4..4 {
        for z in -4..4 {
            grid.set([x, 0, z], Cell::full(Material::Rock));
            grid.set([x, 1, z], Cell::full(Material::Water));
        }
    }
    let meshes = mesh_all(&grid);
    let water: Vec<&Surface> = meshes
        .iter()
        .map(|m| &m.water)
        .filter(|s| !s.is_empty())
        .collect();
    assert!(!water.is_empty());
    let mut top = 0;
    for surface in water {
        assert_faces_out(surface);
        for (p, n) in surface.positions.iter().zip(&surface.normals) {
            // Nothing faces down onto the rock below.
            assert!(n[1] > -0.5, "water against rock at {p:?}");
            if n[1] > 0.99 {
                assert!((p[1] - 8.0).abs() < 1e-4);
                top += 1;
            }
        }
    }
    assert!(top > 20);
}

#[test]
fn empty_and_far_chunks_mesh_to_nothing() {
    let grid = VoxelGrid::new();
    assert!(mesh_chunk(&grid, ChunkKey { x: 0, y: 0, z: 0 }).is_empty());
    let mut one = VoxelGrid::new();
    one.set([0, 0, 0], Cell::full(Material::Rock));
    assert!(mesh_chunk(&one, ChunkKey { x: 5, y: 0, z: 0 }).is_empty());
    assert_eq!(meshable_chunks(&one).len(), 4);
}
