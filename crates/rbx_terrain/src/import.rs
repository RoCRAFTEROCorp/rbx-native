//! The Import tool: a heightmap, and optionally a colormap, applied to the
//! selected region.
//!
//! Following `parts/terrain.md`: brighter is higher, the darkest value sits
//! at the bottom of the region and the brightest at its top (a 128-stud
//! region puts black 64 studs below its centre and white 64 above); one
//! pixel is one 4-stud voxel column when the image matches the region, and
//! the image is stretched over the region when it does not. A colormap
//! picks each column's material by the documented colour key, taking the
//! closest key colour for anything not on it; its Air colour leaves the
//! column empty.

use crate::edit::region::StudBox;
use crate::{Cell, Material, VoxelGrid, VOXEL_STUDS};

/// Studio's own limit on either side of an imported image.
pub const MAX_SIDE: u32 = 4096;

/// A greyscale image, row-major from the top-left, values 0 (low) to 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Heightmap {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

/// An RGB image, row-major from the top-left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colormap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

/// The colormap key, verbatim from `parts/terrain.md`.
pub const COLOR_KEY: [(Material, [u8; 3]); 23] = [
    (Material::Air, [255, 255, 255]),
    (Material::Asphalt, [115, 123, 107]),
    (Material::Basalt, [30, 30, 37]),
    (Material::Brick, [138, 86, 62]),
    (Material::Cobblestone, [132, 123, 90]),
    (Material::Concrete, [127, 102, 63]),
    (Material::CrackedLava, [232, 156, 74]),
    (Material::Glacier, [101, 176, 234]),
    (Material::Grass, [106, 127, 63]),
    (Material::Ground, [102, 92, 59]),
    (Material::Ice, [129, 194, 224]),
    (Material::LeafyGrass, [115, 132, 74]),
    (Material::Limestone, [206, 173, 148]),
    (Material::Mud, [58, 46, 36]),
    (Material::Pavement, [148, 148, 140]),
    (Material::Rock, [102, 108, 111]),
    (Material::Salt, [198, 189, 181]),
    (Material::Sand, [143, 126, 95]),
    (Material::Sandstone, [137, 90, 71]),
    (Material::Slate, [63, 127, 107]),
    (Material::Snow, [195, 199, 218]),
    (Material::WoodPlanks, [139, 109, 79]),
    (Material::Water, [12, 84, 92]),
];

/// The key material whose colour is nearest `rgb`.
pub fn material_for_color(rgb: [u8; 3]) -> Material {
    COLOR_KEY
        .iter()
        .min_by_key(|(_, key)| {
            (0..3)
                .map(|c| (i32::from(rgb[c]) - i32::from(key[c])).pow(2))
                .sum::<i32>()
        })
        .map_or(Material::Air, |(material, _)| *material)
}

impl Heightmap {
    /// Bilinear sample at `u, v` in 0..1 across the image.
    fn sample(&self, u: f32, v: f32) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 0.0;
        }
        let x = (u * self.width as f32 - 0.5).clamp(0.0, (self.width - 1) as f32);
        let y = (v * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: u32, y: u32| self.values[(y * self.width + x) as usize];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        top + (bottom - top) * fy
    }
}

impl Colormap {
    fn sample(&self, u: f32, v: f32) -> [u8; 3] {
        let x = ((u * self.width as f32) as u32).min(self.width.saturating_sub(1));
        let y = ((v * self.height as f32) as u32).min(self.height.saturating_sub(1));
        self.pixels[(y * self.width + x) as usize]
    }
}

