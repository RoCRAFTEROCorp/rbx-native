//! The sparse voxel grid: 32³-voxel chunks keyed by chunk coordinate, the
//! layout `SmoothGrid` serializes and Roblox keeps in memory.

use std::collections::{BTreeMap, BTreeSet};

use crate::Cell;

/// Voxels along one chunk edge (`SmoothGrid`'s header stores log2 of it: 5).
pub const CHUNK: i32 = 32;
pub(crate) const CHUNK_CELLS: usize = (CHUNK * CHUNK * CHUNK) as usize;
/// Studs along one voxel edge. Voxel `(0, 0, 0)` spans studs 0..4 on every
/// axis, so voxel `i`'s centre is at `4i + 2`.
pub const VOXEL_STUDS: f32 = 4.0;

/// A chunk's position in chunks. Ordering is X, then Y, then Z: the order
/// Roblox writes chunks in (a real place's `SmoothGrid` re-encodes byte for
/// byte with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ChunkKey {
    pub fn containing(voxel: [i32; 3]) -> ChunkKey {
        ChunkKey {
            x: voxel[0].div_euclid(CHUNK),
            y: voxel[1].div_euclid(CHUNK),
            z: voxel[2].div_euclid(CHUNK),
        }
    }

    /// The chunk's lowest voxel.
    pub fn origin(self) -> [i32; 3] {
        [self.x * CHUNK, self.y * CHUNK, self.z * CHUNK]
    }
}

/// Index of a voxel inside its chunk: X fastest, then Z, then Y. The order is
/// what keeps a real place's terrain continuous across chunk seams on all
/// three axes; any other assignment tears it.
pub(crate) fn index_in_chunk(voxel: [i32; 3]) -> usize {
    let local = |v: i32| v.rem_euclid(CHUNK) as usize;
    local(voxel[0]) + local(voxel[2]) * CHUNK as usize + local(voxel[1]) * (CHUNK * CHUNK) as usize
}

/// The inverse of [`index_in_chunk`], as an offset from the chunk origin.
pub(crate) fn local_of_index(index: usize) -> [i32; 3] {
    let edge = CHUNK as usize;
    [
        (index % edge) as i32,
        (index / (edge * edge)) as i32,
        (index / edge % edge) as i32,
    ]
}

/// A sparse grid of voxels. Chunks that are all Air are never kept, so an
/// empty grid is an empty map and serializes to the two header bytes.
///
/// Memory: a stored chunk is 32³ three-byte cells (96 KiB) whatever it holds.
// ponytail: dense chunks only; a large place whose interior is solid rock
// pays full price for it. Add a uniform-chunk variant if memory ever bites.
#[derive(Clone, Debug, Default)]
pub struct VoxelGrid {
    chunks: BTreeMap<ChunkKey, Box<[Cell]>>,
    /// Chunks whose voxels changed since the last [`VoxelGrid::take_dirty`],
    /// so a mesher can rebuild only those (and their neighbours).
    dirty: BTreeSet<ChunkKey>,
}

/// Two grids are equal when they hold the same voxels; pending dirty marks
/// are bookkeeping, not content.
impl PartialEq for VoxelGrid {
    fn eq(&self, other: &Self) -> bool {
        self.chunks().eq(other.chunks())
    }
}

impl Eq for VoxelGrid {}

impl VoxelGrid {
    pub fn new() -> VoxelGrid {
        VoxelGrid::default()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks
            .values()
            .all(|cells| cells.iter().all(|c| c.is_air()))
    }

