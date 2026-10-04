//! A material pack baked into a texture of the part's own, for the facets a
//! single UV set cannot carry: a facet tilted off every axis, where
//! `renderer/material.wgsl` blends three box projections of the pack, and a
//! textured mesh, where `filemesh.wgsl` multiplies its own image by the pack.
//!
//! Every triangle gets a chart of its own in one atlas, laid flat at its true
//! shape, and every texel of the chart is shaded the way the shader shades
//! that point of the surface: the same `triplanar_weights`, the same fast
//! path and per-leg cut-off, the same `face_frame`s and `tangent_normal`. The
//! normal map comes out in the chart's own tangent frame, which the baked
//! vertices carry as tangents, so a viewer decodes it back to the normal the
//! viewport shades with. What the bake cannot match is the GPU's filtering:
//! each map is box-filtered once to the bake's density, where the viewport
//! picks a mip level per pixel from how far away the surface is.
//!
//! The bake depends only on the part's shape in its own studs (unit mesh
//! times size), the pack and the image: a rigid move or turn of the part
//! moves the surface and its tangent frame together, leaving every texel as
//! it was, which is what lets one bake serve every part alike.

mod atlas;

use glam::{Vec2, Vec3, Vec4};

use crate::assets::Image;
use crate::pick::Pack;
use crate::scene::linear_to_srgb;
use atlas::{Map, Page};

/// `renderer/material.wgsl`'s blend constants, which this must match.
const TRIPLANAR_SHARPNESS: f32 = 6.0;
const TRIPLANAR_FAST_PATH: f32 = 0.99;
const TRIPLANAR_WEIGHT_EPSILON: f32 = 0.01;
/// `renderer::material::neutral`: what a pack without a map reads.
const NEUTRAL_NORMAL: Vec4 = Vec4::new(128.0 / 255.0, 128.0 / 255.0, 1.0, 1.0);
const NEUTRAL_ROUGHNESS: f32 = 230.0 / 255.0;

/// One triangle corner in the part's own studs, before its rotation and
/// position: what the shader's `object_studs` and `object_normal` are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Corner {
    pub(super) studs: Vec3,
    /// The unscaled mesh normal, which picks the blend weights.
    pub(super) unit_normal: Vec3,
    /// The surface normal in studs (through the scale's inverse transpose),
    /// which the normal map perturbs.
    pub(super) normal: Vec3,
    /// The mesh's own UV, where it has an image.
    pub(super) uv: Vec2,
}

/// Whether the shader shades this triangle from one axis's projection alone,
/// the same axis at every corner: one tiled UV set then reproduces it
/// exactly and nothing needs baking.
pub(super) fn single_axis(triangle: &[Corner; 3]) -> bool {
    let axis = dominant_axis(triangle[0].unit_normal);
    triangle.iter().all(|corner| {
        triplanar_weights(corner.unit_normal).max_element() >= TRIPLANAR_FAST_PATH
            && dominant_axis(corner.unit_normal) == axis
    })
}

/// One page of a bake: its triangles, unshared, in the same studs frame as
/// their corners, and the four maps every one of them reads through `uvs`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Baked {
    pub(super) positions: Vec<Vec3>,
    pub(super) normals: Vec<Vec3>,
    /// The direction of increasing U; the bitangent is `normal × tangent`
    /// (glTF's handedness +1), pointing up the image.
    pub(super) tangents: Vec<Vec3>,
    pub(super) uvs: Vec<[f32; 2]>,
    /// sRGB, the image's alpha in alpha: what the shader multiplies the
    /// part's own colour by.
    pub(super) color: Image,
    pub(super) normal: Image,
    /// Greyscale, as `SurfaceAppearance` authors them.
    pub(super) metalness: Image,
    pub(super) roughness: Image,
}

