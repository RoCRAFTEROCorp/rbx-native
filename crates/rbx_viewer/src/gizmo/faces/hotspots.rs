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
//!
//! A model's parts get hotspots of their own as well ([`Hotspots`]). The
//! prose says nothing about them; its "Hotspots on a model" screenshot is
//! the only source, and shows the model box's lattice plus, on each part,
//! the middle of every face and its centre — no part corners or edge
//! middles — all drawn at once, with no cursor in the shot. So that is what
//! is drawn here: always, not only near the cursor.

use glam::Vec3;

use crate::pick::Ray;

use super::{arm_length, Faces};

/// How many hotspots one box has: three stops along each of three axes.
pub const HOTSPOTS: usize = 27;
/// How many each of a model's parts adds: its six face middles and its
/// centre.
pub const PART_HOTSPOTS: usize = 7;
/// ponytail: at most this many parts get hotspots of their own — the first
/// in selection order — so a model of hundreds of parts costs a fixed
/// buffer and a fixed mesh per frame (each dot is a full ball). Pick the
/// parts nearest the cursor instead if the cap ever bites in practice.
pub const HOTSPOT_PARTS: usize = 32;
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

    /// A part's own hotspots inside a model: its centre and the middle of
    /// each of its faces.
    pub fn part_hotspots(&self) -> [Vec3; PART_HOTSPOTS] {
        std::array::from_fn(|index| match index {
            0 => self.centre(),
            face => {
                let (axis, sign) = ((face - 1) / 2, if face % 2 == 0 { 1.0 } else { -1.0 });
                self.centre() + self.basis[axis] * self.half[axis] * sign
            }
        })
    }

    /// How big a hotspot at `point` is drawn, in studs.
    pub fn hotspot_radius(&self, point: Vec3) -> f32 {
        HOTSPOT_RADIUS * arm_length(point, self.pose, self.orthographic)
    }
}

/// Every hotspot Edit Pivot shows: the selection's box's whole lattice, and
/// each part's own when there is more than one (see this module's note).
#[derive(Debug, Clone, PartialEq)]
pub struct Hotspots {
    model: Faces,
    parts: Vec<Faces>,
}

impl Hotspots {
    /// `model` is the box round the whole selection, `parts` each part's
    /// own. A lone part's box *is* the selection's, so it adds nothing; past
    /// [`HOTSPOT_PARTS`] the rest are left out.
    pub fn new(model: Faces, parts: impl IntoIterator<Item = Faces>) -> Self {
        let mut parts: Vec<Faces> = parts.into_iter().take(HOTSPOT_PARTS + 1).collect();
        if parts.len() < 2 {
            parts.clear();
        }
        parts.truncate(HOTSPOT_PARTS);
        Hotspots { model, parts }
    }

    pub fn points(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.model
            .hotspots()
            .into_iter()
            .chain(self.parts.iter().flat_map(Faces::part_hotspots))
    }

    /// The hotspot `ray` passes nearest, if it passes within reach of any —
    /// measured on screen, so a far corner is as easy to land on as a near
    /// one. Among several in reach, the one nearest the ray's own line wins,
    /// whichever box it belongs to.
    pub fn nearest(&self, ray: Ray) -> Option<Vec3> {
        self.points()
            .filter_map(|point| {
                let (distance, offset) = ray.nearest(point);
                let reach =
                    HOTSPOT_PICK * arm_length(point, self.model.pose, self.model.orthographic);
                (distance > 0.0 && offset <= reach).then_some((point, offset / reach))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(point, _)| point)
    }

    /// How big a hotspot at `point` is drawn, in studs.
    pub fn radius(&self, point: Vec3) -> f32 {
        self.model.hotspot_radius(point)
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
        let faces = Hotspots::new(faces, []);
        assert_eq!(faces.nearest(near_corner), Some(Vec3::splat(2.0)));
        // Halfway between a face's middle and its edge's, a stud from each.
        let between = toward(Vec3::new(1.0, 0.0, 2.0));
        assert_eq!(faces.nearest(between), None);
        // Pointing away from the box altogether.
        let behind = Ray {
            origin: eye,
            direction: Vec3::Z,
        };
        assert_eq!(faces.nearest(behind), None);
    }

    #[test]
    fn each_part_of_a_model_adds_its_face_middles_and_centre() {
        let left = faces(Vec3::new(-3.0, 0.0, 0.0), Vec3::splat(2.0), Quat::IDENTITY);
        let right = faces(Vec3::new(3.0, 0.0, 0.0), Vec3::splat(2.0), Quat::IDENTITY);
        let model = faces(Vec3::ZERO, Vec3::new(8.0, 2.0, 2.0), Quat::IDENTITY);

        let points: Vec<Vec3> = Hotspots::new(model, [left, right]).points().collect();
        assert_eq!(points.len(), HOTSPOTS + 2 * PART_HOTSPOTS);
        assert!(has(&points, Vec3::new(-3.0, 0.0, 0.0)), "a part's centre");
        assert!(has(&points, Vec3::new(-2.0, 0.0, 0.0)), "a part's face");
        assert!(has(&points, Vec3::new(3.0, 0.0, 1.0)), "the other's face");
        assert!(
            !has(&points, Vec3::new(-2.0, 1.0, 1.0)),
            "no part corner: Studio's shot shows none"
        );

        // One part is the model's own box: nothing added.
        assert_eq!(Hotspots::new(model, [left]).points().count(), HOTSPOTS);
        // Hundreds of parts: capped.
        let many = Hotspots::new(model, std::iter::repeat_n(left, 500));
        assert_eq!(
            many.points().count(),
            HOTSPOTS + HOTSPOT_PARTS * PART_HOTSPOTS
        );
    }

    #[test]
    fn a_ray_lands_on_a_parts_own_hotspot_inside_the_model_box() {
        let left = faces(Vec3::new(-3.0, 0.0, 0.0), Vec3::splat(2.0), Quat::IDENTITY);
        let right = faces(Vec3::new(3.0, 0.0, 0.0), Vec3::splat(2.0), Quat::IDENTITY);
        let model = faces(Vec3::ZERO, Vec3::new(8.0, 2.0, 2.0), Quat::IDENTITY);
        let hotspots = Hotspots::new(model, [left, right]);
        let eye = Vec3::new(0.0, 0.0, 10.0);
        // A hair off the left part's front face middle, nowhere near any of
        // the model box's own.
        let ray = Ray {
            origin: eye,
            direction: (Vec3::new(-2.97, 0.03, 1.0) - eye).normalize(),
        };
        assert_eq!(hotspots.nearest(ray), Some(Vec3::new(-3.0, 0.0, 1.0)));
        assert_eq!(Hotspots::new(model, []).nearest(ray), None);
    }
}
