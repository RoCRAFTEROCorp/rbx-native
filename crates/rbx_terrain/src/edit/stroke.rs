//! The brush tools: Draw, Sculpt, Smooth, Flatten and Paint. Each call is
//! one application of the brush; a drag applies it again at every step.
//!
//! Roblox does not document its brush arithmetic, so these follow the
//! documented behaviour of each tool rather than any copied algorithm: Draw
//! sets voxels to the brush's shape, Sculpt nudges them by a strength,
//! Smooth pulls each voxel toward its neighbours' average, Flatten toward a
//! plane, and Paint changes material without touching shape.

use super::brush::{voxel_center, Brush};
use crate::{Cell, Material, VoxelGrid, VOXEL_STUDS};

/// How the brush treats water. With `ignore_water` set, water voxels are
/// left as they are (and do not stop the cursor); otherwise subtracting or
/// eroding removes water too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaterRule {
    pub ignore_water: bool,
}

/// How far one Sculpt application moves a voxel at strength 1 and the
/// brush's centre: a quarter of a voxel, so a held drag builds up over a
/// few frames rather than snapping like Draw.
const SCULPT_RATE: f32 = 0.25;

fn each_voxel(brush: &Brush, mut visit: impl FnMut([i32; 3], [f32; 3])) {
    let (min, max) = brush.voxel_bounds();
    for y in min[1]..max[1] {
        for z in min[2]..max[2] {
            for x in min[0]..max[0] {
                let voxel = [x, y, z];
                visit(voxel, voxel_center(voxel));
            }
        }
    }
}

/// Draw in Add mode: every voxel becomes at least as full as the brush
/// covers it. Empty (or water) voxels take `material`; solid ones keep
/// theirs.
pub fn draw_add(grid: &mut VoxelGrid, brush: &Brush, material: Material) {
    each_voxel(brush, |voxel, center| {
        let coverage = brush.coverage(center);
        let cell = grid.get(voxel);
        if coverage > cell.solid_fraction() {
            let keep = if cell.material.is_solid() {
                cell.material
            } else {
                material
            };
            grid.set(voxel, Cell::with_fraction(keep, coverage));
        }
    });
}

/// Draw in Subtract mode: every voxel becomes at most as full as the brush
/// leaves it.
pub fn draw_subtract(grid: &mut VoxelGrid, brush: &Brush, water: WaterRule) {
    each_voxel(brush, |voxel, center| {
        let left = 1.0 - brush.coverage(center);
        let cell = grid.get(voxel);
        if cell.material == Material::Water && water.ignore_water {
            return;
        }
        if cell.fraction() > left {
            grid.set(voxel, Cell::with_fraction(cell.material, left));
        }
    });
}

/// Sculpt: grow (or shrink) terrain by `strength` (0.1 to 1), strongest at
/// the brush's centre. Growing only adds to voxels already touching solid
/// terrain, so it builds up from the surface instead of filling the air
/// inside the brush the way Draw does.
pub fn sculpt(
    grid: &mut VoxelGrid,
    brush: &Brush,
    add: bool,
    strength: f32,
    material: Material,
    water: WaterRule,
) {
    let before = grid.clone();
    each_voxel(brush, |voxel, center| {
        let push = strength * SCULPT_RATE * brush.falloff(center);
        if push <= 0.0 {
            return;
        }
        let cell = before.get(voxel);
        if add {
            let solid = cell.solid_fraction();
            if solid >= 1.0 || !touches_solid(&before, voxel) {
                return;
            }
            let keep = if cell.material.is_solid() {
                cell.material
            } else {
                dominant_neighbour(&before, voxel).unwrap_or(material)
            };
            grid.set(voxel, Cell::with_fraction(keep, (solid + push).min(1.0)));
        } else {
            if cell.material == Material::Water && water.ignore_water {
                return;
            }
            let left = cell.fraction() - push;
            grid.set(voxel, Cell::with_fraction(cell.material, left));
        }
    });
}

