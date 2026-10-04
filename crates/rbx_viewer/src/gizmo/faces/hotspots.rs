//! The Edit Pivot tool's snap hotspots: the points on the selection's box a
//! dragged pivot jumps onto.
//!
//! `creator-docs` (`studio/pivot-tools.md`, "Snap"): the pivot "snaps to
//! **hotspots** such as corners, edges, or centers … hotspots display as
//! small magenta points". It names no exact set, so this is this editor's
//! reading of it: every corner of the box Scale's balls stand on, the middle
//! of every edge, the middle of every face, and the box's own centre — the
//! 3×3×3 lattice of points at `-1`, `0` and `+1` half-sizes along each of
//! the box's axes.

use glam::Vec3;

use crate::pick::Ray;

use super::{arm_length, Faces};

/// How many hotspots one box has: three stops along each of three axes.
pub const HOTSPOTS: usize = 27;
/// A hotspot's drawn radius, in arm lengths — a dot, well under a Scale
/// ball's, so the lattice never reads as handles to grab.
const HOTSPOT_RADIUS: f32 = 0.035;
/// How far off a hotspot a ray still lands on it, in arm lengths: the same
/// reach a Move arm's own pick has, so a pivot dragged anywhere near a
/// corner on screen takes it.
const HOTSPOT_PICK: f32 = 0.15;

impl Faces {
    /// Every hotspot on this box, corners, edge middles, face middles and the
    /// centre alike.
    pub fn hotspots(&self) -> [Vec3; HOTSPOTS] {
        std::array::from_fn(|index| {
            let stops = [index % 3, index / 3 % 3, index / 9].map(|stop| stop as f32 - 1.0);
            self.centre()
                + (0..3)
                    .map(|axis| self.basis[axis] * self.half[axis] * stops[axis])
                    .sum::<Vec3>()
        })
    }

    /// The hotspot `ray` passes nearest, if it passes within reach of any —
    /// measured on screen, so a far corner is as easy to land on as a near
    /// one. Among several in reach, the one nearest the ray's own line wins.
    pub fn nearest_hotspot(&self, ray: Ray) -> Option<Vec3> {
        self.hotspots()
            .into_iter()
            .filter_map(|point| {
                let (distance, offset) = ray.nearest(point);
                let reach = HOTSPOT_PICK * arm_length(point, self.pose, self.orthographic);
                (distance > 0.0 && offset <= reach).then_some((point, offset / reach))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(point, _)| point)
    }

    /// How big a hotspot at `point` is drawn, in studs.
    pub fn hotspot_radius(&self, point: Vec3) -> f32 {
        HOTSPOT_RADIUS * arm_length(point, self.pose, self.orthographic)
    }
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat, Vec3};

    use super::*;
    use crate::Pose;

    /// A box of `size` at `centre`, turned by `turn`, seen from ten studs
    /// down +Z of the world origin.
    fn faces(centre: Vec3, size: Vec3, turn: Quat) -> Faces {
        let pose = Pose {
            position: Vec3::new(0.0, 0.0, 10.0),
            yaw: 0.0,
            pitch: 0.0,
            fov_degrees: 70.0,
            ortho_scale: 25.0,
        };
        Faces::new(
            Mat4::from_scale_rotation_translation(size, turn, centre),
            pose,
            false,
        )
    }

    fn has(points: &[Vec3], wanted: Vec3) -> bool {
        points.iter().any(|point| (*point - wanted).length() < 1e-4)
    }

    #[test]
    fn a_box_has_its_corners_edge_middles_face_middles_and_centre() {
        let points = faces(Vec3::ZERO, Vec3::new(4.0, 2.0, 6.0), Quat::IDENTITY).hotspots();

        assert!(has(&points, Vec3::ZERO), "centre");
        assert!(has(&points, Vec3::new(2.0, 0.0, 0.0)), "a face's middle");
        assert!(has(&points, Vec3::new(2.0, 1.0, 0.0)), "an edge's middle");
        assert!(has(&points, Vec3::new(-2.0, -1.0, 3.0)), "a corner");
        // No two the same, and none outside the box.
        for (index, point) in points.iter().enumerate() {
            assert!(!has(&points[index + 1..], *point), "{point} twice");
            assert!(point.abs().cmple(Vec3::new(2.0, 1.0, 3.0) + 1e-4).all());
        }
    }

    #[test]
    fn the_hotspots_turn_with_the_box() {
        let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let points = faces(Vec3::new(5.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 2.0), turn).hotspots();

        // The box's own +X now runs along the world's -Z.
        assert!(has(&points, Vec3::new(5.0, 0.0, -2.0)));
        assert!(!has(&points, Vec3::new(7.0, 0.0, 0.0)));
    }

    #[test]
    fn a_ray_near_a_corner_lands_on_it_and_one_between_hotspots_on_none() {
        let faces = faces(Vec3::ZERO, Vec3::splat(4.0), Quat::IDENTITY);
        let eye = Vec3::new(0.0, 0.0, 10.0);
        let toward = |point: Vec3| Ray {
            origin: eye,
            direction: (point - eye).normalize(),
        };

        // Aimed a hair off the near top-right corner.
        let near_corner = toward(Vec3::new(2.05, 1.95, 2.0));
        assert_eq!(faces.nearest_hotspot(near_corner), Some(Vec3::splat(2.0)));
        // Halfway between a face's middle and its edge's, a stud from each.
        let between = toward(Vec3::new(1.0, 0.0, 2.0));
        assert_eq!(faces.nearest_hotspot(between), None);
        // Pointing away from the box altogether.
        let behind = Ray {
            origin: eye,
            direction: Vec3::Z,
        };
        assert_eq!(faces.nearest_hotspot(behind), None);
    }
}