    /// Stored chunks, including any emptied since the last [`VoxelGrid::prune`].
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// The voxel box the stored chunks span, `(min, max)` with `max`
    /// exclusive: a cheap, loose stand-in for [`VoxelGrid::bounds`].
    pub fn chunk_bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let mut keys = self.chunks.keys();
        let first = keys.next()?.origin();
        let mut min = first;
        let mut max = first.map(|v| v + CHUNK);
        for key in keys {
            let origin = key.origin();
            for a in 0..3 {
                min[a] = min[a].min(origin[a]);
                max[a] = max[a].max(origin[a] + CHUNK);
            }
        }
        Some((min, max))
    }

    pub fn get(&self, voxel: [i32; 3]) -> Cell {
        self.chunks
            .get(&ChunkKey::containing(voxel))
            .map_or(Cell::AIR, |chunk| chunk[index_in_chunk(voxel)])
    }

    /// Emptying a chunk this way leaves it stored (checking every write for
    /// a newly empty chunk would cost a 32³ scan each); [`VoxelGrid::prune`]
    /// drops such chunks, and nothing that reads the grid tells them apart.
    pub fn set(&mut self, voxel: [i32; 3], cell: Cell) {
        let key = ChunkKey::containing(voxel);
        let index = index_in_chunk(voxel);
        match self.chunks.get_mut(&key) {
            Some(chunk) => {
                if chunk[index] == cell {
                    return;
                }
                chunk[index] = cell;
            }
            None if cell.is_air() => return,
            None => {
                let mut chunk = vec![Cell::AIR; CHUNK_CELLS].into_boxed_slice();
                chunk[index] = cell;
                self.chunks.insert(key, chunk);
            }
        }
        self.mark_dirty(key, voxel);
    }

    /// Marks `key`, and the neighbours whose surface meshes read `voxel`: a
    /// chunk's mesh depends on its own voxels and on the one-voxel layer
    /// around it, so a voxel on a chunk face (edge, corner) also dirties the
    /// chunk(s) across it.
    fn mark_dirty(&mut self, key: ChunkKey, voxel: [i32; 3]) {
        let reach = |v: i32| -> (i32, i32) {
            match v.rem_euclid(CHUNK) {
                0 => (-1, 0),
                l if l == CHUNK - 1 => (0, 1),
                _ => (0, 0),
            }
        };
        let (rx, ry, rz) = (reach(voxel[0]), reach(voxel[1]), reach(voxel[2]));
        for dx in rx.0..=rx.1 {
            for dy in ry.0..=ry.1 {
                for dz in rz.0..=rz.1 {
                    self.dirty.insert(ChunkKey {
                        x: key.x + dx,
                        y: key.y + dy,
                        z: key.z + dz,
                    });
                }
            }
        }
    }

    fn mark_with_neighbours(&mut self, key: ChunkKey) {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    self.dirty.insert(ChunkKey {
                        x: key.x + dx,
                        y: key.y + dy,
                        z: key.z + dz,
                    });
                }
            }
        }
    }

    /// Drops stored chunks that edits have emptied.
    pub fn prune(&mut self) {
        self.chunks
            .retain(|_, cells| cells.iter().any(|c| !c.is_air()));
    }

    /// Stores a whole chunk read from a file. All-Air chunks are dropped, the
    /// same as an edit that empties one.
    pub(crate) fn insert_chunk(&mut self, key: ChunkKey, cells: Box<[Cell]>) {
        debug_assert_eq!(cells.len(), CHUNK_CELLS);
        if cells.iter().all(|c| c.is_air()) {
            self.chunks.remove(&key);
        } else {
            self.chunks.insert(key, cells);
        }
        self.mark_with_neighbours(key);
    }

    pub(crate) fn chunk(&self, key: ChunkKey) -> Option<&[Cell]> {
        self.chunks.get(&key).map(|c| &c[..])
    }

    pub fn chunk_keys(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.chunks.keys().copied()
    }

    /// Stored chunks that hold any terrain.
    pub(crate) fn chunks(&self) -> impl Iterator<Item = (ChunkKey, &[Cell])> {
        self.chunks
            .iter()
            .filter(|(_, c)| c.iter().any(|cell| !cell.is_air()))
            .map(|(k, c)| (*k, &c[..]))
    }

    /// Every non-Air voxel with its coordinate.
    pub fn voxels(&self) -> impl Iterator<Item = ([i32; 3], Cell)> + '_ {
        self.chunks.iter().flat_map(|(key, cells)| {
            let origin = key.origin();
            cells
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_air())
                .map(move |(index, cell)| {
                    let local = local_of_index(index);
                    (
                        [
                            origin[0] + local[0],
                            origin[1] + local[1],
                            origin[2] + local[2],
                        ],
                        *cell,
                    )
                })
        })
    }

    /// Which material slots appear among the voxels of `keys` (every stored
    /// chunk for `None`): what a renderer needs textures for, without
    /// walking the whole grid after an edit that touched a few chunks.
    pub fn materials_in(&self, keys: Option<&BTreeSet<ChunkKey>>) -> [bool; 23] {
        let mut present = [false; 23];
        let mut mark = |cells: &[Cell]| {
            for cell in cells {
                present[usize::from(cell.material.slot())] = true;
            }
        };
        match keys {
            None => self.chunks.values().for_each(|cells| mark(cells)),
            Some(keys) => keys
                .iter()
                .filter_map(|key| self.chunks.get(key))
                .for_each(|cells| mark(cells)),
        }
        present
    }

    /// The smallest voxel box holding every non-Air voxel, as `(min, max)`
    /// with `max` exclusive, or `None` for an empty grid.
    pub fn bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let mut bounds: Option<([i32; 3], [i32; 3])> = None;
        for (voxel, _) in self.voxels() {
            let (min, max) = bounds.get_or_insert((voxel, voxel.map(|v| v + 1)));
            for axis in 0..3 {
                min[axis] = min[axis].min(voxel[axis]);
                max[axis] = max[axis].max(voxel[axis] + 1);
            }
        }
        bounds
    }

    pub fn clear(&mut self) {
        let keys: Vec<ChunkKey> = self.chunks.keys().copied().collect();
        for key in keys {
            self.mark_with_neighbours(key);
        }
        self.chunks.clear();
    }

    /// The chunks whose surface meshes differ between `self` and `other`:
    /// what a renderer holding meshes of one has to rebuild to show the
    /// other, without the edit history `take_dirty` keeps.
    pub fn changed_meshes(&self, other: &VoxelGrid) -> BTreeSet<ChunkKey> {
        let keys: BTreeSet<ChunkKey> = self
            .chunks
            .keys()
            .chain(other.chunks.keys())
            .copied()
            .collect();
        let mut marks = VoxelGrid::new();
        for key in keys {
            let (a, b) = (self.chunks.get(&key), other.chunks.get(&key));
            if a == b {
                continue;
            }
            let origin = key.origin();
            for index in 0..CHUNK_CELLS {
                let before = a.map_or(Cell::AIR, |cells| cells[index]);
                let after = b.map_or(Cell::AIR, |cells| cells[index]);
                if before != after {
                    let local = local_of_index(index);
                    marks.mark_dirty(
                        key,
                        [
                            origin[0] + local[0],
                            origin[1] + local[1],
                            origin[2] + local[2],
                        ],
                    );
                }
            }
        }
        marks.dirty
    }

    /// The chunks changed since the last call. A chunk emptied by an edit is
    /// included even though it is no longer stored.
    pub fn take_dirty(&mut self) -> BTreeSet<ChunkKey> {
        std::mem::take(&mut self.dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Material;

    #[test]
    fn index_is_x_then_z_then_y() {
        assert_eq!(index_in_chunk([1, 0, 0]), 1);
        assert_eq!(index_in_chunk([0, 0, 1]), 32);
        assert_eq!(index_in_chunk([0, 1, 0]), 1024);
        assert_eq!(index_in_chunk([-1, -1, -1]), 31 + 31 * 32 + 31 * 1024);
        for index in [0, 1, 33, 1057, CHUNK_CELLS - 1] {
            assert_eq!(index_in_chunk(local_of_index(index)), index);
        }
    }

    #[test]
    fn negative_voxels_land_in_negative_chunks() {
        assert_eq!(
            ChunkKey::containing([-1, 31, 32]),
            ChunkKey { x: -1, y: 0, z: 1 }
        );
        assert_eq!(ChunkKey { x: -1, y: 0, z: 1 }.origin(), [-32, 0, 32]);
    }

    #[test]
    fn emptying_a_chunk_empties_the_grid_and_marks_it_dirty() {
        let mut grid = VoxelGrid::new();
        grid.set([-5, 3, 40], Cell::full(Material::Rock));
        assert_eq!(grid.chunk_count(), 1);
        assert_eq!(grid.get([-5, 3, 40]), Cell::full(Material::Rock));
        assert_eq!(grid.bounds(), Some(([-5, 3, 40], [-4, 4, 41])));
        assert_eq!(grid.chunk_bounds(), Some(([-32, 0, 32], [0, 32, 64])));
        grid.take_dirty();
        grid.set([-5, 3, 40], Cell::AIR);
        assert!(grid.is_empty());
        assert_eq!(grid, VoxelGrid::new());
        assert_eq!(
            grid.take_dirty().into_iter().collect::<Vec<_>>(),
            vec![ChunkKey { x: -1, y: 0, z: 1 }]
        );
        grid.prune();
        assert_eq!(grid.chunk_count(), 0);
    }

    #[test]
    fn a_voxel_on_a_chunk_face_dirties_the_chunk_across_it() {
        let mut grid = VoxelGrid::new();
        grid.set([0, 5, 31], Cell::full(Material::Rock));
        let dirty: Vec<ChunkKey> = grid.take_dirty().into_iter().collect();
        assert_eq!(
            dirty,
            vec![
                ChunkKey { x: -1, y: 0, z: 0 },
                ChunkKey { x: -1, y: 0, z: 1 },
                ChunkKey { x: 0, y: 0, z: 0 },
                ChunkKey { x: 0, y: 0, z: 1 },
            ]
        );
    }

    #[test]
    fn changed_meshes_matches_what_the_edits_dirtied() {
        let mut before = VoxelGrid::new();
        before.set([3, 3, 3], Cell::full(Material::Rock));
        before.set([40, 3, 3], Cell::full(Material::Rock));
        let mut after = before.clone();
        after.take_dirty();
        after.set([0, 3, 3], Cell::full(Material::Sand));
        after.set([40, 3, 3], Cell::AIR);
        assert_eq!(before.changed_meshes(&after), after.take_dirty());
        assert!(before.changed_meshes(&before).is_empty());
    }

    #[test]
    fn writing_the_same_cell_is_not_a_change() {
        let mut grid = VoxelGrid::new();
        grid.set([0, 0, 0], Cell::AIR);
        assert!(grid.take_dirty().is_empty());
        grid.set([0, 0, 0], Cell::full(Material::Sand));
        grid.take_dirty();
        grid.set([0, 0, 0], Cell::full(Material::Sand));
        assert!(grid.take_dirty().is_empty());
    }
}
