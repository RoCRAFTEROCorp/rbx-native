//! The Generate tool: procedural terrain in the selected region from the
//! biomes ticked, how much they blend, whether to carve caves, how large a
//! biome is, and a seed (`studio/terrain-editor.md`).
//!
//! Roblox does not publish its generator, so the same seed will not give
//! Studio's terrain. What is kept is the contract: the nine documented
//! biomes, a deterministic result per seed and settings, smoother borders
//! with more blending, larger patches with a larger biome size, and caves
//! only when asked.

use crate::edit::region::StudBox;
use crate::noise::{fbm2, noise3, ridged2, unit};
use crate::{Cell, Material, VoxelGrid, VOXEL_STUDS};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    Water,
    Plains,
    Dunes,
    Mountains,
    Arctic,
    Marsh,
    Hills,
    Canyons,
    Lavascape,
}

impl Biome {
    pub const ALL: [Biome; 9] = [
        Biome::Arctic,
        Biome::Dunes,
        Biome::Canyons,
        Biome::Lavascape,
        Biome::Water,
        Biome::Mountains,
        Biome::Hills,
        Biome::Plains,
        Biome::Marsh,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Biome::Water => "Water",
            Biome::Plains => "Plains",
            Biome::Dunes => "Dunes",
            Biome::Mountains => "Mountains",
            Biome::Arctic => "Arctic",
            Biome::Marsh => "Marsh",
            Biome::Hills => "Hills",
            Biome::Canyons => "Canyons",
            Biome::Lavascape => "Lavascape",
        }
    }

    /// Surface height as a fraction of the region's height, at `p` in units
    /// of the biome size.
    fn height(self, seed: u32, p: [f32; 2]) -> f32 {
        let [x, z] = p;
        match self {
            Biome::Water => 0.12 + 0.06 * fbm2(seed, x * 3.0, z * 3.0, 3),
            Biome::Marsh => SEA_LEVEL + 0.015 * fbm2(seed, x * 8.0, z * 8.0, 3),
            Biome::Plains => 0.42 + 0.03 * fbm2(seed, x * 2.0, z * 2.0, 4),
            Biome::Hills => 0.45 + 0.14 * fbm2(seed, x * 3.0, z * 3.0, 4),
            Biome::Arctic => 0.44 + 0.06 * fbm2(seed, x * 4.0, z * 4.0, 4),
            Biome::Lavascape => 0.44 + 0.12 * fbm2(seed, x * 3.0, z * 3.0, 5),
            Biome::Mountains => 0.45 + 0.5 * ridged2(seed, x * 1.5, z * 1.5, 5),
            Biome::Dunes => {
                let warp = fbm2(seed, x * 2.0, z * 2.0, 2) * 3.0;
                0.42 + 0.07 * ((x * 9.0 + warp).sin() * 0.5 + 0.5).powf(1.5)
            }
            Biome::Canyons => {
                let mesa = 0.66 + 0.04 * fbm2(seed, x * 2.0, z * 2.0, 3);
                let river = 1.0 - ridged2(seed ^ 0x5bd1, x * 1.2, z * 1.2, 3);
                // Flat mesas cut by steep, stepped channels.
                let cut = ((river - 0.55) / 0.2).clamp(0.0, 1.0);
                let floor = SEA_LEVEL + 0.02;
                let height = floor + (mesa - floor) * cut;
                (height * 24.0).round() / 24.0
            }
        }
    }

    /// The material at `depth` voxels under the surface, on ground at
    /// `height` (a fraction of the region) with a slope of `steep` (rise per
    /// run).
    fn material(self, seed: u32, voxel: [i32; 3], depth: i32, height: f32, steep: f32) -> Material {
        let speckle = unit(seed, voxel[0], voxel[1], voxel[2]);
        let deep = depth >= 3;
        match self {
            Biome::Water | Biome::Dunes => {
                if deep && self == Biome::Water {
                    Material::Rock
                } else if deep {
                    Material::Sandstone
                } else {
                    Material::Sand
                }
            }
            Biome::Marsh => match (deep, speckle) {
                (true, _) => Material::Ground,
                (_, s) if s < 0.35 => Material::LeafyGrass,
                _ => Material::Mud,
            },
            Biome::Plains | Biome::Hills => {
                if deep {
                    Material::Rock
                } else if steep > 1.2 {
                    Material::Rock
                } else if depth > 0 {
                    Material::Ground
                } else if self == Biome::Hills && speckle < 0.2 {
                    Material::LeafyGrass
                } else {
                    Material::Grass
                }
            }
            Biome::Mountains => {
                if deep || steep > 1.0 {
                    if speckle < 0.3 {
                        Material::Slate
                    } else {
                        Material::Rock
                    }
                } else if height > 0.8 {
                    Material::Snow
                } else if height > 0.6 {
                    Material::Rock
                } else {
                    Material::Grass
                }
            }
            Biome::Arctic => {
                if deep {
                    Material::Glacier
                } else if speckle < 0.15 {
                    Material::Ice
                } else {
                    Material::Snow
                }
            }
            Biome::Canyons => {
                if steep > 0.8 || deep {
                    if (voxel[1] / 2) % 2 == 0 {
                        Material::Sandstone
                    } else {
                        Material::Limestone
                    }
                } else {
                    Material::Sand
                }
            }
            Biome::Lavascape => {
                if height < 0.42 && depth == 0 {
                    Material::CrackedLava
                } else {
                    Material::Basalt
                }
            }
        }
    }
}

