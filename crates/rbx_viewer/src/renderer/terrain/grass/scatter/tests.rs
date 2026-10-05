use super::*;
use rbx_terrain::mesh::mesh_chunk;
use rbx_terrain::{Cell, ChunkKey};

const KEY: ChunkKey = ChunkKey { x: 0, y: 0, z: 0 };

/// A flat slab two voxels deep over the chunk's 32 x 32 columns.
fn slab(material: Material) -> VoxelGrid {
    let mut grid = VoxelGrid::new();
    for x in 0..CHUNK {
        for z in 0..CHUNK {
            for y in 0..2 {
                grid.set([x, y, z], Cell::full(material));
            }
        }
    }
    grid
}

fn blades(grid: &VoxelGrid) -> (Vec<BladeRaw>, Vec<Tile>) {
    scatter(&patches(&mesh_chunk(grid, KEY), grid), Vec3::ZERO)
}

#[test]
fn a_grass_field_grows_a_carpet_on_its_top_only() {
    let (blades, tiles) = blades(&slab(Material::Grass));
    // The top is 128 x 128 studs; its rim is cut by the slab's rounded edge.
    let expected = 128.0 * 128.0 * BLADES_PER_STUD;
    assert!(blades.len() as f32 > expected * 0.85, "{}", blades.len());
    assert!((blades.len() as f32) < expected * 1.05, "{}", blades.len());
    // On the top, bar the shallow start of the rounded rim; never the sides.
    assert!(blades.iter().all(|b| (4.0..=8.01).contains(&b.root[1])));
    let flat = blades.iter().filter(|b| (b.root[1] - 8.0).abs() < 0.01);
    assert!(flat.count() as f32 > blades.len() as f32 * 0.9);
    assert_eq!(tiles.len(), TILES * TILES);
}

#[test]
fn rock_grows_nothing_and_neither_does_a_lake_bed() {
    assert!(blades(&slab(Material::Rock)).0.is_empty());
    let mut grid = slab(Material::Grass);
    for x in 0..CHUNK {
        for z in 0..CHUNK {
            grid.set([x, 2, z], Cell::full(Material::Water));
        }
    }
    assert!(blades(&grid).0.is_empty());
}

#[test]
fn leafy_grass_is_not_grass() {
    assert!(blades(&slab(Material::LeafyGrass)).0.is_empty());
}

#[test]
fn the_same_surface_grows_the_same_blades_whatever_else_changed() {
    let mut grid = slab(Material::Grass);
    let before = blades(&grid);
    // Far outside this chunk and the layer around it that it meshes from.
    grid.set([200, 0, 200], Cell::full(Material::Rock));
    assert_eq!(blades(&grid), before);
}

#[test]
fn tiles_cover_every_blade_in_rank_order() {
    let (blades, tiles) = blades(&slab(Material::Grass));
    let mut next = 0;
    for tile in &tiles {
        assert_eq!(tile.blades.start, next);
        next = tile.blades.end;
        let ranks: Vec<u8> = blades[tile.blades.start as usize..tile.blades.end as usize]
            .iter()
            .map(|b| b.shape[3])
            .collect();
        assert!(ranks.is_sorted());
        assert!(tile.max.x - tile.min.x <= TILE && tile.max.z - tile.min.z <= TILE);
    }
    assert_eq!(next as usize, blades.len());
}

#[test]
fn density_thins_to_nothing_at_the_reach() {
    assert_eq!(density_at(0.0, 150.0, 0.8), 0.8);
    assert_eq!(density_at(50.0, 150.0, 1.0), 1.0);
    assert!((density_at(100.0, 150.0, 1.0) - 0.5).abs() < 1e-6);
    assert_eq!(density_at(150.0, 150.0, 1.0), 0.0);
    assert_eq!(density_at(0.0, 0.0, 1.0), 0.0);
}