/// Bakes `triangles` (degenerate ones dropped) with `pack` projected and,
/// where the part has one, `image` multiplied in through each corner's UV,
/// at no less than the pack's own texel density (or the image's, where
/// finer). A surface too large for one page spills onto more, a triangle
/// too large for any page is split until its pieces fit, and the pages are
/// shaded in parallel.
pub(super) fn bake(triangles: &[[Corner; 3]], pack: &Pack, image: Option<&Image>) -> Vec<Baked> {
    let charts: Vec<Chart> = triangles.iter().filter_map(Chart::of).collect();
    if charts.is_empty() {
        return Vec::new();
    }
    let density = atlas::density(&charts, pack, image);
    let charts: Vec<Chart> = charts
        .into_iter()
        .flat_map(|chart| split(chart, density))
        .collect();
    let spt = pack.studs_per_tile.max(0.001);
    let [color, normal, metalness, roughness] = &pack.maps;
    let maps = Maps {
        color: Map::tiled(color.as_deref(), true, density, spt),
        normal: Map::tiled(normal.as_deref(), false, density, spt),
        metalness: Map::tiled(metalness.as_deref(), false, density, spt),
        roughness: Map::tiled(roughness.as_deref(), false, density, spt),
        image: image.map(|image| Map::decode(image, true)),
        studs_per_tile: spt,
    };
    let pages = atlas::pages(&charts, density);
    let shade = |page: &Page| shade_page(page, &charts, &maps, density);
    if pages.len() == 1 {
        return pages.iter().map(shade).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = pages
            .iter()
            .map(|page| scope.spawn(move || shade(page)))
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    })
}

fn shade_page(page: &Page, charts: &[Chart], maps: &Maps, density: f32) -> Baked {
    let mut out = Out::new(page.size);
    let (mut positions, mut normals, mut tangents, mut uvs) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (index, cell) in &page.cells {
        let chart = &charts[*index];
        for (corner, local) in chart.corners.iter().zip(chart.local) {
            let texel = cell.origin + (local - chart.min) * density;
            positions.push(corner.studs);
            normals.push(corner.normal);
            tangents.push(chart.u);
            uvs.push((texel / page.size as f32).to_array());
        }
        for y in cell.texels.1.clone() {
            for x in cell.texels.0.clone() {
                let centre = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let local = chart.min + (centre - cell.origin) / density;
                out.write(x, y, maps.shade(chart, chart.barycentric(local)));
            }
        }
    }
    let [color, normal, metalness, roughness] = out.into_images();
    Baked {
        positions,
        normals,
        tangents,
        uvs,
        color,
        normal,
        metalness,
        roughness,
    }
}

