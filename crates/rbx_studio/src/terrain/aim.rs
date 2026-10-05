//! Where the brush lands: what the cursor's ray meets — terrain, a part, or
//! a locked plane — and how the brush sits against it.

use glam::Vec3;
#[cfg(test)]
use rbx_terrain::VoxelGrid;
use rbx_terrain::{Voxels, VOXEL_STUDS};
use rbx_viewer::pick::Ray;

use super::settings::{Pivot, PlaneLock, Settings};

/// How far the cursor may reach for a surface, in studs.
const REACH: f32 = 10_000.0;
/// Where the brush floats when the ray meets nothing at all: far enough to
/// see, near enough to draw with.
const EMPTY_REACH: f32 = 96.0;

/// What a ray can land on besides the voxels: the nearest part along it, as
/// a distance and the surface normal there (`None` with Ignore Parts on).
pub(crate) struct Surfaces<'a> {
    pub(crate) grid: &'a dyn Voxels,
    pub(crate) part: Option<&'a dyn Fn(Ray) -> Option<(f32, Vec3)>>,
}

/// One placement of the brush.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Aim {
    /// The brush's centre.
    pub(crate) center: Vec3,
    /// Where the ray met the surface.
    pub(crate) hit: Vec3,
    pub(crate) normal: Vec3,
}

/// Manual plane lock's normal as tilts in degrees: about X, then about Z,
/// from straight up — the two turns a plane facing up can take.
pub(crate) fn plane_tilt(normal: [f32; 3]) -> [f32; 2] {
    let n = Vec3::from(normal).normalize_or(Vec3::Y);
    // n = Rz(z) · Rx(x) · Y = (-sin z·cos x, cos z·cos x, sin x).
    let x = n.z.clamp(-1.0, 1.0).asin();
    let z = (-n.x).atan2(n.y);
    [x.to_degrees(), z.to_degrees()]
}

/// The inverse of [`plane_tilt`].
pub(crate) fn plane_normal(tilt: [f32; 2]) -> [f32; 3] {
    let (x, z) = (tilt[0].to_radians(), tilt[1].to_radians());
    [-z.sin() * x.cos(), z.cos() * x.cos(), x.sin()]
}

/// A plane a stroke is locked to: through `origin`, facing `normal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Plane {
    pub(crate) origin: Vec3,
    pub(crate) normal: Vec3,
}

impl Plane {
    fn crossing(&self, ray: Ray) -> Option<Vec3> {
        let facing = ray.direction.dot(self.normal);
        if facing.abs() < 1e-5 {
            return None;
        }
        let t = (self.origin - ray.origin).dot(self.normal) / facing;
        (t > 0.0).then(|| ray.origin + ray.direction * t)
    }
}

/// The plane a stroke beginning at `hit` locks to, if the settings lock
/// one: Manual's own, or Auto's through the first hit, facing back along
/// the camera's view (`forward`).
pub(crate) fn stroke_plane(settings: &Settings, hit: Vec3, forward: Vec3) -> Option<Plane> {
    match settings.plane_lock {
        PlaneLock::Off => None,
        PlaneLock::Auto => Some(Plane {
            origin: hit,
            normal: (-forward).normalize_or(Vec3::Y),
        }),
        PlaneLock::Manual => Some(Plane {
            origin: Vec3::from(settings.plane_origin),
            normal: Vec3::from(settings.plane_normal).normalize_or(Vec3::Y),
        }),
    }
}

/// Places the brush for `ray`. With `plane` (a stroke locked to one), the
/// ray only ever meets that plane; otherwise the nearest of the terrain and
/// (unless ignored) the parts, then the ground plane `y = 0`, then a point
/// in mid-air.
pub(crate) fn aim(
    settings: &Settings,
    surfaces: &Surfaces<'_>,
    ray: Ray,
    plane: Option<Plane>,
) -> Option<Aim> {
    let (hit, normal) = match plane {
        Some(plane) => (plane.crossing(ray)?, plane.normal),
        None => surface(settings, surfaces, ray),
    };
    let half = settings.brush_height() * 0.5;
    let lift = match settings.pivot {
        Pivot::Bottom => half,
        Pivot::Center => 0.0,
        Pivot::Top => -half,
    };
    let mut center = hit + Vec3::Y * lift;
    if settings.snap {
        center = center.map(|v| {
            ((v - VOXEL_STUDS * 0.5) / VOXEL_STUDS).round() * VOXEL_STUDS + VOXEL_STUDS * 0.5
        });
    }
    Some(Aim {
        center,
        hit,
        normal,
    })
}

