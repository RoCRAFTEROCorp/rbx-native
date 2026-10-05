//! Per-vertex material weights, which is what lets the renderer blend one
//! material into the next instead of drawing a hard border on the voxel grid.
//!
//! A surface vertex stands for one cell of 8 voxels, and its share of a
//! material is how much of that cell's solid fill is that material. A
//! triangle can only interpolate weights for materials its three corners
//! name in the same order, so the list is chosen per quad — the heaviest
//! [`BLEND`] over its four corners — and a vertex is shared only between
//! quads that chose the same list.

use crate::{Cell, Material};

/// Materials one vertex blends. Three covers any border where three
/// materials meet; a fourth in the same quad is dropped and the rest
/// renormalized.
pub const BLEND: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    /// Ascending slot order, then `Air` with weight 0 for any unused entry.
    pub materials: [Material; BLEND],
    /// Non-negative, summing to 1.
    pub weights: [f32; BLEND],
}

/// One cell's solid fill per material slot, normalized to sum to 1.
pub(super) type Shares = [f32; Material::ALL.len()];

pub(super) fn shares(corners: impl Iterator<Item = Cell>) -> Shares {
    let mut shares = [0.0f32; Material::ALL.len()];
    for cell in corners {
        shares[usize::from(cell.material.slot())] += cell.solid_fraction();
    }
    // The square root lifts a minority: a lone voxel of rock in grass is a
    // quarter of each of its corner cells, and at a quarter it would never
    // win against grass anywhere, vanishing instead of showing as a patch.
    // Still monotone, so a border's even split stays even.
    shares.iter_mut().for_each(|s| *s = s.sqrt());
    let total: f32 = shares.iter().sum();
    if total > 0.0 {
        shares.iter_mut().for_each(|s| *s /= total);
    }
    shares
}

/// The materials a quad's four corners blend between.
pub(super) fn pick(corners: [&Shares; 4]) -> [Material; BLEND] {
    let mut sum = [0.0f32; Material::ALL.len()];
    for shares in corners {
        sum.iter_mut().zip(shares).for_each(|(s, v)| *s += v);
    }
    // Heaviest first, ties to the lower slot, so both sides of a chunk seam
    // pick the same list for the same cells.
    let mut order: Vec<usize> = (0..sum.len()).filter(|&i| sum[i] > 0.0).collect();
    order.sort_by(|&a, &b| sum[b].total_cmp(&sum[a]).then(a.cmp(&b)));
    order.truncate(BLEND);
    order.sort_unstable();
    let mut materials = [Material::Air; BLEND];
    for (m, slot) in materials.iter_mut().zip(order) {
        *m = Material::ALL[slot];
    }
    materials
}

/// One vertex's weights for the quad's chosen `materials`.
pub(super) fn blend(shares: &Shares, materials: [Material; BLEND]) -> Blend {
    let mut weights = [0.0; BLEND];
    for (w, m) in weights.iter_mut().zip(materials) {
        if m != Material::Air {
            *w = shares[usize::from(m.slot())];
        }
    }
    let total: f32 = weights.iter().sum();
    if total > 0.0 {
        weights.iter_mut().for_each(|w| *w /= total);
    } else {
        // Every material this vertex holds lost out to heavier ones in the
        // quad (four or more meeting): one of the quad's stands in.
        weights[0] = 1.0;
    }
    Blend { materials, weights }
}