/// `chart`, or its four midpoint subdivisions (recursively) where it would
/// not fit on one page at `density`. The pieces lie on the same plane, so
/// the surface is unchanged; every corner attribute is interpolated
/// linearly, as the GPU interpolates it across the original triangle.
fn split(chart: Chart, density: f32) -> Vec<Chart> {
    let (w, h) = atlas::texels(&chart, density);
    if w <= atlas::PAGE_SIZE && h <= atlas::PAGE_SIZE {
        return vec![chart];
    }
    let [a, b, c] = chart.corners;
    let mid = |p: Corner, q: Corner| Corner {
        studs: (p.studs + q.studs) * 0.5,
        unit_normal: (p.unit_normal + q.unit_normal) * 0.5,
        normal: (p.normal + q.normal) * 0.5,
        uv: (p.uv + q.uv) * 0.5,
    };
    let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
    [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
        .iter()
        .filter_map(Chart::of)
        .flat_map(|piece| split(piece, density))
        .collect()
}

/// One triangle laid flat: `u` across the image, and each corner's place
/// across and down it, in studs.
#[derive(Debug, Clone)]
struct Chart {
    corners: [Corner; 3],
    u: Vec3,
    local: [Vec2; 3],
    /// The corner of the chart's bounding box nearest the image's origin.
    min: Vec2,
    extent: Vec2,
}

impl Chart {
    fn of(triangle: &[Corner; 3]) -> Option<Chart> {
        let [a, b, c] = triangle.map(|corner| corner.studs);
        let normal = (b - a).cross(c - a).try_normalize()?;
        let u = (b - a).try_normalize()?;
        // Up the image, so the bitangent `normal × u` is; image rows run
        // down, hence the minus.
        let up = normal.cross(u);
        let local = [a, b, c].map(|p| Vec2::new((p - a).dot(u), -(p - a).dot(up)));
        let min = local[0].min(local[1]).min(local[2]);
        let max = local[0].max(local[1]).max(local[2]);
        Some(Chart {
            corners: *triangle,
            u,
            local,
            min,
            extent: max - min,
        })
    }

    /// Barycentric weights of `point` against the flattened triangle,
    /// extrapolated past its edges so a chart's padding continues the
    /// surface rather than smearing its border.
    fn barycentric(&self, point: Vec2) -> Vec3 {
        let [a, b, c] = self.local;
        let (ab, ac, ap) = (b - a, c - a, point - a);
        let area = ab.perp_dot(ac);
        let wb = ap.perp_dot(ac) / area;
        let wc = ab.perp_dot(ap) / area;
        Vec3::new(1.0 - wb - wc, wb, wc)
    }

    /// The image's texels and the studs this triangle covers, both as
    /// areas.
    fn image_area(&self, image: &Image) -> Option<(f32, f32)> {
        let [a, b, c] = self
            .corners
            .map(|corner| corner.uv * Vec2::new(image.width as f32, image.height as f32));
        let texels = (b - a).perp_dot(c - a).abs();
        let [la, lb, lc] = self.local;
        let studs = (lb - la).perp_dot(lc - la).abs();
        (studs > 0.0).then_some((texels, studs))
    }
}

struct Maps {
    color: Option<Map>,
    normal: Option<Map>,
    metalness: Option<Map>,
    roughness: Option<Map>,
    image: Option<Map>,
    studs_per_tile: f32,
}

/// One baked texel: linear colour and alpha, the normal in the chart's
/// tangent frame, metalness and roughness.
struct Texel {
    color: Vec3,
    alpha: f32,
    normal: Vec3,
    metalness: f32,
    roughness: f32,
}

impl Maps {
    /// `material_shade_with_normal`'s mapped surface at the point `weights`
    /// names, before any light.
    fn shade(&self, chart: &Chart, weights: Vec3) -> Texel {
        let at = |pick: fn(&Corner) -> Vec3| {
            let [a, b, c] = chart.corners.map(|corner| pick(&corner));
            a * weights.x + b * weights.y + c * weights.z
        };
        let studs = at(|c| c.studs);
        let unit_normal = at(|c| c.unit_normal).normalize_or(Vec3::Y);
        let normal = at(|c| c.normal).normalize_or(Vec3::Y);
        let read = |map: &Option<Map>, uv: Vec2, neutral: Vec4| {
            map.as_ref().map_or(neutral, |m| m.sample(uv))
        };

        let mut texel = Texel {
            color: Vec3::ZERO,
            alpha: 1.0,
            normal: Vec3::ZERO,
            metalness: 0.0,
            roughness: 0.0,
        };
        for (axis, weight) in legs(unit_normal) {
            let (u, v) = face_frame(axis);
            let uv = Vec2::new(studs.dot(u), studs.dot(v)) / self.studs_per_tile;
            texel.color += read(&self.color, uv, Vec4::ONE).truncate() * weight;
            let sample = read(&self.normal, uv, NEUTRAL_NORMAL).truncate();
            texel.normal += tangent_normal(sample, normal, u, v) * weight;
            texel.metalness += read(&self.metalness, uv, Vec4::ZERO).x * weight;
            texel.roughness += read(&self.roughness, uv, Vec4::splat(NEUTRAL_ROUGHNESS)).x * weight;
        }
        if let Some(image) = &self.image {
            let painted = image.sample(at(|c| c.uv.extend(0.0)).truncate());
            texel.color *= painted.truncate();
            texel.alpha = painted.w;
        }
        // Into the chart's frame, built on the interpolated normal as a
        // viewer builds it from the vertices' normals and tangents.
        let shaded = texel.normal.normalize_or(normal);
        let tangent = (chart.u - normal * normal.dot(chart.u)).normalize_or(chart.u);
        let bitangent = normal.cross(tangent);
        texel.normal = Vec3::new(
            shaded.dot(tangent),
            shaded.dot(bitangent),
            shaded.dot(normal),
        );
        texel
    }
}

/// The projections the shader samples at a surface facing `normal`, and
/// how much of each: one on an axis-aligned face (its fast path), else each
/// axis whose weight clears the cut-off, the weights left unrenormalized as
/// there.
fn legs(normal: Vec3) -> Vec<(Vec3, f32)> {
    let weights = triplanar_weights(normal);
    if weights.max_element() >= TRIPLANAR_FAST_PATH {
        return vec![(dominant_axis(normal), 1.0)];
    }
    let sign = |c: f32| if c >= 0.0 { 1.0 } else { -1.0 };
    [
        (Vec3::new(sign(normal.x), 0.0, 0.0), weights.x),
        (Vec3::new(0.0, sign(normal.y), 0.0), weights.y),
        (Vec3::new(0.0, 0.0, sign(normal.z)), weights.z),
    ]
    .into_iter()
    .filter(|&(_, weight)| weight >= TRIPLANAR_WEIGHT_EPSILON)
    .collect()
}

/// `tangent_normal` from `material.wgsl`, in the part's own frame.
fn tangent_normal(sample: Vec3, normal: Vec3, u: Vec3, v: Vec3) -> Vec3 {
    let tangent = (u - normal * normal.dot(u)).normalize_or(u);
    let cross = normal.cross(tangent);
    let bitangent = if cross.dot(-v) >= 0.0 { cross } else { -cross };
    let t = sample * 2.0 - 1.0;
    (tangent * t.x + bitangent * t.y + normal * t.z).normalize_or(normal)
}

fn triplanar_weights(normal: Vec3) -> Vec3 {
    let w = normal.abs().powf(TRIPLANAR_SHARPNESS);
    w / (w.x + w.y + w.z).max(0.0001)
}

/// `dominant_axis` from `lighting.wgsl`: ties go to Y, then X.
pub(super) fn dominant_axis(normal: Vec3) -> Vec3 {
    let sign = |c: f32| if c >= 0.0 { 1.0 } else { -1.0 };
    let a = normal.abs();
    if a.y >= a.x && a.y >= a.z {
        Vec3::new(0.0, sign(normal.y), 0.0)
    } else if a.x >= a.z {
        Vec3::new(sign(normal.x), 0.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, sign(normal.z))
    }
}

/// `face_frame` from `material.wgsl`, for a signed unit axis: image right,
/// then image down.
pub(super) fn face_frame(axis: Vec3) -> (Vec3, Vec3) {
    let sign = |c: f32| if c >= 0.0 { 1.0 } else { -1.0 };
    if axis.y != 0.0 {
        (Vec3::X, Vec3::new(0.0, 0.0, sign(axis.y)))
    } else if axis.x != 0.0 {
        (Vec3::new(0.0, 0.0, -sign(axis.x)), Vec3::NEG_Y)
    } else {
        (Vec3::new(sign(axis.z), 0.0, 0.0), Vec3::NEG_Y)
    }
}

/// The four images being written, RGBA8 each, starting out as the neutral
/// the renderer gives a missing map so the space between charts is inert
/// under a far mip level.
struct Out {
    size: u32,
    images: [Vec<u8>; 4],
}

impl Out {
    fn new(size: u32) -> Out {
        let fill = |texel: [u8; 4]| texel.repeat((size * size) as usize);
        Out {
            size,
            images: [
                fill([255; 4]),
                fill([128, 128, 255, 255]),
                fill([0, 0, 0, 255]),
                fill([230, 230, 230, 255]),
            ],
        }
    }

    fn write(&mut self, x: u32, y: u32, texel: Texel) {
        let at = ((y * self.size + x) * 4) as usize;
        let unit = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let srgb = |value: f32| unit(linear_to_srgb(value.max(0.0)));
        let [color, normal, metalness, roughness] = &mut self.images;
        let c = texel.color;
        color[at..at + 4].copy_from_slice(&[srgb(c.x), srgb(c.y), srgb(c.z), unit(texel.alpha)]);
        let n = texel.normal * 0.5 + 0.5;
        normal[at..at + 4].copy_from_slice(&[unit(n.x), unit(n.y), unit(n.z), u8::MAX]);
        let m = unit(texel.metalness);
        metalness[at..at + 4].copy_from_slice(&[m, m, m, u8::MAX]);
        let r = unit(texel.roughness);
        roughness[at..at + 4].copy_from_slice(&[r, r, r, u8::MAX]);
    }

    fn into_images(self) -> [Image; 4] {
        let size = self.size;
        self.images.map(|pixels| Image {
            width: size,
            height: size,
            pixels,
        })
    }
}

#[cfg(test)]
#[path = "bake/tests.rs"]
mod tests;
