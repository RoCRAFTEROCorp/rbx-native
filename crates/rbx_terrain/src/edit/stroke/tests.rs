use super::*;
use crate::edit::brush::Shape;

fn slab(grid: &mut VoxelGrid, material: Material, top: i32) {
    for x in -6..6 {
        for z in -6..6 {
            for y in -3..top {
                grid.set([x, y, z], Cell::full(material));
            }
        }
    }
}

fn solid_volume(grid: &VoxelGrid) -> f32 {
    grid.voxels().map(|(_, c)| c.solid_fraction()).sum()
}

#[test]
fn draw_add_fills_a_ball_with_the_source_material() {
    let mut grid = VoxelGrid::new();
    let brush = Brush::new(Shape::Sphere, [0.0; 3], 16.0, 16.0);
    draw_add(&mut grid, &brush, Material::Grass);
    assert_eq!(grid.get([0, 0, 0]), Cell::full(Material::Grass));
    assert!(!grid.get([1, 0, 0]).is_air());
    assert!(grid.get([3, 0, 0]).is_air());
    // About a sphere of radius 2 voxels: 4/3·π·2³ ≈ 33.5.
    let volume = solid_volume(&grid);
    assert!((volume - 33.5).abs() < 4.0, "volume {volume}");
}

#[test]
fn draw_add_keeps_existing_material_and_never_shrinks() {
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Rock, 0);
    let before = solid_volume(&grid);
    draw_add(
        &mut grid,
        &Brush::new(Shape::Sphere, [0.0; 3], 16.0, 16.0),
        Material::Grass,
    );
    assert_eq!(grid.get([0, -1, 0]).material, Material::Rock);
    assert_eq!(grid.get([0, 1, 0]).material, Material::Grass);
    assert!(solid_volume(&grid) > before);
}

#[test]
fn draw_subtract_carves_and_respects_ignore_water() {
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Rock, 0);
    grid.set([0, 0, 0], Cell::full(Material::Water));
    let brush = Brush::new(Shape::Sphere, [2.0, 0.0, 2.0], 12.0, 12.0);
    draw_subtract(&mut grid, &brush, WaterRule { ignore_water: true });
    assert!(grid.get([0, -1, 0]).is_air());
    assert_eq!(grid.get([0, 0, 0]), Cell::full(Material::Water));
    draw_subtract(&mut grid, &brush, WaterRule::default());
    assert!(grid.get([0, 0, 0]).is_air());
}

