//! The region tools: Fill/Replace, Sea Level, Delete and Clear act on the box
//! the Select tool holds.

use super::brush::voxel_center;
use crate::{Cell, Material, VoxelGrid, VOXEL_STUDS};

/// An axis-aligned box in studs: the selection region. It need not sit on
/// the voxel grid; Fill covers a partly overlapped voxel by the overlap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StudBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl StudBox {
    pub fn from_center_size(center: [f32; 3], size: [f32; 3]) -> StudBox {
        StudBox {
            min: std::array::from_fn(|a| center[a] - size[a].abs() * 0.5),
            max: std::array::from_fn(|a| center[a] + size[a].abs() * 0.5),
        }
    }

    pub fn center(&self) -> [f32; 3] {
        std::array::from_fn(|a| (self.min[a] + self.max[a]) * 0.5)
    }

    pub fn size(&self) -> [f32; 3] {
        std::array::from_fn(|a| self.max[a] - self.min[a])
    }

    /// Grows the box outward to whole voxels (Snap to Voxels).
    pub fn snapped(&self) -> StudBox {
        StudBox {
            min: self.min.map(|v| (v / VOXEL_STUDS).floor() * VOXEL_STUDS),
            max: self.max.map(|v| (v / VOXEL_STUDS).ceil() * VOXEL_STUDS),
        }
    }

    /// The voxels the box touches, `(min, max)` with `max` exclusive.
    pub fn voxels(&self) -> ([i32; 3], [i32; 3]) {
        (
            self.min.map(|v| (v / VOXEL_STUDS).floor() as i32),
            self.max.map(|v| (v / VOXEL_STUDS).ceil() as i32),
        )
    }

    /// How much of voxel `v` lies inside the box, 0 to 1.
    pub fn overlap(&self, v: [i32; 3]) -> f32 {
        (0..3)
            .map(|a| {
                let low = v[a] as f32 * VOXEL_STUDS;
                let high = low + VOXEL_STUDS;
                ((high.min(self.max[a]) - low.max(self.min[a])) / VOXEL_STUDS).clamp(0.0, 1.0)
            })
            .product()
    }

    pub fn contains_center(&self, v: [i32; 3]) -> bool {
        let c = voxel_center(v);
        (0..3).all(|a| c[a] >= self.min[a] && c[a] < self.max[a])
    }

    pub(crate) fn each_voxel(&self, mut visit: impl FnMut([i32; 3])) {
        let (min, max) = self.voxels();
        for y in min[1]..max[1] {
            for z in min[2]..max[2] {
                for x in min[0]..max[0] {
                    visit([x, y, z]);
                }
            }
        }
    }
}

/// Fill mode: the box becomes `material`, voxels it only partly overlaps
/// filling by the overlap without ever emptying what is already there. Air
/// clears the box instead.
pub fn fill(grid: &mut VoxelGrid, region: &StudBox, material: Material) {
    if material == Material::Air {
        delete(grid, region);
        return;
    }
    region.each_voxel(|v| {
        let overlap = region.overlap(v);
        let cell = grid.get(v);
        if overlap >= 1.0 {
            grid.set(v, Cell::full(material));
        } else if overlap > cell.fraction() || cell.material == material {
            grid.set(
                v,
                Cell::with_fraction(material, overlap.max(cell.fraction())),
            );
        }
    });
}

/// Replace mode: every voxel of `from` inside the box becomes `to`, keeping
/// its fill. Air as `from` fills the empty space; Air as `to` deletes.
pub fn replace(grid: &mut VoxelGrid, region: &StudBox, from: Material, to: Material) {
    region.each_voxel(|v| {
        if !region.contains_center(v) {
            return;
        }
        let cell = grid.get(v);
        if cell.material != from {
            return;
        }
        let next = match (from, to) {
            (_, Material::Air) => Cell::AIR,
            (Material::Air, to) => Cell::full(to),
            (_, to) => Cell::new(to, cell.occupancy, cell.liquid),
        };
        grid.set(v, next);
    });
}

