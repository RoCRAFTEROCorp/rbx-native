//! `Terrain.PhysicsGrid`, rebuilt from the voxels whenever they change.
//!
//! Layout: version byte 2, then 3 (log2 of an 8-voxel region edge), then
//! three lists, each a big-endian u32 count followed by that many region
//! coordinates as offsets from the previous one (restarting from 0,0,0 per
//! list), encoded exactly like `SmoothGrid`'s chunk offsets.
//!
//! In a real place the first list holds, sorted by X, Y, Z, every 8³-voxel
//! region that any non-Air voxel touches once grown by one voxel on every
//! side, and the other two are empty; regenerating it from that place's
//! voxels reproduces its 544 regions exactly. A static read of Roblox's
//! reader by another project found that first-list regions are rebuilt
//! from the voxels on demand rather than trusted as stored collision data,
//! which is why this writes everything there and nothing in the other two
//! lists, whose meaning is not known.

use std::collections::BTreeSet;

use crate::grid::index_in_chunk;
use crate::VoxelGrid;

const VERSION: u8 = 2;
const REGION_LOG2: u8 = 3;
const REGION: i32 = 1 << REGION_LOG2;

/// The `PhysicsGrid` Roblox would save alongside `grid`.
pub fn encode(grid: &VoxelGrid) -> Vec<u8> {
    let regions = regions(grid);
    let mut out = Vec::with_capacity(2 + 12 + regions.len() * 12);
    out.extend([VERSION, REGION_LOG2]);
    out.extend((regions.len() as u32).to_be_bytes());
    let mut previous = [0i32; 3];
    for region in &regions {
        let delta: [u32; 3] =
            std::array::from_fn(|axis| region[axis].wrapping_sub(previous[axis]) as u32);
        previous = *region;
        for shift in [24, 16, 8, 0] {
            out.extend(delta.iter().map(|axis| (axis >> shift) as u8));
        }
    }
    out.extend([0u8; 8]);
    out
}

fn regions(grid: &VoxelGrid) -> BTreeSet<[i32; 3]> {
    let mut regions = BTreeSet::new();
    for (key, cells) in grid.chunks() {
        let origin = key.origin();
        // A chunk spans exactly four regions per axis, so only voxels on a
        // region's border can reach a neighbour; interior ones add their own.
        for y in 0..crate::CHUNK {
            for z in 0..crate::CHUNK {
                for x in 0..crate::CHUNK {
                    if cells[index_in_chunk([x, y, z])].is_air() {
                        continue;
                    }
                    let voxel = [origin[0] + x, origin[1] + y, origin[2] + z];
                    let low = voxel.map(|v| (v - 1).div_euclid(REGION));
                    let high = voxel.map(|v| (v + 1).div_euclid(REGION));
                    for rx in low[0]..=high[0] {
                        for ry in low[1]..=high[1] {
                            for rz in low[2]..=high[2] {
                                regions.insert([rx, ry, rz]);
                            }
                        }
                    }
                }
            }
        }
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cell, Material};

    #[test]
    fn empty_terrain_matches_a_fresh_place() {
        // Every empty-terrain place in the fixtures stores exactly this.
        assert_eq!(
            encode(&VoxelGrid::new()),
            vec![2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn a_voxel_inside_a_region_indexes_only_it() {
        let mut grid = VoxelGrid::new();
        grid.set([3, 3, 3], Cell::full(Material::Rock));
        let bytes = encode(&grid);
        assert_eq!(&bytes[2..6], &[0, 0, 0, 1]);
        assert_eq!(&bytes[6..18], &[0; 12]);
        assert_eq!(bytes.len(), 2 + 4 + 12 + 8);
    }

    #[test]
    fn a_border_voxel_reaches_its_neighbours() {
        let mut grid = VoxelGrid::new();
        grid.set([0, 7, 0], Cell::full(Material::Rock));
        let regions = regions(&grid);
        // x: -1..=0, y: 0..=1, z: -1..=0.
        assert_eq!(regions.len(), 8);
        assert!(regions.contains(&[-1, 1, -1]));
        let bytes = encode(&grid);
        // The first region is (-1, 0, -1): all three offsets negative.
        assert_eq!(
            &bytes[6..18],
            &[0xff, 0, 0xff, 0xff, 0, 0xff, 0xff, 0, 0xff, 0xff, 0, 0xff]
        );
    }
}
