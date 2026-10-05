//! The Terrain Editor's brush: a sphere, box or cylinder in studs, and how
//! much of each voxel it covers.

use crate::VOXEL_STUDS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Sphere,
    Box,
    Cylinder,
}

/// A brush placed in the world. `size` is the base diameter (or width) and
/// `height` the vertical extent for a box or cylinder, both in studs; Studio
/// offers 1 to 64. `axes` are the brush's local X, Y, Z in world space, the
/// identity unless a plane lock tilts it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub shape: Shape,
    pub center: [f32; 3],
    pub size: f32,
    pub height: f32,
    pub axes: [[f32; 3]; 3],
}

pub const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl Brush {
    pub fn new(shape: Shape, center: [f32; 3], size: f32, height: f32) -> Brush {
        Brush {
            shape,
            center,
            size,
            height,
            axes: IDENTITY,
        }
    }

    fn half_extents(&self) -> [f32; 3] {
        let radius = self.size * 0.5;
        match self.shape {
            Shape::Sphere => [radius; 3],
            Shape::Box | Shape::Cylinder => [radius, self.height * 0.5, radius],
        }
    }

    fn local(&self, point: [f32; 3]) -> [f32; 3] {
        let d = sub(point, self.center);
        self.axes.map(|axis| dot(d, axis))
    }

    /// Signed distance from `point` to the brush's surface in studs,
    /// negative inside.
    pub fn distance(&self, point: [f32; 3]) -> f32 {
        let p = self.local(point);
        let half = self.half_extents();
        match self.shape {
            Shape::Sphere => length(p) - half[0],
            Shape::Box => {
                let q = [
                    p[0].abs() - half[0],
                    p[1].abs() - half[1],
                    p[2].abs() - half[2],
                ];
                length(q.map(|v| v.max(0.0))) + q[0].max(q[1]).max(q[2]).min(0.0)
            }
            Shape::Cylinder => {
                let radial = (p[0] * p[0] + p[2] * p[2]).sqrt() - half[0];
                let vertical = p[1].abs() - half[1];
                let outside = (radial.max(0.0).powi(2) + vertical.max(0.0).powi(2)).sqrt();
                outside + radial.max(vertical).min(0.0)
            }
        }
    }

    /// How much of the voxel centred on `point` the brush fills, 0 to 1:
    /// full a voxel inside the surface, empty a voxel outside, and a linear
    /// ramp across the one voxel the surface passes through, which is what
    /// keeps a brushed surface smooth instead of stepped.
    pub fn coverage(&self, point: [f32; 3]) -> f32 {
        (0.5 - self.distance(point) / VOXEL_STUDS).clamp(0.0, 1.0)
    }

    /// 1 at the centre, easing to 0 at the edge: how hard strength-based
    /// tools (Sculpt, Smooth) push each voxel.
    pub fn falloff(&self, point: [f32; 3]) -> f32 {
        let p = self.local(point);
        let half = self.half_extents().map(|h| h.max(VOXEL_STUDS * 0.5));
        let t = match self.shape {
            Shape::Sphere => length(p) / half[0],
            Shape::Box => (p[0].abs() / half[0])
                .max(p[1].abs() / half[1])
                .max(p[2].abs() / half[2]),
            Shape::Cylinder => {
                ((p[0] * p[0] + p[2] * p[2]).sqrt() / half[0]).max(p[1].abs() / half[1])
            }
        };
        let inside = (1.0 - t * t).max(0.0);
        inside * inside
    }

    /// The voxels the brush can touch, `(min, max)` with `max` exclusive,
    /// padded by one voxel for the soft edge.
    pub fn voxel_bounds(&self) -> ([i32; 3], [i32; 3]) {
        let half = self.half_extents();
        // The world-space half extent of the rotated box.
        let reach: [f32; 3] = std::array::from_fn(|world| {
            (0..3)
                .map(|local| self.axes[local][world].abs() * half[local])
                .sum::<f32>()
        });
        let min =
            std::array::from_fn(|a| ((self.center[a] - reach[a]) / VOXEL_STUDS).floor() as i32 - 1);
        let max =
            std::array::from_fn(|a| ((self.center[a] + reach[a]) / VOXEL_STUDS).ceil() as i32 + 1);
        (min, max)
    }
}

/// The centre of voxel `v`, in studs.
pub fn voxel_center(v: [i32; 3]) -> [f32; 3] {
    v.map(|c| (c as f32 + 0.5) * VOXEL_STUDS)
}

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_covers_inside_and_ramps_at_the_edge() {
        let brush = Brush::new(Shape::Sphere, [0.0; 3], 16.0, 16.0);
        assert_eq!(brush.coverage([0.0; 3]), 1.0);
        assert_eq!(brush.coverage([20.0, 0.0, 0.0]), 0.0);
        assert!((brush.coverage([8.0, 0.0, 0.0]) - 0.5).abs() < 1e-6);
        assert!(brush.falloff([0.0; 3]) == 1.0 && brush.falloff([8.0, 0.0, 0.0]) == 0.0);
    }

    #[test]
    fn box_and_cylinder_use_height_separately() {
        let column = Brush::new(Shape::Cylinder, [0.0; 3], 8.0, 32.0);
        assert_eq!(column.coverage([0.0, 14.0, 0.0]), 1.0);
        assert_eq!(column.coverage([6.0, 0.0, 0.0]), 0.0);
        let slab = Brush::new(Shape::Box, [0.0; 3], 32.0, 4.0);
        assert_eq!(slab.coverage([14.0, 0.0, 14.0]), 1.0);
        assert_eq!(slab.coverage([0.0, 6.0, 0.0]), 0.0);
        // The box corner is a corner, not a rounded sphere.
        assert!(slab.distance([16.0, 2.0, 16.0]).abs() < 1e-5);
    }

    #[test]
    fn tilted_axes_turn_the_shape() {
        let mut slab = Brush::new(Shape::Box, [0.0; 3], 32.0, 4.0);
        // Local Y along world X: the slab stands upright.
        slab.axes = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(slab.coverage([0.0, 14.0, 0.0]), 1.0);
        assert_eq!(slab.coverage([6.0, 0.0, 0.0]), 0.0);
        let (min, max) = slab.voxel_bounds();
        assert!(max[1] - min[1] > max[0] - min[0]);
    }

    #[test]
    fn bounds_cover_the_brush() {
        let brush = Brush::new(Shape::Sphere, [2.0, 2.0, 2.0], 4.0, 4.0);
        let (min, max) = brush.voxel_bounds();
        assert!(min.iter().all(|&v| v <= -1) && max.iter().all(|&v| v >= 2));
        assert_eq!(voxel_center([0, -1, 2]), [2.0, -2.0, 10.0]);
    }
}
