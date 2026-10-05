use super::*;

fn region() -> StudBox {
    StudBox::from_center_size([0.0, 32.0, 0.0], [256.0, 128.0, 256.0])
}

fn summary(grid: &VoxelGrid) -> (usize, usize, Vec<Material>) {
    let mut materials: Vec<Material> = grid.voxels().map(|(_, c)| c.material).collect();
    let water = materials.iter().filter(|m| **m == Material::Water).count();
    let total = materials.len();
    materials.sort();
    materials.dedup();
    (total, water, materials)
}

#[test]
fn same_seed_same_terrain_and_a_new_seed_changes_it() {
    let settings = GenerateSettings::default();
    let mut a = VoxelGrid::new();
    let mut b = VoxelGrid::new();
    generate(&mut a, &region(), &settings);
    generate(&mut b, &region(), &settings);
    assert_eq!(a, b);
    let mut c = VoxelGrid::new();
    generate(&mut c, &region(), &GenerateSettings { seed: 12, ..settings });
    assert_ne!(a, c);
}

#[test]
fn terrain_stays_inside_the_region() {
    let mut grid = VoxelGrid::new();
    grid.set([100, 100, 100], Cell::full(Material::Brick));
    generate(&mut grid, &region(), &GenerateSettings::default());
    let (min, max) = region().voxels();
    for (v, cell) in grid.voxels() {
        if v == [100, 100, 100] {
            assert_eq!(cell, Cell::full(Material::Brick), "outside terrain survives");
            continue;
        }
        assert!((0..3).all(|a| v[a] >= min[a] && v[a] < max[a]), "{v:?}");
    }
}

#[test]
fn biomes_pick_their_materials() {
    let only = |biome: Biome| {
        let mut grid = VoxelGrid::new();
        let settings = GenerateSettings { biomes: vec![biome], ..Default::default() };
        generate(&mut grid, &region(), &settings);
        summary(&grid)
    };
    let (_, _, arctic) = only(Biome::Arctic);
    assert!(arctic.contains(&Material::Snow) && !arctic.contains(&Material::Grass));
    let (_, water, ocean) = only(Biome::Water);
    assert!(water > 0 && ocean.contains(&Material::Sand));
    let (_, _, lava) = only(Biome::Lavascape);
    assert!(lava.contains(&Material::Basalt));
    let (_, none, dunes) = only(Biome::Dunes);
    assert_eq!(none, 0, "no water without Water or Marsh");
    assert!(dunes.contains(&Material::Sand));
    let (mountains, _, _) = only(Biome::Mountains);
    let (plains, _, _) = only(Biome::Plains);
    assert!(mountains > plains, "mountains are bulkier than plains");
}

#[test]
fn caves_remove_rock_only_when_asked() {
    let base = GenerateSettings { biomes: vec![Biome::Mountains], ..Default::default() };
    let mut solid = VoxelGrid::new();
    generate(&mut solid, &region(), &base);
    let mut caves = VoxelGrid::new();
    generate(&mut caves, &region(), &GenerateSettings { caves: true, ..base });
    assert!(caves.voxels().count() < solid.voxels().count());
}

#[test]
fn no_biomes_does_nothing() {
    let mut grid = VoxelGrid::new();
    generate(&mut grid, &region(), &GenerateSettings { biomes: vec![], ..Default::default() });
    assert!(grid.is_empty());
}