#[test]
fn sculpt_grows_from_the_surface_only() {
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Sand, 0);
    let brush = Brush::new(Shape::Sphere, [2.0, 2.0, 2.0], 24.0, 24.0);
    sculpt(
        &mut grid,
        &brush,
        true,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    let raised = grid.get([0, 0, 0]);
    assert_eq!(
        raised.material,
        Material::Sand,
        "takes the surface's material"
    );
    assert!(raised.solid_fraction() > 0.0 && raised.solid_fraction() < 0.5);
    // Two voxels up is not touching anything solid yet.
    assert!(grid.get([0, 2, 0]).is_air());
    let once = solid_volume(&grid);
    sculpt(
        &mut grid,
        &brush,
        true,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    assert!(solid_volume(&grid) > once);
    sculpt(
        &mut grid,
        &brush,
        false,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    sculpt(
        &mut grid,
        &brush,
        false,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    assert!(solid_volume(&grid) < once);
}

#[test]
fn weaker_sculpt_moves_less() {
    let brush = Brush::new(Shape::Sphere, [2.0, 2.0, 2.0], 24.0, 24.0);
    let mut strong = VoxelGrid::new();
    slab(&mut strong, Material::Sand, 0);
    let mut weak = strong.clone();
    sculpt(
        &mut strong,
        &brush,
        true,
        1.0,
        Material::Sand,
        WaterRule::default(),
    );
    sculpt(
        &mut weak,
        &brush,
        true,
        0.1,
        Material::Sand,
        WaterRule::default(),
    );
    assert!(solid_volume(&strong) > solid_volume(&weak));
}

#[test]
fn smooth_wears_down_a_spike_and_keeps_volume_roughly() {
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Rock, 0);
    for y in 0..4 {
        grid.set([0, y, 0], Cell::full(Material::Rock));
    }
    let before = solid_volume(&grid);
    let brush = Brush::new(Shape::Sphere, [2.0, 8.0, 2.0], 32.0, 32.0);
    smooth(&mut grid, &brush, 1.0, WaterRule::default());
    assert!(grid.get([0, 3, 0]).solid_fraction() < 0.5);
    assert!(
        grid.get([1, 0, 0]).solid_fraction() > 0.0,
        "the foot fills in"
    );
    assert!((solid_volume(&grid) - before).abs() < before * 0.1);
}

#[test]
fn flatten_cuts_and_fills_to_the_plane() {
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Rock, 0);
    grid.set([0, 0, 0], Cell::full(Material::Rock));
    grid.set([0, 1, 0], Cell::full(Material::Rock));
    grid.set([2, -1, 0], Cell::AIR);
    let brush = Brush::new(Shape::Cylinder, [2.0, 0.0, 2.0], 24.0, 24.0);
    flatten(
        &mut grid,
        &brush,
        2.0,
        FlattenMode::Both,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    assert!((grid.get([0, 0, 0]).solid_fraction() - 0.5).abs() < 0.01);
    assert!(grid.get([0, 1, 0]).is_air());
    assert_eq!(
        grid.get([2, -1, 0]),
        Cell::full(Material::Rock),
        "fills from below"
    );
}

#[test]
fn flatten_modes_only_go_one_way() {
    let brush = Brush::new(Shape::Cylinder, [2.0, 0.0, 2.0], 24.0, 24.0);
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Rock, 0);
    grid.set([0, 0, 0], Cell::full(Material::Rock));
    grid.set([2, -1, 0], Cell::AIR);
    let mut erode = grid.clone();
    flatten(
        &mut erode,
        &brush,
        -2.0,
        FlattenMode::Erode,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    assert!(erode.get([0, 0, 0]).is_air());
    assert!(erode.get([2, -1, 0]).is_air(), "erode never fills");
    flatten(
        &mut grid,
        &brush,
        2.0,
        FlattenMode::Grow,
        1.0,
        Material::Grass,
        WaterRule::default(),
    );
    assert_eq!(
        grid.get([0, 0, 0]),
        Cell::full(Material::Rock),
        "grow never cuts"
    );
}

#[test]
fn paint_changes_material_not_shape() {
    let mut grid = VoxelGrid::new();
    slab(&mut grid, Material::Rock, 0);
    grid.set([1, 0, 0], Cell::new(Material::Sand, 100, 0));
    let before = solid_volume(&grid);
    let brush = Brush::new(Shape::Box, [2.0, 0.0, 2.0], 16.0, 16.0);
    paint(&mut grid, &brush, Material::Snow, Some(Material::Rock));
    assert_eq!(grid.get([0, -1, 0]).material, Material::Snow);
    assert_eq!(
        grid.get([1, 0, 0]),
        Cell::new(Material::Sand, 100, 0),
        "replace skips other materials"
    );
    paint(&mut grid, &brush, Material::Snow, None);
    assert_eq!(grid.get([1, 0, 0]), Cell::new(Material::Snow, 100, 0));
    assert_eq!(solid_volume(&grid), before);
    paint(&mut grid, &brush, Material::Water, None);
    assert_eq!(
        grid.get([1, 0, 0]).material,
        Material::Snow,
        "water is not paint"
    );
}

#[test]
fn auto_material_picks_the_nearest_solid() {
    let mut grid = VoxelGrid::new();
    grid.set([0, 0, 0], Cell::full(Material::Mud));
    grid.set([3, 0, 0], Cell::full(Material::Snow));
    let brush = Brush::new(Shape::Sphere, [3.0, 2.0, 2.0], 32.0, 32.0);
    assert_eq!(auto_material(&grid, &brush), Some(Material::Mud));
    assert_eq!(auto_material(&VoxelGrid::new(), &brush), None);
}
