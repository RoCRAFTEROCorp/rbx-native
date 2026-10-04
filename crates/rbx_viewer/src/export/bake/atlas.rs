//! Where each chart goes in the baked atlas, how finely, and the maps it
//! samples, filtered down to that fineness.

use std::ops::Range;

use glam::{Vec2, Vec4};

use super::Chart;
use crate::assets::Image;
use crate::pick::Pack;

/// One page's side at most, the size every viewer and GPU takes whole. A
/// part whose charts need more spills onto further pages rather than baking
/// coarser.
pub(super) const PAGE_SIZE: u32 = 2048;
/// Texels of each chart's own surface continued past its edges, so a
/// filtered read, and the first few mip levels a viewer builds from the
/// page, never reach a neighbour.
pub(super) const PADDING: u32 = 4;

pub(super) struct Cell {
    /// The texel the chart's bounding-box corner lands on.
    pub(super) origin: Vec2,
    /// Every texel the chart writes, padding included.
    pub(super) texels: (Range<u32>, Range<u32>),
}

/// One square, power-of-two page and the charts on it, by index.
pub(super) struct Page {
    pub(super) size: u32,
    pub(super) cells: Vec<(usize, Cell)>,
}

/// A chart's side in texels at `density`, padding included.
pub(super) fn texels(chart: &Chart, density: f32) -> (u32, u32) {
    let texels = (chart.extent * density).ceil();
    (texels.x as u32 + 2 * PADDING, texels.y as u32 + 2 * PADDING)
}

/// Every chart laid out at `density` (each fits a page, see
/// `super::split`): one page in the smallest power-of-two square that holds
/// them all when [`PAGE_SIZE`] is enough, else full pages filled in turn by
/// shelves, tallest first, and a last page shrunk to what it holds.
pub(super) fn pages(charts: &[Chart], density: f32) -> Vec<Page> {
    let sizes: Vec<(u32, u32)> = charts.iter().map(|chart| texels(chart, density)).collect();
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
    let mut pages = Vec::new();
    let mut rest: &[usize] = &order;
    while !rest.is_empty() {
        let area: u64 = rest
            .iter()
            .map(|&i| u64::from(sizes[i].0) * u64::from(sizes[i].1))
            .sum();
        let mut size = ((area as f64).sqrt().ceil() as u32)
            .next_power_of_two()
            .clamp(16, PAGE_SIZE);
        // The smallest page that takes everything left, else a full page
        // of as much as fits.
        let (cells, placed) = loop {
            let (cells, placed) = shelves(&sizes, rest, size);
            if placed == rest.len() || size == PAGE_SIZE {
                break (cells, placed);
            }
            size *= 2;
        };
        pages.push(Page { size, cells });
        rest = &rest[placed.max(1)..];
    }
    pages
}

/// Shelves `order` onto a `size` page until one no longer fits: the cells
/// placed, and how many of `order` they are.
fn shelves(sizes: &[(u32, u32)], order: &[usize], size: u32) -> (Vec<(usize, Cell)>, usize) {
    let mut cells = Vec::new();
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for &i in order {
        let (w, h) = sizes[i];
        if x + w > size {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if y + h > size || w > size {
            break;
        }
        cells.push((
            i,
            Cell {
                origin: Vec2::new((x + PADDING) as f32, (y + PADDING) as f32),
                texels: (x..x + w, y..y + h),
            },
        ));
        x += w;
        shelf = shelf.max(h);
    }
    let placed = cells.len();
    (cells, placed)
}

/// The pack's own texels per stud, or the image's across the mesh where
/// that is finer (its area-weighted mean, so one sliver of a triangle with a
/// large UV area cannot blow the bake up): never coarser than either.
pub(super) fn density(charts: &[Chart], pack: &Pack, image: Option<&Image>) -> f32 {
    let pack_density = pack
        .maps
        .iter()
        .flatten()
        .map(|map| map.width as f32 / pack.studs_per_tile.max(0.001))
        .fold(1.0, f32::max);
    let image_density = image.map_or(0.0, |image| {
        let (texels, studs) = charts
            .iter()
            .filter_map(|chart| chart.image_area(image))
            .fold((0.0, 0.0), |(t, s), (texels, studs)| {
                (t + texels, s + studs)
            });
        if studs > 0.0 {
            (texels / studs).sqrt()
        } else {
            0.0
        }
    });
    pack_density.max(image_density)
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
