//! Rays against the voxel grid: where the Terrain Editor's brush lands
//! under the cursor.

use crate::{Material, VoxelGrid, VOXEL_STUDS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Studs along the (normalized) ray.
    pub distance: f32,
    pub position: [f32; 3],
    /// The surface normal, from the occupancy gradient around the hit so it
    /// follows the smoothed surface rather than voxel faces.
    pub normal: [f32; 3],
    pub voxel: [i32; 3],
}

/// The first voxel at least half full along the ray, stepping voxel by
/// voxel (Amanatides–Woo). Water counts unless `ignore_water` is set.
pub fn raycast(
    grid: &VoxelGrid,
    origin: [f32; 3],
    direction: [f32; 3],
    max_distance: f32,
    ignore_water: bool,
) -> Option<Hit> {
    let length = (direction[0].powi(2) + direction[1].powi(2) + direction[2].powi(2)).sqrt();
    if grid.is_empty() || length < f32::EPSILON {
        return None;
    }
    let dir = direction.map(|d| d / length);
    // Start where the ray enters the grid's bounds, so a camera far away
    // does not walk thousands of empty voxels.
    let (min, max) = grid.chunk_bounds()?;
    let (enter, exit) = slab(
        origin,
        dir,
        min.map(|v| v as f32 * VOXEL_STUDS),
        max.map(|v| v as f32 * VOXEL_STUDS),
    )?;
    let start = enter.max(0.0);
    let stop = exit.min(max_distance);
    if start > stop {
        return None;
    }
    let p = std::array::from_fn::<f32, 3, _>(|a| (origin[a] + dir[a] * start) / VOXEL_STUDS);
    let mut voxel = p.map(|v| v.floor() as i32);
    let step = dir.map(|d| if d > 0.0 { 1 } else { -1 });
    let delta = dir.map(|d| {
        if d == 0.0 {
            f32::INFINITY
        } else {
            VOXEL_STUDS / d.abs()
        }
    });
    let mut next: [f32; 3] = std::array::from_fn(|a| {
        if dir[a] == 0.0 {
            return f32::INFINITY;
        }
        let edge = if dir[a] > 0.0 {
            voxel[a] as f32 + 1.0
        } else {
            voxel[a] as f32
        };
        start + (edge - p[a]) * VOXEL_STUDS / dir[a]
    });
    let mut t = start;
    // A ray that starts inside terrain or water (a camera under the sea)
    // looks out of it, not at it: hits only count once it has crossed an
    // empty voxel.
    let mut outside = start > 0.0;
    while t <= stop {
        let cell = grid.get(voxel);
        let fill = if cell.material == Material::Water {
            if ignore_water {
                0.0
            } else {
                cell.fraction()
            }
        } else if ignore_water {
            cell.solid_fraction()
        } else {
            cell.solid_fraction() + cell.water_fraction()
        };
        if fill < 0.5 {
            outside = true;
        } else if outside {
            let position = std::array::from_fn(|a| origin[a] + dir[a] * t);
            return Some(Hit {
                distance: t,
                position,
                normal: gradient_normal(grid, voxel, dir),
                voxel,
            });
        }
        let axis = if next[0] < next[1] {
            if next[0] < next[2] {
                0
            } else {
                2
            }
        } else if next[1] < next[2] {
            1
        } else {
            2
        };
        t = next[axis];
        voxel[axis] += step[axis];
        next[axis] += delta[axis];
    }
    None
}

fn slab(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<(f32, f32)> {
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for a in 0..3 {
        if dir[a] == 0.0 {
            if origin[a] < min[a] || origin[a] > max[a] {
                return None;
            }
            continue;
        }
        let t0 = (min[a] - origin[a]) / dir[a];
        let t1 = (max[a] - origin[a]) / dir[a];
        enter = enter.max(t0.min(t1));
        exit = exit.min(t0.max(t1));
    }
    (enter <= exit && exit >= 0.0).then_some((enter, exit))
}

/// Points from full toward empty, so it faces out of the terrain. Falls back
/// to facing the ray when the neighbourhood is uniform.
fn gradient_normal(grid: &VoxelGrid, v: [i32; 3], dir: [f32; 3]) -> [f32; 3] {
    let fill = |dx: i32, dy: i32, dz: i32| {
        let cell = grid.get([v[0] + dx, v[1] + dy, v[2] + dz]);
        cell.solid_fraction() + cell.water_fraction()
    };
    let g = [
        fill(-1, 0, 0) - fill(1, 0, 0),
        fill(0, -1, 0) - fill(0, 1, 0),
        fill(0, 0, -1) - fill(0, 0, 1),
    ];
    let len = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
    if len < 1e-4 {
        return dir.map(|d| -d);
    }
    g.map(|c| c / len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cell;

    fn floor() -> VoxelGrid {
        let mut grid = VoxelGrid::new();
        for x in -4..4 {
            for z in -4..4 {
                grid.set([x, -1, z], Cell::full(Material::Grass));
            }
        }
        grid
    }

    #[test]
    fn a_ray_down_hits_the_floor_top_facing_up() {
        let hit = raycast(&floor(), [1.0, 50.0, 1.0], [0.0, -1.0, 0.0], 1000.0, false).unwrap();
        assert!((hit.position[1] - 0.0).abs() < 1e-4);
        assert!((hit.distance - 50.0).abs() < 1e-4);
        assert_eq!(hit.voxel, [0, -1, 0]);
        assert!(hit.normal[1] > 0.9);
    }

    #[test]
    fn slanted_rays_and_misses() {
        let grid = floor();
        let hit = raycast(&grid, [-10.0, 10.0, 2.0], [1.0, -1.0, 0.0], 1000.0, false).unwrap();
        assert_eq!(hit.voxel, [0, -1, 0]);
        assert!(raycast(&grid, [0.0, 10.0, 0.0], [0.0, 1.0, 0.0], 1000.0, false).is_none());
        assert!(raycast(&grid, [0.0, 50.0, 0.0], [0.0, -1.0, 0.0], 10.0, false).is_none());
        assert!(raycast(&VoxelGrid::new(), [0.0; 3], [0.0, -1.0, 0.0], 10.0, false).is_none());
    }

    #[test]
    fn a_ray_starting_inside_looks_out_to_the_next_surface() {
        let mut grid = floor();
        for y in 0..4 {
            grid.set([0, y, 0], Cell::full(Material::Water));
        }
        // From inside the water column, looking down: through the water,
        // out of it is never reached, so the floor below is not hit either
        // until air was crossed — here it never is.
        assert!(raycast(&grid, [2.0, 10.0, 2.0], [0.0, -1.0, 0.0], 1000.0, false).is_none());
        // From inside, looking sideways out into air and onto a wall.
        grid.set([3, 2, 0], Cell::full(Material::Rock));
        let hit = raycast(&grid, [2.0, 10.0, 2.0], [1.0, 0.0, 0.0], 1000.0, false).unwrap();
        assert_eq!(hit.voxel, [3, 2, 0]);
    }

    #[test]
    fn water_stops_the_ray_unless_ignored() {
        let mut grid = floor();
        grid.set([0, 2, 0], Cell::full(Material::Water));
        let hit = raycast(&grid, [2.0, 50.0, 2.0], [0.0, -1.0, 0.0], 1000.0, false).unwrap();
        assert_eq!(hit.voxel, [0, 2, 0]);
        let through = raycast(&grid, [2.0, 50.0, 2.0], [0.0, -1.0, 0.0], 1000.0, true).unwrap();
        assert_eq!(through.voxel, [0, -1, 0]);
    }
}