/// Deletes every voxel whose centre is inside the box (the Select tool's
/// Delete key).
pub fn delete(grid: &mut VoxelGrid, region: &StudBox) {
    region.each_voxel(|v| {
        if region.contains_center(v) {
            grid.set(v, Cell::AIR);
        }
    });
}

/// Sea Level's Create: fill the box's empty space with water, leaving solid
/// terrain where it is.
pub fn sea_level_create(grid: &mut VoxelGrid, region: &StudBox) {
    region.each_voxel(|v| {
        if !region.contains_center(v) {
            return;
        }
        if grid.get(v).is_air() {
            grid.set(v, Cell::full(Material::Water));
        }
    });
}

/// Sea Level's Evaporate: remove all water in the box, including water
/// sharing a voxel with solid terrain.
pub fn sea_level_evaporate(grid: &mut VoxelGrid, region: &StudBox) {
    region.each_voxel(|v| {
        if !region.contains_center(v) {
            return;
        }
        let cell = grid.get(v);
        if cell.material == Material::Water {
            grid.set(v, Cell::AIR);
        } else if cell.liquid != 0 {
            grid.set(v, Cell::new(cell.material, cell.occupancy, 0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_covers_partial_voxels_by_overlap() {
        let mut grid = VoxelGrid::new();
        let region = StudBox {
            min: [0.0, 0.0, 0.0],
            max: [8.0, 6.0, 4.0],
        };
        fill(&mut grid, &region, Material::Salt);
        assert_eq!(grid.get([0, 0, 0]), Cell::full(Material::Salt));
        assert!((grid.get([1, 1, 0]).fraction() - 0.5).abs() < 0.01);
        assert!(grid.get([2, 0, 0]).is_air());
        assert_eq!(region.snapped().max, [8.0, 8.0, 4.0]);
    }

    #[test]
    fn replace_swaps_one_material_keeping_shape() {
        let mut grid = VoxelGrid::new();
        grid.set([0, 0, 0], Cell::new(Material::Grass, 90, 0));
        grid.set([1, 0, 0], Cell::full(Material::Rock));
        let region = StudBox::from_center_size([4.0, 2.0, 2.0], [8.0, 4.0, 4.0]);
        replace(&mut grid, &region, Material::Grass, Material::Snow);
        assert_eq!(grid.get([0, 0, 0]), Cell::new(Material::Snow, 90, 0));
        assert_eq!(grid.get([1, 0, 0]), Cell::full(Material::Rock));
        replace(&mut grid, &region, Material::Rock, Material::Air);
        assert!(grid.get([1, 0, 0]).is_air());
        replace(&mut grid, &region, Material::Air, Material::Mud);
        assert_eq!(grid.get([1, 0, 0]), Cell::full(Material::Mud));
    }

    #[test]
    fn sea_level_fills_air_and_evaporates_all_water() {
        let mut grid = VoxelGrid::new();
        grid.set([0, 0, 0], Cell::full(Material::Rock));
        grid.set([1, 0, 0], Cell::new(Material::Rock, 100, 50));
        let region = StudBox::from_center_size([4.0, 2.0, 4.0], [12.0, 4.0, 8.0]);
        sea_level_create(&mut grid, &region);
        assert_eq!(grid.get([0, 0, 0]), Cell::full(Material::Rock));
        assert_eq!(grid.get([1, 0, 1]), Cell::full(Material::Water));
        sea_level_evaporate(&mut grid, &region);
        assert!(grid.get([1, 0, 1]).is_air());
        assert_eq!(grid.get([1, 0, 0]), Cell::new(Material::Rock, 100, 0));
    }

    #[test]
    fn delete_and_air_fill_empty_the_box() {
        let mut grid = VoxelGrid::new();
        let region = StudBox::from_center_size([0.0; 3], [16.0; 3]);
        fill(&mut grid, &region, Material::Brick);
        assert_eq!(grid.voxels().count(), 64);
        fill(&mut grid, &region, Material::Air);
        assert!(grid.is_empty());
    }
}