/// Builds terrain in `region` (snapped to voxels) from the images. Columns
/// are filled from the region's floor up to the sampled height, the top
/// voxel partly full; existing terrain in the region is replaced.
pub fn import(
    grid: &mut VoxelGrid,
    region: &StudBox,
    heightmap: &Heightmap,
    colormap: Option<&Colormap>,
    default_material: Material,
) {
    let region = region.snapped();
    let (min, max) = region.voxels();
    crate::edit::region::delete(grid, &region);
    let size = region.size();
    for z in min[2]..max[2] {
        for x in min[0]..max[0] {
            // The image's top row is the region's -Z edge: north up in a
            // top-down view, the way Studio lays a heightmap out.
            let u = (x - min[0]) as f32 / (max[0] - min[0]) as f32 + 0.5 / (max[0] - min[0]) as f32;
            let v = (z - min[2]) as f32 / (max[2] - min[2]) as f32 + 0.5 / (max[2] - min[2]) as f32;
            let material = match colormap {
                Some(map) => material_for_color(map.sample(u, v)),
                None => default_material,
            };
            if material == Material::Air {
                continue;
            }
            let top = region.min[1] + heightmap.sample(u, v).clamp(0.0, 1.0) * size[1];
            for y in min[1]..max[1] {
                let bottom = y as f32 * VOXEL_STUDS;
                let fill = ((top - bottom) / VOXEL_STUDS).clamp(0.0, 1.0);
                if fill <= 0.0 {
                    break;
                }
                grid.set([x, y, z], Cell::with_fraction(material, fill));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(width: u32) -> Heightmap {
        Heightmap {
            width,
            height: 1,
            values: (0..width).map(|x| x as f32 / (width - 1) as f32).collect(),
        }
    }

    #[test]
    fn key_colours_map_to_their_materials_and_near_colours_snap() {
        for (material, rgb) in COLOR_KEY {
            assert_eq!(material_for_color(rgb), material);
        }
        assert_eq!(material_for_color([250, 250, 250]), Material::Air);
        assert_eq!(material_for_color([10, 80, 95]), Material::Water);
    }

    #[test]
    fn black_is_the_floor_and_white_the_ceiling() {
        let mut grid = VoxelGrid::new();
        // 4 voxels wide, 128 studs tall, centred on y = 0.
        let region = StudBox::from_center_size([8.0, 0.0, 2.0], [16.0, 128.0, 4.0]);
        import(&mut grid, &region, &ramp(4), None, Material::Grass);
        let column = |x: i32| -> f32 {
            (-16..16)
                .map(|y| grid.get([x, y, 0]).fraction())
                .sum::<f32>()
                * VOXEL_STUDS
        };
        assert!(column(0) < 1.0, "black column is empty: {}", column(0));
        assert!((column(3) - 128.0).abs() < 1.0);
        assert!((column(1) - 128.0 / 3.0).abs() < 1.0, "{}", column(1));
        assert_eq!(grid.get([3, 15, 0]), Cell::full(Material::Grass));
    }

    #[test]
    fn colormap_sets_materials_and_air_skips() {
        let mut grid = VoxelGrid::new();
        let region = StudBox::from_center_size([4.0, 8.0, 2.0], [8.0, 16.0, 4.0]);
        let flat = Heightmap {
            width: 1,
            height: 1,
            values: vec![0.5],
        };
        let colors = Colormap {
            width: 2,
            height: 1,
            pixels: vec![[195, 199, 218], [255, 255, 255]],
        };
        import(&mut grid, &region, &flat, Some(&colors), Material::Grass);
        assert_eq!(grid.get([0, 0, 0]).material, Material::Snow);
        assert!(grid.get([1, 0, 0]).is_air());
    }

    #[test]
    fn a_small_image_stretches_over_a_large_region() {
        let mut grid = VoxelGrid::new();
        let region = StudBox::from_center_size([32.0, 32.0, 2.0], [64.0, 64.0, 4.0]);
        import(&mut grid, &region, &ramp(2), None, Material::Rock);
        let heights: Vec<f32> = (0..16)
            .map(|x| (0..16).map(|y| grid.get([x, y, 0]).fraction()).sum())
            .collect();
        assert!(
            heights.windows(2).all(|w| w[1] >= w[0] - 1e-3),
            "{heights:?}"
        );
        assert!(heights[0] < heights[15]);
    }
}
