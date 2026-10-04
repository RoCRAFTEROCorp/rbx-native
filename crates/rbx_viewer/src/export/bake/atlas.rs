//! Where each chart goes in the baked atlas, how finely, and the maps it
//! samples, filtered down to that fineness.

use std::ops::Range;

use glam::{Vec2, Vec4};

use super::Chart;
use crate::assets::Image;
use crate::pick::Pack;

/// Texels of the surface baked per stud at most: past this a pack's tiles
/// are sharper than any export needs and the atlas only grows.
const MAX_DENSITY: f32 = 64.0;
/// The atlas's side at most, the size most viewers and GPUs take whole.
// ponytail: one atlas per part, capped; a huge tilted part bakes coarser
// rather than spilling into a second page.
pub(super) const MAX_SIZE: u32 = 2048;
/// Texels of each chart's own surface continued past its edges, so a
/// filtered or mipmapped read near a seam never reaches a neighbour.
const PADDING: u32 = 2;

pub(super) struct Cell {
    /// The texel the chart's bounding-box corner lands on.
    pub(super) origin: Vec2,
    /// Every texel the chart writes, padding included.
    pub(super) texels: (Range<u32>, Range<u32>),
}

pub(super) struct Atlas {
    pub(super) size: u32,
    pub(super) density: f32,
    pub(super) cells: Vec<Cell>,
}

impl Atlas {
    /// Lays every chart out at the finest density both the pack and the
    /// image deserve, coarsening until they fit in [`MAX_SIZE`].
    pub(super) fn build(charts: &[Chart], pack: &Pack, image: Option<&Image>) -> Atlas {
        let mut density = wanted_density(charts, pack, image);
        loop {
            if let Some(atlas) = Atlas::pack(charts, density) {
                return atlas;
            }
            density *= 0.8;
        }
    }

    /// Shelf packing, tallest first, in the smallest power-of-two square
    /// that holds everything; `None` past [`MAX_SIZE`].
    fn pack(charts: &[Chart], density: f32) -> Option<Atlas> {
        let sizes: Vec<(u32, u32)> = charts
            .iter()
            .map(|chart| {
                let texels = (chart.extent * density).ceil();
                (texels.x as u32 + 2 * PADDING, texels.y as u32 + 2 * PADDING)
            })
            .collect();
        let area: u64 = sizes
            .iter()
            .map(|&(w, h)| u64::from(w) * u64::from(h))
            .sum();
        let mut size = ((area as f64).sqrt().ceil() as u32)
            .next_power_of_two()
            .max(16);
        let mut order: Vec<usize> = (0..sizes.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
        while size <= MAX_SIZE {
            if let Some(cells) = shelves(&sizes, &order, size) {
                return Some(Atlas {
                    size,
                    density,
                    cells,
                });
            }
            size *= 2;
        }
        None
    }
}

fn shelves(sizes: &[(u32, u32)], order: &[usize], size: u32) -> Option<Vec<Cell>> {
    let mut cells: Vec<Option<Cell>> = (0..sizes.len()).map(|_| None).collect();
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for &i in order {
        let (w, h) = sizes[i];
        if w > size {
            return None;
        }
        if x + w > size {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if y + h > size {
            return None;
        }
        cells[i] = Some(Cell {
            origin: Vec2::new((x + PADDING) as f32, (y + PADDING) as f32),
            texels: (x..x + w, y..y + h),
        });
        x += w;
        shelf = shelf.max(h);
    }
    cells.into_iter().collect()
}

/// The pack's own texels per stud, or the image's across the mesh where
/// that is finer, capped at [`MAX_DENSITY`].
fn wanted_density(charts: &[Chart], pack: &Pack, image: Option<&Image>) -> f32 {
    let pack_density = pack
        .maps
        .iter()
        .flatten()
        .map(|map| map.width as f32 / pack.studs_per_tile.max(0.001))
        .fold(1.0, f32::max);
    let image_density = image.map_or(0.0, |image| {
        charts
            .iter()
            .filter_map(|chart| chart.image_density(image))
            .fold(0.0, f32::max)
    });
    pack_density.max(image_density).min(MAX_DENSITY)
}

/// One map as the bake reads it: filtered down to the bake's density, as a
/// mipmapped sampler would, and read bilinearly with the image repeating.
pub(super) struct Map {
    width: u32,
    height: u32,
    /// Linear where the map is colour, as the GPU decodes an sRGB texture.
    texels: Vec<Vec4>,
}

impl Map {
    /// `map` filtered for a bake `density` texels per stud when it tiles
    /// every `studs_per_tile`; `None` without a map.
    pub(super) fn tiled(
        map: Option<&Image>,
        srgb: bool,
        density: f32,
        studs_per_tile: f32,
    ) -> Option<Map> {
        let map = map?;
        let own = map.width as f32 / studs_per_tile.max(0.001);
        let level = (own / density).log2().floor().max(0.0) as u32;
        Some(Map::decode(map, srgb).halved(level))
    }

    pub(super) fn decode(image: &Image, srgb: bool) -> Map {
        let channel = |value: u8, colour: bool| {
            let unit = f32::from(value) / 255.0;
            if colour {
                crate::scene::srgb_to_linear(unit)
            } else {
                unit
            }
        };
        let texels = image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                Vec4::new(
                    channel(p[0], srgb),
                    channel(p[1], srgb),
                    channel(p[2], srgb),
                    channel(p[3], false),
                )
            })
            .collect();
        Map {
            width: image.width.max(1),
            height: image.height.max(1),
            texels,
        }
    }

    /// A box filter `levels` times over, never below one texel.
    fn halved(mut self, levels: u32) -> Map {
        for _ in 0..levels {
            if self.width == 1 && self.height == 1 {
                break;
            }
            let (width, height) = ((self.width / 2).max(1), (self.height / 2).max(1));
            let texels = (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let at = |dx: u32, dy: u32| {
                        let sx = (x * 2 + dx).min(self.width - 1);
                        let sy = (y * 2 + dy).min(self.height - 1);
                        self.texels[(sy * self.width + sx) as usize]
                    };
                    (at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1)) * 0.25
                })
                .collect();
            self = Map {
                width,
                height,
                texels,
            };
        }
        self
    }

    /// Bilinear, repeating, at `uv` in image widths (top-left origin).
    pub(super) fn sample(&self, uv: Vec2) -> Vec4 {
        let x = uv.x * self.width as f32 - 0.5;
        let y = uv.y * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |dx: f32, dy: f32| {
            let sx = (x0 + dx).rem_euclid(self.width as f32) as u32 % self.width;
            let sy = (y0 + dy).rem_euclid(self.height as f32) as u32 % self.height;
            self.texels[(sy * self.width + sx) as usize]
        };
        let top = at(0.0, 0.0).lerp(at(1.0, 0.0), fx);
        let bottom = at(0.0, 1.0).lerp(at(1.0, 1.0), fx);
        top.lerp(bottom, fy)
    }
}
