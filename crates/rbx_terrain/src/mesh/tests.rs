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

fn weight_of(blend: &Blend, material: Material) -> f32 {
    blend
        .materials
        .iter()
        .zip(blend.weights)
        .filter(|(m, _)| **m == material)
        .map(|(_, w)| w)
        .sum()
}

/// One blend per vertex, each a proper one: weights in 0..=1 summing to 1,
/// real materials in slot order, Air padding after them with no weight.
fn assert_blends_valid(mesh: &ChunkMesh) {
    assert_eq!(mesh.blends.len(), mesh.solid.positions.len());
    for blend in &mesh.blends {
        let sum: f32 = blend.weights.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "{blend:?}");
        assert!(blend.weights.iter().all(|w| (0.0..=1.0).contains(w)));
        let used = blend.materials.iter().take_while(|m| m.is_solid()).count();
        assert!(used > 0, "{blend:?}");
        assert!(blend.materials[..used].windows(2).all(|p| p[0] < p[1]));
        for (m, w) in blend.materials.iter().zip(blend.weights).skip(used) {
            assert_eq!((*m, w), (Material::Air, 0.0), "{blend:?}");
        }
    }
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
        let surface = &mesh.solid;
        assert_faces_out(surface);
        assert_blends_valid(mesh);
        // One material throughout: all of its weight, nothing to blend.
        for blend in &mesh.blends {
            assert_eq!(blend.materials[0], Material::Grass);
            assert_eq!(blend.weights[0], 1.0);
        }
        for (p, n) in surface.positions.iter().zip(&surface.normals) {
            if n[1] > 0.99 {
                assert!(p[1].abs() < 1e-4, "top at {p:?}");
                tops += 1;
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
    let surface = &mesh.solid;
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
fn a_two_material_ball_is_closed_and_seamless_across_chunks() {
    let mut grid = VoxelGrid::new();
    // Centred on a chunk corner so all eight chunks share it.
    draw_add(
        &mut grid,
        &Brush::new(Shape::Sphere, [0.0, 0.0, 0.0], 40.0, 40.0),
        Material::Sand,
    );
    // Its +x half is rock: the border runs through chunk seams, which is
    // where vertices split by blend list must still line up.
    let east: Vec<_> = grid.voxels().filter(|(v, _)| v[0] >= 0).collect();
    for (v, cell) in east {
        grid.set(v, Cell::new(Material::Rock, cell.occupancy, cell.liquid));
    }
    let meshes = mesh_all(&grid);
    // Gather every edge by its two world-space endpoints; in a closed
    // surface each appears exactly twice, once per neighbouring triangle.
    let mut edges = std::collections::HashMap::new();
    // Every copy of a vertex, in any chunk or blend list, must agree on how
    // much rock it is, or the blend would jump along an edge.
    let mut rock = std::collections::HashMap::new();
    let key = |p: [f32; 3]| p.map(|v| (v * 1000.0).round() as i64);
    let mut triangles = 0;
    let mut blended = 0;
    for mesh in &meshes {
        let surface = &mesh.solid;
        assert_faces_out(surface);
        assert_blends_valid(mesh);
        for (p, blend) in surface.positions.iter().zip(&mesh.blends) {
            let w = weight_of(blend, Material::Rock);
            let seen = *rock.entry(key(*p)).or_insert(w);
            assert!((seen - w).abs() < 1e-5, "{p:?}: {seen} vs {w}");
            if w > 0.0 && w < 1.0 {
                blended += 1;
            }
        }
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
    assert!(triangles > 200);
    assert!(blended > 10, "{blended} vertices blend");
    assert!(
        edges.values().all(|&n| n == 2),
        "open edges: {}",
        edges.values().filter(|&&n| n != 2).count()
    );
}

#[test]
fn a_border_vertex_blends_both_materials() {
    let mut grid = VoxelGrid::new();
    for x in -8..8 {
        for z in -8..8 {
            let material = if x < 0 {
                Material::Snow
            } else {
                Material::Rock
            };
            grid.set([x, -1, z], Cell::full(material));
        }
    }
    let mut seen = 0;
    for mesh in mesh_all(&grid) {
        assert_blends_valid(&mesh);
        let tops = mesh.solid.positions.iter().zip(&mesh.solid.normals);
        for ((p, n), blend) in tops.zip(&mesh.blends) {
            if n[1] < 0.99 || p[2].abs() > 16.0 {
                continue;
            }
            let (snow, rock) = (
                weight_of(blend, Material::Snow),
                weight_of(blend, Material::Rock),
            );
            // The cell straddling the border sits on it, half of each.
            if p[0].abs() < 1e-4 {
                assert!((snow - 0.5).abs() < 1e-5 && (rock - 0.5).abs() < 1e-5);
                seen += 1;
            } else if p[0] < -1.0 {
                assert_eq!(snow, 1.0, "{p:?}");
            } else {
                assert_eq!(rock, 1.0, "{p:?}");
            }
        }
    }
    assert!(seen >= 8, "{seen}");
}

#[test]
fn four_materials_meeting_keep_the_heaviest_three() {
    let mut grid = VoxelGrid::new();
    let corner = [
        Material::Grass,
        Material::Rock,
        Material::Sand,
        Material::Snow,
    ];
    for x in -4..4 {
        for z in -4..4 {
            let quadrant = usize::from(x >= 0) + 2 * usize::from(z >= 0);
            grid.set([x, 0, z], Cell::full(corner[quadrant]));
        }
    }
    for mesh in mesh_all(&grid) {
        assert_blends_valid(&mesh);
    }
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
        for tri in surface.indices.chunks(3) {
            let p = [0, 1, 2].map(|i| surface.positions[tri[i] as usize]);
            let face = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            // No face looks down onto the rock below.
            assert!(face[1] >= -1e-4, "water against rock at {p:?}");
        }
        for (p, n) in surface.positions.iter().zip(&surface.normals) {
            // Never above the water's own top.
            assert!(p[1] <= 8.0 + 1e-4, "water rises to {p:?}");
            if n[1] > 0.99 {
                assert!((p[1] - 8.0).abs() < 1e-4);
                top += 1;
            }
        }
    }
    assert!(top > 20);
}

#[test]
fn water_never_climbs_the_shore() {
    // A sand slope rising out of a flat sea: the water's vertices stay at
    // or below the sea's top, never up on the sand.
    let mut grid = VoxelGrid::new();
    for x in 0..16 {
        for z in 0..4 {
            let top = x / 2;
            for y in 0..=top {
                grid.set([x, y, z], Cell::full(Material::Sand));
            }
            for y in (top + 1)..4 {
                grid.set([x, y, z], Cell::full(Material::Water));
            }
        }
    }
    let sea_top = 16.0;
    for mesh in mesh_all(&grid) {
        for p in &mesh.water.positions {
            assert!(p[1] <= sea_top + 1e-3, "water at {p:?}");
        }
    }
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
