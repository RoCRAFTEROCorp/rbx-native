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

use crate::VoxelGrid;

const VERSION: u8 = 2;
const REGION_LOG2: u8 = 3;
const REGION: i32 = 1 << REGION_LOG2;

/// The `PhysicsGrid` Roblox would save alongside `grid`.
pub fn encode(grid: &VoxelGrid) -> Vec<u8> {
    Encoder::default().encode(grid)
}

/// [`encode`] that remembers each chunk's regions by revision, so encoding
/// again after an edit only scans the chunks the edit touched.
#[derive(Debug, Default)]
pub struct Encoder {
    chunks: std::collections::HashMap<crate::ChunkKey, (u64, Vec<[i32; 3]>)>,
}

impl Encoder {
    pub fn encode(&mut self, grid: &VoxelGrid) -> Vec<u8> {
        let mut regions = BTreeSet::new();
        let mut kept = std::collections::HashSet::new();
        for (key, cells, revision) in grid.revised_chunks() {
            kept.insert(key);
            let entry = self.chunks.entry(key).or_insert((0, Vec::new()));
            if entry.0 != revision {
                *entry = (revision, chunk_regions(key, cells));
            }
            regions.extend(entry.1.iter().copied());
        }
        self.chunks.retain(|key, _| kept.contains(key));
        write(&regions)
    }
}

fn write(regions: &BTreeSet<[i32; 3]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 12 + regions.len() * 12);
    out.extend([VERSION, REGION_LOG2]);
    out.extend((regions.len() as u32).to_be_bytes());
    let mut previous = [0i32; 3];
    for region in regions {
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

#[cfg(test)]
fn regions(grid: &VoxelGrid) -> BTreeSet<[i32; 3]> {
    grid.chunks()
        .flat_map(|(key, cells)| chunk_regions(key, cells))
        .collect()
}

/// Which of the 6×6×6 regions around one chunk (its own 4×4×4 plus a ring,
/// since a voxel on a region's face reaches the one beside it) any of its
/// voxels touches. A flag per region rather than a set insert per voxel: a
/// large map is tens of millions of voxels and a few thousand regions.
fn chunk_regions(key: crate::ChunkKey, cells: &[crate::Cell]) -> Vec<[i32; 3]> {
    const SIDE: usize = 6;
    let mut touched = [false; SIDE * SIDE * SIDE];
    let mut index = 0;
    for y in 0..crate::CHUNK {
        let ys = reach(y);
        for z in 0..crate::CHUNK {
            let zs = reach(z);
            for x in 0..crate::CHUNK {
                if !cells[index].is_air() {
                    let xs = reach(x);
                    for ry in ys.0..=ys.1 {
                        for rz in zs.0..=zs.1 {
                            for rx in xs.0..=xs.1 {
                                touched[rx + SIDE * (rz + SIDE * ry)] = true;
                            }
                        }
                    }
                }
                index += 1;
            }
        }
    }
    let base = key.origin().map(|v| v.div_euclid(REGION) - 1);
    let mut regions = Vec::new();
    for ry in 0..SIDE {
        for rz in 0..SIDE {
            for rx in 0..SIDE {
                if touched[rx + SIDE * (rz + SIDE * ry)] {
                    regions.push([
                        base[0] + rx as i32,
                        base[1] + ry as i32,
                        base[2] + rz as i32,
                    ]);
                }
            }
        }
    }
    regions
}

/// The regions, numbered from one before the chunk's first, that a voxel at
/// chunk-local `v` reaches once grown by one: its own, and the neighbour
/// across a face it sits on.
fn reach(v: i32) -> (usize, usize) {
    let low = (v - 1).div_euclid(REGION) + 1;
    let high = (v + 1).div_euclid(REGION) + 1;
    (low as usize, high as usize)
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
    fn the_caching_encoder_matches_a_fresh_one_after_edits() {
        let mut grid = VoxelGrid::new();
        grid.set([3, 3, 3], Cell::full(Material::Rock));
        grid.set([40, 3, 3], Cell::full(Material::Rock));
        let mut encoder = Encoder::default();
        assert_eq!(encoder.encode(&grid), encode(&grid));
        grid.set([0, 7, 0], Cell::full(Material::Sand));
        grid.set([40, 3, 3], Cell::AIR);
        assert_eq!(encoder.encode(&grid), encode(&grid));
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
