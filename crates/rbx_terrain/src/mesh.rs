//! Turns voxels into triangles, one chunk at a time.
//!
//! Roblox's own mesher is not public. This one is a Surface Nets mesher over
//! voxel fill: the surface passes where fill crosses one half between two
//! voxel centres, which puts a slab of full voxels' top exactly on the voxel
//! boundary and a partly full voxel's top near `fill × 4` studs up, the way
//! Roblox draws them. Each surface vertex sits at the average crossing in its
//! cell, which rounds corners the way smooth terrain does, and its normal is
//! the fill gradient, so shading is smooth too.
//!
//! A quad takes the material of the solid voxel it bounds; materials are
//! split into separate meshes because the renderer picks textures per draw.
//! Water gets its own mesh: the surface of water and solid together, kept
//! only where water (not solid) meets air.

use crate::grid::{index_in_chunk, ChunkKey, CHUNK};
use crate::{Cell, Material, VoxelGrid, VOXEL_STUDS};

/// Triangles in world studs, indices into `positions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Surface {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkMesh {
    /// One surface per solid material present, in material slot order.
    pub solids: Vec<(Material, Surface)>,
    pub water: Surface,
}

impl ChunkMesh {
    pub fn is_empty(&self) -> bool {
        self.solids.is_empty() && self.water.is_empty()
    }
}

/// Samples per side: the chunk plus one voxel on each side.
const SIDE: i32 = CHUNK + 2;

/// The chunk's voxels and the one-voxel layer around it, copied once so the
/// mesher never goes back to the chunk map.
struct Block {
    cells: Vec<Cell>,
}

impl Block {
    fn read(grid: &VoxelGrid, key: ChunkKey) -> Block {
        let origin = key.origin();
        let mut cells = vec![Cell::AIR; (SIDE * SIDE * SIDE) as usize];
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbour = ChunkKey {
                        x: key.x + dx,
                        y: key.y + dy,
                        z: key.z + dz,
                    };
                    let Some(source) = grid.chunk(neighbour) else {
                        continue;
                    };
                    // The part of the sample block this neighbour covers,
                    // in local coordinates (-1..=CHUNK).
                    let span = |d: i32| match d {
                        -1 => (-1, -1),
                        0 => (0, CHUNK - 1),
                        _ => (CHUNK, CHUNK),
                    };
                    let (sx, sy, sz) = (span(dx), span(dy), span(dz));
                    for y in sy.0..=sy.1 {
                        for z in sz.0..=sz.1 {
                            for x in sx.0..=sx.1 {
                                let world = [origin[0] + x, origin[1] + y, origin[2] + z];
                                cells[Self::index([x, y, z])] = source[index_in_chunk(world)];
                            }
                        }
                    }
                }
            }
        }
        Block { cells }
    }

    fn index(local: [i32; 3]) -> usize {
        ((local[0] + 1) + SIDE * ((local[2] + 1) + SIDE * (local[1] + 1))) as usize
    }

    fn get(&self, local: [i32; 3]) -> Cell {
        self.cells[Self::index(local)]
    }
}

/// Which field a pass meshes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Solid,
    /// Solid and water together, so water's surface ends at the terrain.
    Wet,
}

fn density(cell: Cell, field: Field) -> f32 {
    match field {
        Field::Solid => cell.solid_fraction(),
        Field::Wet => cell.solid_fraction() + cell.water_fraction(),
    }
}

const ISO: f32 = 0.5;

/// Meshes one chunk. Its surfaces are the faces between each of its voxels
/// and the voxel above, north or east of it; together with its neighbours'
/// meshes that covers every face exactly once, with matching vertices on
/// both sides of a chunk seam.
pub fn mesh_chunk(grid: &VoxelGrid, key: ChunkKey) -> ChunkMesh {
    let block = Block::read(grid, key);
    if block.cells.iter().all(|c| c.is_air()) {
        return ChunkMesh::default();
    }
    let origin = key.origin();
    let mut solids: Vec<(Material, Builder)> = Vec::new();
    let mut solid_net = Net::new(&block, Field::Solid, origin);
    let mut water = Builder::default();
    let mut wet_net = Net::new(&block, Field::Wet, origin);
    for y in 0..CHUNK {
        for z in 0..CHUNK {
            for x in 0..CHUNK {
                let v = [x, y, z];
                for axis in 0..3 {
                    let mut n = v;
                    n[axis] += 1;
                    let (a, b) = (block.get(v), block.get(n));
                    // Solid surface.
                    let (da, db) = (density(a, Field::Solid), density(b, Field::Solid));
                    if (da >= ISO) != (db >= ISO) {
                        let inside_first = da >= ISO;
                        let material = if inside_first { a.material } else { b.material };
                        let slot = match solids.iter().position(|(m, _)| *m == material) {
                            Some(slot) => slot,
                            None => {
                                solids.push((material, Builder::default()));
                                solids.len() - 1
                            }
                        };
                        solid_net.quad(&mut solids[slot].1, v, axis, inside_first);
                    }
                    // Water surface: the wet field crosses, and the wet side
                    // is mostly water rather than solid.
                    let (wa, wb) = (density(a, Field::Wet), density(b, Field::Wet));
                    if (wa >= ISO) != (wb >= ISO) {
                        let inside = if wa >= ISO { a } else { b };
                        if inside.water_fraction() > inside.solid_fraction() {
                            wet_net.quad(&mut water, v, axis, wa >= ISO);
                        }
                    }
                }
            }
        }
    }
    solids.sort_by_key(|(m, _)| *m);
    ChunkMesh {
        solids: solids.into_iter().map(|(m, b)| (m, b.surface)).collect(),
        water: water.surface,
    }
}

