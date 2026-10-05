//! Terrain on the clipboard: copy a region's voxels out, and paste them back
//! moved, turned and stretched (the Select tool's Copy/Cut/Paste/Duplicate
//! and the Transform tool).

use super::brush::{dot, sub, voxel_center};
use super::region::StudBox;
use crate::{Cell, VoxelGrid, VOXEL_STUDS};

/// A voxel-aligned block of terrain, like a `TerrainRegion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub size: [i32; 3],
    cells: Vec<Cell>,
}

impl Clip {
    fn index(&self, v: [i32; 3]) -> usize {
        (v[0] + self.size[0] * (v[2] + self.size[2] * v[1])) as usize
    }

    pub fn get(&self, v: [i32; 3]) -> Cell {
        if (0..3).any(|a| v[a] < 0 || v[a] >= self.size[a]) {
            return Cell::AIR;
        }
        self.cells[self.index(v)]
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| c.is_air())
    }

    /// The clip's size in studs.
    pub fn studs(&self) -> [f32; 3] {
        self.size.map(|s| s as f32 * VOXEL_STUDS)
    }
}

/// Copies every voxel the (snapped) box covers.
pub fn copy(grid: &VoxelGrid, region: &StudBox) -> Clip {
    let (min, max) = region.snapped().voxels();
    let size = std::array::from_fn(|a| (max[a] - min[a]).max(0));
    let mut clip = Clip {
        size,
        cells: vec![Cell::AIR; size.iter().map(|&s| s as usize).product()],
    };
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let index = clip.index([x, y, z]);
                clip.cells[index] = grid.get([min[0] + x, min[1] + y, min[2] + z]);
            }
        }
    }
    clip
}

/// Where a pasted clip goes: its centre and size in studs, and its local
/// X, Y, Z axes in world space (the rows of the rotation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub center: [f32; 3],
    pub size: [f32; 3],
    pub axes: [[f32; 3]; 3],
}

impl Placement {
    /// The clip back where `region` copied it from, unturned.
    pub fn at(region: &StudBox) -> Placement {
        let snapped = region.snapped();
        Placement {
            center: snapped.center(),
            size: snapped.size(),
            axes: super::brush::IDENTITY,
        }
    }

    /// The world-space box the turned placement occupies.
    pub fn bounds(&self) -> StudBox {
        let reach: [f32; 3] = std::array::from_fn(|world| {
            (0..3)
                .map(|local| self.axes[local][world].abs() * self.size[local].abs() * 0.5)
                .sum()
        });
        StudBox {
            min: std::array::from_fn(|a| self.center[a] - reach[a]),
            max: std::array::from_fn(|a| self.center[a] + reach[a]),
        }
    }
}

/// Writes `clip` into the grid at `placement`, sampling the nearest clip
/// voxel for each world voxel the placement covers. With `merge_empty`, Air
/// in the clip clears what it lands on; without it, only the clip's terrain
/// is written and existing terrain shows through its empty parts.
pub fn paste(grid: &mut VoxelGrid, clip: &Clip, placement: &Placement, merge_empty: bool) {
    if placement.size.iter().any(|s| s.abs() < f32::EPSILON) {
        return;
    }
    placement.bounds().each_voxel(|v| {
        let local = sub(voxel_center(v), placement.center);
        let source: [i32; 3] = std::array::from_fn(|a| {
            let along = dot(local, placement.axes[a]) / placement.size[a] + 0.5;
            (along * clip.size[a] as f32).floor() as i32
        });
        if (0..3).any(|a| source[a] < 0 || source[a] >= clip.size[a]) {
            return;
        }
        let cell = clip.get(source);
        if !cell.is_air() || merge_empty {
            grid.set(v, cell);
        }
    });
}

/// The Transform tool's apply: lift the region's terrain out and put it down
/// at `placement`.
pub fn transform(grid: &mut VoxelGrid, region: &StudBox, placement: &Placement, merge_empty: bool) {
    let clip = copy(grid, region);
    super::region::delete(grid, &region.snapped());
    paste(grid, &clip, placement, merge_empty);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Material;

    fn l_shape() -> VoxelGrid {
        let mut grid = VoxelGrid::new();
        grid.set([0, 0, 0], Cell::full(Material::Rock));
        grid.set([1, 0, 0], Cell::full(Material::Rock));
        grid.set([0, 1, 0], Cell::new(Material::Grass, 99, 0));
        grid
    }

    #[test]
    fn copy_then_paste_in_place_is_identity() {
        let grid = l_shape();
        let region = StudBox::from_center_size([4.0, 4.0, 2.0], [8.0, 8.0, 4.0]);
        let clip = copy(&grid, &region);
        assert_eq!(clip.size, [2, 2, 1]);
        let mut back = VoxelGrid::new();
        paste(&mut back, &clip, &Placement::at(&region), true);
        assert_eq!(back, grid);
    }

    #[test]
    fn transform_moves_turns_and_scales() {
        let mut grid = l_shape();
        let region = StudBox::from_center_size([4.0, 4.0, 2.0], [8.0, 8.0, 4.0]);
        // A quarter turn about Y: local X runs along world -Z.
        let placement = Placement {
            center: [44.0, 4.0, 4.0],
            size: [8.0, 8.0, 4.0],
            axes: [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        };
        transform(&mut grid, &region, &placement, false);
        assert!(grid.get([0, 0, 0]).is_air(), "source cleared");
        assert_eq!(grid.get([10, 1, 1]), Cell::new(Material::Grass, 99, 0));
        assert_eq!(grid.get([10, 0, 0]), Cell::full(Material::Rock));
        assert_eq!(grid.get([10, 0, 1]), Cell::full(Material::Rock));
        assert_eq!(grid.voxels().count(), 3);

        let clip = copy(&l_shape(), &region);
        let mut big = VoxelGrid::new();
        let doubled = Placement {
            center: [8.0, 8.0, 4.0],
            size: [16.0, 16.0, 8.0],
            axes: crate::edit::brush::IDENTITY,
        };
        paste(&mut big, &clip, &doubled, false);
        assert_eq!(big.voxels().count(), 3 * 8);
    }

    #[test]
    fn merge_empty_decides_whether_air_overwrites() {
        let mut grid = VoxelGrid::new();
        grid.set([5, 1, 0], Cell::full(Material::Snow));
        let clip = copy(
            &l_shape(),
            &StudBox::from_center_size([4.0, 4.0, 2.0], [8.0, 8.0, 4.0]),
        );
        let over = Placement {
            center: [20.0, 4.0, 2.0],
            size: [8.0, 8.0, 4.0],
            axes: crate::edit::brush::IDENTITY,
        };
        let mut kept = grid.clone();
        paste(&mut kept, &clip, &over, false);
        assert_eq!(kept.get([5, 1, 0]), Cell::full(Material::Snow));
        paste(&mut grid, &clip, &over, true);
        assert!(grid.get([5, 1, 0]).is_air());
    }
}
