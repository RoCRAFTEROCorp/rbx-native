//! Where blades grow: the grassy, upward-facing, dry triangles of a chunk's
//! solid surface ([`patches`]), and blades scattered over them ([`scatter`]).
//!
//! Everything random is seeded from a triangle's own corners, so a chunk
//! re-meshed into the same triangles grows the same blades, and an edit to
//! one chunk never moves a blade in another.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use rbx_terrain::mesh::{Blend, ChunkMesh};
use rbx_terrain::{Material, VoxelGrid, CHUNK, VOXEL_STUDS};

/// Blades per square stud of surface at full density. Roblox's grass is a
/// carpet: at a few per stud the ground under it barely shows near the eye.
const BLADES_PER_STUD: f32 = 4.0;
/// Below this upward component a slope grows nothing; full density from
/// `FLAT` up. Roblox's grass thins out on hillsides and leaves cliffs bare.
const STEEP: f32 = 0.6;
const FLAT: f32 = 0.8;
/// The Grass weight band a blade's odds rise across: the terrain shader
/// draws the material border about where Grass crosses one half, and the
/// blades stop with it.
const BORDER: Range<f32> = 0.35..0.65;
/// Side of the square columns a chunk's blades are grouped into, in studs:
/// the unit of culling and of distance thinning.
const TILE: f32 = 16.0;
const TILES: usize = (CHUNK as f32 * VOXEL_STUDS / TILE) as usize;

/// One grassy triangle of the solid surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::renderer::terrain) struct Patch {
    corners: [Vec3; 3],
    /// Each corner's Grass weight, 0 to 1.
    grass: [f32; 3],
    /// How flat it is, as the mean of its normals' upward components.
    up: f32,
}

/// One blade, as the grass shader's per-instance record.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct BladeRaw {
    pub(super) root: [f32; 3],
    /// Yaw, height variation, sway offset and rank, each 0 to 255. A blade
    /// is drawn while its rank is under the density wanted where it stands.
    pub(super) shape: [u8; 4],
}

/// A column of blades, a contiguous range of the chunk's instance buffer
/// sorted by rank so any prefix of it is an even thinning.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Tile {
    pub(super) blades: Range<u32>,
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}

fn grass_weight(blend: &Blend) -> f32 {
    blend
        .materials
        .iter()
        .zip(blend.weights)
        .filter(|(m, _)| **m == Material::Grass)
        .map(|(_, w)| w)
        .sum()
}

fn smoothstep(edge: Range<f32>, x: f32) -> f32 {
    let t = ((x - edge.start) / (edge.end - edge.start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The triangles of `mesh` that can grow grass. Water above a triangle
/// drowns it: Roblox draws no grass on a lake bed.
pub(in crate::renderer::terrain) fn patches(mesh: &ChunkMesh, grid: &VoxelGrid) -> Vec<Patch> {
    let surface = &mesh.solid;
    surface
        .indices
        .chunks_exact(3)
        .filter_map(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|i| triangle[i] as usize);
            let grass = [a, b, c].map(|i| grass_weight(&mesh.blends[i]));
            let up = [a, b, c].iter().map(|&i| surface.normals[i][1]).sum::<f32>() / 3.0;
            if grass.iter().all(|&w| w <= BORDER.start) || up <= STEEP {
                return None;
            }
            let corners = [a, b, c].map(|i| Vec3::from(surface.positions[i]));
            let above = (corners[0] + corners[1] + corners[2]) / 3.0 + Vec3::Y * 2.0;
            let voxel = (above / VOXEL_STUDS).floor().as_ivec3().to_array();
            (grid.get(voxel).water_fraction() == 0.0).then_some(Patch {
                corners,
                grass,
                up,
            })
        })
        .collect()
}

/// SplitMix64: tiny, and plenty for scattering.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}

/// A seed from the triangle's corners in any order, so the index order the
/// mesher happened to emit never matters.
fn seed(corners: &[Vec3; 3]) -> u64 {
    corners
        .iter()
        .map(|c| {
            let q = (*c * 16.0).round().as_ivec3();
            let mut rng = Rng(q.x as u64 ^ ((q.y as u64) << 21) ^ ((q.z as u64) << 42));
            rng.next()
        })
        .fold(0u64, u64::wrapping_add)
}

/// Every blade `patches` grows at full density, grouped into tiles of the
/// chunk whose surface starts at `origin` (in studs).
pub(super) fn scatter(patches: &[Patch], origin: Vec3) -> (Vec<BladeRaw>, Vec<Tile>) {
    let mut blades: Vec<(usize, BladeRaw)> = Vec::new();
    for patch in patches {
        let [a, b, c] = patch.corners;
        let area = 0.5 * (b - a).cross(c - a).length();
        let mut rng = Rng(seed(&patch.corners));
        let count = (area * BLADES_PER_STUD + rng.unit()) as usize;
        let slope = smoothstep(STEEP..FLAT, patch.up);
        for _ in 0..count {
            // Uniform over the triangle.
            let (r1, r2) = (rng.unit().sqrt(), rng.unit());
            let w = [1.0 - r1, r1 * (1.0 - r2), r1 * r2];
            let grass = w[0] * patch.grass[0] + w[1] * patch.grass[1] + w[2] * patch.grass[2];
            let keep = smoothstep(BORDER, grass) * slope;
            let root = a * w[0] + b * w[1] + c * w[2];
            let shape = [rng.byte(), rng.byte(), rng.byte(), rng.byte()];
            if rng.unit() >= keep {
                continue;
            }
            let local = ((root - origin) / TILE).floor();
            let column = |v: f32| (v.max(0.0) as usize).min(TILES - 1);
            let tile = column(local.x) + column(local.z) * TILES;
            blades.push((
                tile,
                BladeRaw {
                    root: root.to_array(),
                    shape,
                },
            ));
        }
    }
    blades.sort_by_key(|(tile, blade)| (*tile, blade.shape[3]));

    let mut tiles: Vec<Tile> = Vec::new();
    let mut last = usize::MAX;
    for (i, (tile, blade)) in blades.iter().enumerate() {
        let root = Vec3::from(blade.root);
        if *tile != last {
            last = *tile;
            tiles.push(Tile {
                blades: i as u32..i as u32,
                min: root,
                max: root,
            });
        }
        if let Some(open) = tiles.last_mut() {
            open.blades.end = i as u32 + 1;
            open.min = open.min.min(root);
            open.max = open.max.max(root);
        }
    }
    (blades.into_iter().map(|(_, blade)| blade).collect(), tiles)
}

/// The share of blades drawn `studs` from the eye: all of `density` out to
/// a third of `reach`, thinning to none at `reach`. `grass.wgsl`'s
/// `grass_density` is the same curve, per blade.
pub(super) fn density_at(studs: f32, reach: f32, density: f32) -> f32 {
    if reach <= 0.0 {
        return 0.0;
    }
    density * ((reach - studs) / (reach * (2.0 / 3.0))).clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "scatter/tests.rs"]
mod tests;