/// Every chunk that can hold a surface: each stored chunk, and the chunk
/// below, south and west of it, which owns the faces against it.
pub fn meshable_chunks(grid: &VoxelGrid) -> std::collections::BTreeSet<ChunkKey> {
    let mut keys = std::collections::BTreeSet::new();
    for key in grid.chunk_keys() {
        keys.insert(key);
        keys.insert(ChunkKey {
            x: key.x - 1,
            ..key
        });
        keys.insert(ChunkKey {
            y: key.y - 1,
            ..key
        });
        keys.insert(ChunkKey {
            z: key.z - 1,
            ..key
        });
    }
    keys
}

#[derive(Default)]
struct Builder {
    surface: Surface,
    /// Cell → vertex index in this builder, so quads share vertices.
    vertices: std::collections::HashMap<[i32; 3], u32>,
}

/// The dual vertices of one field: one per cell (cube of 8 voxel centres)
/// the surface crosses, computed on first use.
struct Net<'a> {
    block: &'a Block,
    field: Field,
    origin: [i32; 3],
    cache: std::collections::HashMap<[i32; 3], ([f32; 3], [f32; 3])>,
}

impl<'a> Net<'a> {
    fn new(block: &'a Block, field: Field, origin: [i32; 3]) -> Net<'a> {
        Net {
            block,
            field,
            origin,
            cache: std::collections::HashMap::new(),
        }
    }

    /// The cell whose lowest corner is voxel `c`: its surface point in
    /// studs and its outward normal.
    fn vertex(&mut self, c: [i32; 3]) -> ([f32; 3], [f32; 3]) {
        if let Some(found) = self.cache.get(&c) {
            return *found;
        }
        let mut d = [0.0f32; 8];
        for (i, value) in d.iter_mut().enumerate() {
            let corner = [
                c[0] + (i & 1) as i32,
                c[1] + (i >> 1 & 1) as i32,
                c[2] + (i >> 2 & 1) as i32,
            ];
            *value = density(self.block.get(corner), self.field);
        }
        let mut sum = [0.0f32; 3];
        let mut crossings = 0.0;
        for (i, j) in EDGES {
            let (a, b) = (d[i], d[j]);
            if (a >= ISO) == (b >= ISO) {
                continue;
            }
            let t = (ISO - a) / (b - a);
            for (axis, s) in sum.iter_mut().enumerate() {
                let pa = (i >> axis & 1) as f32;
                let pb = (j >> axis & 1) as f32;
                *s += pa + (pb - pa) * t;
            }
            crossings += 1.0;
        }
        let local = if crossings > 0.0 {
            sum.map(|s| s / crossings)
        } else {
            [0.5; 3]
        };
        let position = std::array::from_fn(|a| {
            ((self.origin[a] + c[a]) as f32 + 0.5 + local[a]) * VOXEL_STUDS
        });
        // Fill falls outward, so the outward normal is minus its gradient.
        let g = [
            (d[1] + d[3] + d[5] + d[7]) - (d[0] + d[2] + d[4] + d[6]),
            (d[2] + d[3] + d[6] + d[7]) - (d[0] + d[1] + d[4] + d[5]),
            (d[4] + d[5] + d[6] + d[7]) - (d[0] + d[1] + d[2] + d[3]),
        ];
        let len = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
        let normal = if len > 1e-6 {
            g.map(|v| -v / len)
        } else {
            [0.0, 1.0, 0.0]
        };
        self.cache.insert(c, (position, normal));
        (position, normal)
    }

    /// The quad across the face between voxel `v` and its neighbour along
    /// `axis`, wound so its front faces out of the filled side.
    fn quad(&mut self, builder: &mut Builder, v: [i32; 3], axis: usize, inside_first: bool) {
        let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut cells = [v; 4];
        cells[1][u] -= 1;
        cells[2][u] -= 1;
        cells[2][w] -= 1;
        cells[3][w] -= 1;
        let mut ids = [0u32; 4];
        for (id, cell) in ids.iter_mut().zip(cells) {
            *id = match builder.vertices.get(&cell) {
                Some(id) => *id,
                None => {
                    let (position, normal) = self.vertex(cell);
                    let id = builder.surface.positions.len() as u32;
                    builder.surface.positions.push(position);
                    builder.surface.normals.push(normal);
                    builder.vertices.insert(cell, id);
                    id
                }
            };
        }
        let [a, b, c, d] = ids;
        // (axis, u, w) is right-handed, so a→b→c turns counter-clockwise
        // seen from +axis.
        if inside_first {
            builder.surface.indices.extend([a, b, c, a, c, d]);
        } else {
            builder.surface.indices.extend([a, c, b, a, d, c]);
        }
    }
}

/// The 12 edges of a cell as pairs of corner indices (bit 0 = x, 1 = y,
/// 2 = z).
const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

#[cfg(test)]
mod tests;