/// Water fills empty space below this fraction of the region's height.
const SEA_LEVEL: f32 = 0.35;

#[derive(Clone, Debug, PartialEq)]
pub struct GenerateSettings {
    pub biomes: Vec<Biome>,
    /// 0 (hard borders) to 1 (wide, smooth transitions).
    pub blending: f32,
    pub caves: bool,
    /// Studs across one biome patch.
    pub biome_size: f32,
    pub seed: u32,
}

impl Default for GenerateSettings {
    fn default() -> Self {
        GenerateSettings {
            biomes: vec![Biome::Mountains, Biome::Hills, Biome::Plains],
            blending: 0.5,
            caves: false,
            biome_size: 256.0,
            seed: 618_033,
        }
    }
}

/// The biomes nearby a column and how much each contributes: one jittered
/// site per biome-size cell, weighted by how much closer it is than the
/// nearest (so blending widens the band where two sites compete).
fn biome_weights(settings: &GenerateSettings, p: [f32; 2]) -> Vec<(Biome, f32)> {
    let cell = p.map(|v| v.floor() as i32);
    let mut sites: Vec<(f32, Biome)> = Vec::with_capacity(9);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (cx, cz) = (cell[0] + dx, cell[1] + dz);
            let site = [
                cx as f32 + unit(settings.seed, cx, cz, 1),
                cz as f32 + unit(settings.seed, cx, cz, 2),
            ];
            let pick = unit(settings.seed, cx, cz, 3) * settings.biomes.len() as f32;
            let biome = settings.biomes[(pick as usize).min(settings.biomes.len() - 1)];
            let d = ((p[0] - site[0]).powi(2) + (p[1] - site[1]).powi(2)).sqrt();
            sites.push((d, biome));
        }
    }
    let nearest = sites.iter().map(|s| s.0).fold(f32::INFINITY, f32::min);
    let width = 0.02 + settings.blending.clamp(0.0, 1.0) * 0.3;
    let mut weights: Vec<(Biome, f32)> = Vec::new();
    for (d, biome) in sites {
        let w = (-(d - nearest) / width).exp();
        match weights.iter_mut().find(|(b, _)| *b == biome) {
            Some(entry) => entry.1 += w,
            None => weights.push((biome, w)),
        }
    }
    let total: f32 = weights.iter().map(|w| w.1).sum();
    weights.iter_mut().for_each(|w| w.1 /= total);
    weights
}

/// Replaces the terrain in `region` (snapped to voxels) with generated
/// terrain. Does nothing with no biome selected.
pub fn generate(grid: &mut VoxelGrid, region: &StudBox, settings: &GenerateSettings) {
    if settings.biomes.is_empty() {
        return;
    }
    let region = region.snapped();
    let (min, max) = region.voxels();
    crate::edit::region::delete(grid, &region);
    let size = region.size();
    let scale = settings.biome_size.max(VOXEL_STUDS);
    let column = |x: i32, z: i32| -> (f32, Biome) {
        let p = [
            (x as f32 + 0.5) * VOXEL_STUDS / scale,
            (z as f32 + 0.5) * VOXEL_STUDS / scale,
        ];
        let weights = biome_weights(settings, p);
        let height = weights
            .iter()
            .map(|(b, w)| b.height(settings.seed, p) * w)
            .sum();
        let main = weights
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(settings.biomes[0], |w| w.0);
        (height, main)
    };
    let water = settings
        .biomes
        .iter()
        .any(|b| matches!(b, Biome::Water | Biome::Marsh));
    let sea = region.min[1] + SEA_LEVEL * size[1];
    for z in min[2]..max[2] {
        for x in min[0]..max[0] {
            let (height, biome) = column(x, z);
            let (east, _) = column(x + 1, z);
            let (south, _) = column(x, z + 1);
            let steep = ((east - height).abs().max((south - height).abs()) * size[1]) / VOXEL_STUDS;
            let top = region.min[1] + height.clamp(0.0, 1.0) * size[1];
            for y in min[1]..max[1] {
                let bottom = y as f32 * VOXEL_STUDS;
                let fill = ((top - bottom) / VOXEL_STUDS).clamp(0.0, 1.0);
                let voxel = [x, y, z];
                if fill <= 0.0 {
                    if water && bottom < sea {
                        let level = ((sea - bottom) / VOXEL_STUDS).clamp(0.0, 1.0);
                        grid.set(voxel, Cell::with_fraction(Material::Water, level));
                        continue;
                    }
                    break;
                }
                let depth = ((top - bottom) / VOXEL_STUDS) as i32;
                if settings.caves && depth > 2 && y > min[1] && is_cave(settings.seed, voxel) {
                    continue;
                }
                let material = biome.material(settings.seed, voxel, depth, height, steep);
                grid.set(voxel, Cell::with_fraction(material, fill));
            }
        }
    }
}

/// Worm-like tunnels where two 3D noise fields both cross zero.
fn is_cave(seed: u32, voxel: [i32; 3]) -> bool {
    let p = voxel.map(|v| v as f32 * VOXEL_STUDS / 48.0);
    let a = noise3(seed ^ 0x00ca_5e01, p);
    let b = noise3(seed ^ 0x00ca_5e02, [p[0], p[1] * 1.6, p[2]]);
    a.abs() < 0.08 && b.abs() < 0.08
}

#[cfg(test)]
mod tests;