/// Smooth: pull each voxel's solid fill toward the average of its 3×3×3
/// neighbourhood by `strength`, so ridges shrink and pits fill.
pub fn smooth(grid: &mut VoxelGrid, brush: &Brush, strength: f32, water: WaterRule) {
    let before = grid.clone();
    each_voxel(brush, |voxel, center| {
        let pull = strength * brush.falloff(center);
        if pull <= 0.0 {
            return;
        }
        let cell = before.get(voxel);
        if cell.material == Material::Water && water.ignore_water {
            return;
        }
        let mut sum = 0.0;
        for offset in NEIGHBOURHOOD {
            sum += before.get(add(voxel, offset)).solid_fraction();
        }
        let average = sum / NEIGHBOURHOOD.len() as f32;
        let solid = cell.solid_fraction();
        let target = solid + (average - solid) * pull;
        if (target - solid).abs() < 0.5 / 256.0 {
            return;
        }
        let material = if cell.material.is_solid() {
            cell.material
        } else {
            match dominant_neighbour(&before, voxel) {
                Some(material) => material,
                None => return,
            }
        };
        grid.set(voxel, Cell::with_fraction(material, target));
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlattenMode {
    /// Lower terrain above the plane only.
    Erode,
    /// Raise terrain below the plane only.
    Grow,
    /// Both: Studio's default.
    #[default]
    Both,
}

/// Flatten toward the horizontal plane at `plane_y` studs. Each voxel's
/// target fill is the part of it below the plane; `strength` sets how far
/// one application moves toward it.
pub fn flatten(
    grid: &mut VoxelGrid,
    brush: &Brush,
    plane_y: f32,
    mode: FlattenMode,
    strength: f32,
    material: Material,
    water: WaterRule,
) {
    each_voxel(brush, |voxel, center| {
        let reach = brush.coverage(center) * strength;
        if reach <= 0.0 {
            return;
        }
        let cell = grid.get(voxel);
        if cell.material == Material::Water && water.ignore_water {
            return;
        }
        let bottom = voxel[1] as f32 * VOXEL_STUDS;
        let target = ((plane_y - bottom) / VOXEL_STUDS).clamp(0.0, 1.0);
        let solid = cell.solid_fraction();
        let lower = solid > target && mode != FlattenMode::Grow;
        let raise = solid < target && mode != FlattenMode::Erode;
        if !(lower || raise) {
            return;
        }
        let next = solid + (target - solid) * reach;
        let fill = if cell.material.is_solid() {
            cell.material
        } else {
            let below = grid.get(add(voxel, [0, -1, 0]));
            if below.material.is_solid() {
                below.material
            } else {
                material
            }
        };
        grid.set(voxel, Cell::with_fraction(fill, next));
    });
}

/// Paint: change the material of solid voxels the brush covers by at least
/// half, keeping their shape. With `replace` set, only voxels of that
/// material change (the tool's Replace mode).
pub fn paint(grid: &mut VoxelGrid, brush: &Brush, material: Material, replace: Option<Material>) {
    if !material.is_solid() {
        return;
    }
    each_voxel(brush, |voxel, center| {
        if brush.coverage(center) < 0.5 {
            return;
        }
        let cell = grid.get(voxel);
        if !cell.material.is_solid() || replace.is_some_and(|from| from != cell.material) {
            return;
        }
        grid.set(voxel, Cell::new(material, cell.occupancy, cell.liquid));
    });
}

/// The solid material nearest the brush's centre, for Auto Material: new
/// terrain matches what it is drawn onto.
pub fn auto_material(grid: &VoxelGrid, brush: &Brush) -> Option<Material> {
    let mut best: Option<(f32, Material)> = None;
    each_voxel(brush, |voxel, center| {
        let cell = grid.get(voxel);
        if !cell.material.is_solid() {
            return;
        }
        let distance = super::brush::length(super::brush::sub(center, brush.center));
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, cell.material));
        }
    });
    best.map(|(_, material)| material)
}

const NEIGHBOURHOOD: [[i32; 3]; 27] = {
    let mut all = [[0; 3]; 27];
    let mut i = 0;
    while i < 27 {
        all[i] = [
            (i % 3) as i32 - 1,
            (i / 3 % 3) as i32 - 1,
            (i / 9) as i32 - 1,
        ];
        i += 1;
    }
    all
};

const FACES: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn touches_solid(grid: &VoxelGrid, voxel: [i32; 3]) -> bool {
    grid.get(voxel).solid_fraction() > 0.0
        || FACES
            .iter()
            .any(|f| grid.get(add(voxel, *f)).solid_fraction() >= 0.5)
}

/// The solid material most voxels around `voxel` share, weighted by fill.
fn dominant_neighbour(grid: &VoxelGrid, voxel: [i32; 3]) -> Option<Material> {
    let mut weights = [0.0f32; 23];
    for offset in NEIGHBOURHOOD {
        let cell = grid.get(add(voxel, offset));
        weights[usize::from(cell.material.slot())] += cell.solid_fraction();
    }
    let (slot, weight) = weights
        .iter()
        .enumerate()
        .skip(2)
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    (*weight > 0.0)
        .then(|| Material::from_slot(slot as u8))
        .flatten()
}

#[cfg(test)]
mod tests;
