//! One application of a brush tool at one placement.

use glam::Vec3;
use rbx_terrain::edit::brush::Brush;
use rbx_terrain::edit::stroke::{self, WaterRule};
use rbx_terrain::VoxelGrid;

use super::settings::{BrushMode, PaintMode, Settings};
use super::TerrainTool;

/// What a brush step does, once the tool and the held modifiers have been
/// read: `Ctrl` flips Draw and Sculpt to their other mode, `Shift` turns
/// either into Smooth for as long as it is held.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Effect {
    Draw {
        add: bool,
    },
    Sculpt {
        add: bool,
    },
    Smooth,
    Paint,
    /// Toward the plane at this height.
    Flatten {
        plane_y: f32,
    },
}

impl Effect {
    /// `start_y` is where the stroke began, which is the plane Flatten levels
    /// to unless its plane is fixed.
    pub(crate) fn of(
        tool: TerrainTool,
        settings: &Settings,
        ctrl: bool,
        shift: bool,
        start_y: f32,
    ) -> Option<Effect> {
        let add = (settings.mode == BrushMode::Add) != ctrl;
        Some(match tool {
            TerrainTool::Draw | TerrainTool::Sculpt if shift => Effect::Smooth,
            TerrainTool::Draw => Effect::Draw { add },
            TerrainTool::Sculpt => Effect::Sculpt { add },
            TerrainTool::Smooth => Effect::Smooth,
            TerrainTool::Paint => Effect::Paint,
            TerrainTool::Flatten => Effect::Flatten {
                plane_y: if settings.flatten_fixed {
                    settings.flatten_y
                } else {
                    start_y
                },
            },
            _ => return None,
        })
    }
}

/// The brush `settings` describe, centred on `center`.
pub(crate) fn brush(settings: &Settings, center: Vec3) -> Brush {
    Brush::new(
        settings.shape,
        center.to_array(),
        settings.size,
        settings.brush_height(),
    )
}

/// Applies `effect` once at `center`.
pub(crate) fn apply_brush(grid: &mut VoxelGrid, settings: &Settings, effect: Effect, center: Vec3) {
    let brush = brush(settings, center);
    let water = WaterRule {
        ignore_water: settings.ignore_water,
    };
    let material = if settings.auto_material {
        stroke::auto_material(grid, &brush).unwrap_or(settings.material)
    } else {
        settings.material
    };
    match effect {
        Effect::Draw { add: true } => stroke::draw_add(grid, &brush, material),
        Effect::Draw { add: false } => stroke::draw_subtract(grid, &brush, water),
        Effect::Sculpt { add } => {
            stroke::sculpt(grid, &brush, add, settings.strength, material, water)
        }
        Effect::Smooth => stroke::smooth(grid, &brush, settings.strength, water),
        Effect::Paint => {
            let from = (settings.paint_mode == PaintMode::Replace).then_some(settings.paint_from);
            stroke::paint(grid, &brush, settings.paint_material, from);
        }
        Effect::Flatten { plane_y } => stroke::flatten(
            grid,
            &brush,
            plane_y,
            settings.flatten_mode,
            settings.strength,
            material,
            water,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_terrain::{Cell, Material};

    #[test]
    fn modifiers_flip_and_smooth() {
        let settings = Settings::default();
        assert_eq!(
            Effect::of(TerrainTool::Draw, &settings, false, false, 0.0),
            Some(Effect::Draw { add: true })
        );
        assert_eq!(
            Effect::of(TerrainTool::Draw, &settings, true, false, 0.0),
            Some(Effect::Draw { add: false })
        );
        assert_eq!(
            Effect::of(TerrainTool::Sculpt, &settings, true, true, 0.0),
            Some(Effect::Smooth)
        );
        let subtracting = Settings {
            mode: BrushMode::Subtract,
            ..Settings::default()
        };
        assert_eq!(
            Effect::of(TerrainTool::Sculpt, &subtracting, true, false, 0.0),
            Some(Effect::Sculpt { add: true })
        );
        assert_eq!(
            Effect::of(TerrainTool::Fill, &settings, false, false, 0.0),
            None
        );
    }

    #[test]
    fn flatten_levels_to_the_stroke_start_unless_fixed() {
        let mut settings = Settings::default();
        assert_eq!(
            Effect::of(TerrainTool::Flatten, &settings, false, false, 12.0),
            Some(Effect::Flatten { plane_y: 12.0 })
        );
        settings.flatten_fixed = true;
        settings.flatten_y = -4.0;
        assert_eq!(
            Effect::of(TerrainTool::Flatten, &settings, false, false, 12.0),
            Some(Effect::Flatten { plane_y: -4.0 })
        );
    }

    #[test]
    fn auto_material_draws_what_it_lands_on() {
        let mut grid = VoxelGrid::new();
        grid.set([0, 0, 0], Cell::full(Material::Mud));
        let settings = Settings {
            auto_material: true,
            material: Material::Snow,
            ..Settings::default()
        };
        apply_brush(
            &mut grid,
            &settings,
            Effect::Draw { add: true },
            Vec3::new(6.0, 2.0, 2.0),
        );
        assert_eq!(grid.get([1, 0, 0]).material, Material::Mud);
    }
}