fn surface(settings: &Settings, surfaces: &Surfaces<'_>, ray: Ray) -> (Vec3, Vec3) {
    let terrain = rbx_terrain::raycast(
        surfaces.grid,
        ray.origin.to_array(),
        ray.direction.to_array(),
        REACH,
        settings.ignore_water,
    )
    .map(|hit| (hit.distance, Vec3::from(hit.normal)));
    let part = if settings.ignore_parts {
        None
    } else {
        surfaces.part.and_then(|cast| cast(ray))
    };
    let nearest = match (terrain, part) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    };
    if let Some((distance, normal)) = nearest {
        return (ray.at(distance), normal);
    }
    let ground = Plane {
        origin: Vec3::ZERO,
        normal: Vec3::Y,
    };
    match ground.crossing(ray) {
        Some(point) if point.distance(ray.origin) < REACH => (point, Vec3::Y),
        _ => (ray.at(EMPTY_REACH), -ray.direction),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_terrain::{Cell, Material};

    fn down_at(x: f32, z: f32) -> Ray {
        Ray {
            origin: Vec3::new(x, 100.0, z),
            direction: Vec3::NEG_Y,
        }
    }

    fn floor() -> VoxelGrid {
        let mut grid = VoxelGrid::new();
        for x in -4..4 {
            for z in -4..4 {
                grid.set([x, 1, z], Cell::full(Material::Grass));
            }
        }
        grid
    }

    #[test]
    fn the_brush_lands_on_terrain_and_sits_by_its_pivot() {
        let grid = floor();
        let surfaces = Surfaces {
            grid: &grid,
            part: None,
        };
        let mut settings = Settings::default();
        let aim_at =
            |settings: &Settings| aim(settings, &surfaces, down_at(1.0, 1.0), None).unwrap();
        let centred = aim_at(&settings);
        assert!((centred.hit.y - 8.0).abs() < 1e-3);
        assert_eq!(centred.center, centred.hit);
        settings.pivot = Pivot::Bottom;
        assert!((aim_at(&settings).center.y - 12.0).abs() < 1e-3);
        settings.pivot = Pivot::Top;
        assert!((aim_at(&settings).center.y - 4.0).abs() < 1e-3);
        settings.snap = true;
        settings.pivot = Pivot::Center;
        assert_eq!(aim_at(&settings).center, Vec3::new(2.0, 10.0, 2.0));
    }

    #[test]
    fn nothing_under_the_cursor_falls_to_the_ground_plane() {
        let grid = VoxelGrid::new();
        let surfaces = Surfaces {
            grid: &grid,
            part: None,
        };
        let aim = aim(&Settings::default(), &surfaces, down_at(5.0, 7.0), None).unwrap();
        assert_eq!(aim.hit, Vec3::new(5.0, 0.0, 7.0));
    }

    #[test]
    fn parts_count_unless_ignored() {
        let grid = floor();
        let part = |_: Ray| Some((10.0, Vec3::Y));
        let surfaces = Surfaces {
            grid: &grid,
            part: Some(&part),
        };
        let mut settings = Settings::default();
        assert!(
            (aim(&settings, &surfaces, down_at(1.0, 1.0), None)
                .unwrap()
                .hit
                .y
                - 8.0)
                .abs()
                < 1e-3
        );
        settings.ignore_parts = false;
        assert!(
            (aim(&settings, &surfaces, down_at(1.0, 1.0), None)
                .unwrap()
                .hit
                .y
                - 90.0)
                .abs()
                < 1e-3
        );
    }

    #[test]
    fn plane_tilts_round_trip_through_the_normal() {
        for tilt in [[0.0, 0.0], [30.0, 0.0], [0.0, -45.0], [20.0, 60.0]] {
            let back = plane_tilt(plane_normal(tilt));
            assert!(
                (back[0] - tilt[0]).abs() < 1e-3 && (back[1] - tilt[1]).abs() < 1e-3,
                "{tilt:?} -> {back:?}"
            );
        }
        assert_eq!(plane_normal([0.0, 0.0]), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_locked_plane_holds_the_stroke() {
        let grid = floor();
        let surfaces = Surfaces {
            grid: &grid,
            part: None,
        };
        let mut settings = Settings {
            plane_lock: PlaneLock::Manual,
            plane_origin: [0.0, 30.0, 0.0],
            ..Settings::default()
        };
        let plane = stroke_plane(&settings, Vec3::ZERO, Vec3::NEG_Y);
        let aim = aim(&settings, &surfaces, down_at(1.0, 1.0), plane).unwrap();
        assert!((aim.hit.y - 30.0).abs() < 1e-3);
        settings.plane_lock = PlaneLock::Auto;
        let auto = stroke_plane(
            &settings,
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        )
        .unwrap();
        assert_eq!(auto.normal, Vec3::new(0.0, 0.0, -1.0));
        settings.plane_lock = PlaneLock::Off;
        assert!(stroke_plane(&settings, Vec3::ZERO, Vec3::Y).is_none());
    }
}
