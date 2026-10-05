//! The brush tools' sections: Draw, Sculpt, Smooth, Paint and Flatten,
//! after `terrain-editor.md`'s per-tool option tables.

use gpui_kit::*;
use rbx_terrain::edit::brush::Shape;
use rbx_terrain::edit::stroke::FlattenMode;

use crate::terrain::{BrushMode, MaterialChoice, PaintMode, Pivot, PlaneLock, TerrainTool};

use super::super::super::Shell;
use super::super::fields::{Number, Rail};
use super::controls::{chips, number_row, rail_row, section, switch_row};

impl Shell {
    pub(super) fn brush_section(
        &mut self,
        tool: TerrainTool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fields = self.fields();
        let (size, height, strength, flatten_y) = (
            fields.rail(Rail::Size).clone(),
            fields.rail(Rail::Height).clone(),
            fields.rail(Rail::Strength).clone(),
            fields.number(Number::FlattenY).clone(),
        );
        let plane_origin: Vec<_> = (0..3)
            .map(|a| fields.number(Number::PlaneOrigin(a)).clone())
            .collect();
        let plane_tilt: Vec<_> = (0..2)
            .map(|a| fields.number(Number::PlaneTilt(a)).clone())
            .collect();
        let s = self.terrain.settings.clone();
        let mut rows = Vec::new();
        if matches!(tool, TerrainTool::Draw | TerrainTool::Sculpt) {
            rows.push(chips(
                "terrain-mode",
                &[(BrushMode::Add, "Add"), (BrushMode::Subtract, "Subtract")],
                s.mode,
                cx,
                |shell, mode, cx| {
                    shell.terrain.settings.mode = mode;
                    cx.notify();
                },
            ));
        }
        if tool == TerrainTool::Flatten {
            rows.push(chips(
                "terrain-flatten-mode",
                &[
                    (FlattenMode::Erode, "Erode to Flat"),
                    (FlattenMode::Grow, "Grow to Flat"),
                    (FlattenMode::Both, "Flatten All"),
                ],
                s.flatten_mode,
                cx,
                |shell, mode, cx| {
                    shell.terrain.settings.flatten_mode = mode;
                    cx.notify();
                },
            ));
        }
        rows.push(chips(
            "terrain-shape",
            &[
                (Shape::Sphere, "Sphere"),
                (Shape::Box, "Box"),
                (Shape::Cylinder, "Cylinder"),
            ],
            s.shape,
            cx,
            |shell, shape, cx| {
                shell.terrain.settings.shape = shape;
                shell.redraw_terrain_overlay(cx);
                cx.notify();
            },
        ));
        rows.push(rail_row("Brush Size", &size, cx));
        if s.shape != Shape::Sphere {
            rows.push(rail_row("Height", &height, cx));
            rows.push(switch_row(
                "terrain-height-link",
                "Height follows size",
                s.height_linked,
                cx,
                |shell, on, cx| {
                    shell.terrain.settings.height_linked = on;
                    if on {
                        let size = shell.terrain.settings.size;
                        shell.terrain.settings.set_size(size);
                    }
                    cx.notify();
                },
            ));
        }
        if matches!(
            tool,
            TerrainTool::Sculpt | TerrainTool::Smooth | TerrainTool::Flatten
        ) {
            rows.push(rail_row("Strength", &strength, cx));
        }
        if tool != TerrainTool::Sculpt {
            rows.push(chips(
                "terrain-pivot",
                &[
                    (Pivot::Bottom, "Bottom"),
                    (Pivot::Center, "Center"),
                    (Pivot::Top, "Top"),
                ],
                s.pivot,
                cx,
                |shell, pivot, cx| {
                    shell.terrain.settings.pivot = pivot;
                    cx.notify();
                },
            ));
            rows.push(switch_row(
                "terrain-brush-snap",
                "Snap to Voxels",
                s.snap,
                cx,
                |shell, on, cx| {
                    shell.terrain.settings.snap = on;
                    cx.notify();
                },
            ));
        }
        if tool == TerrainTool::Flatten {
            rows.push(switch_row(
                "terrain-flatten-fixed",
                "Fixed plane",
                s.flatten_fixed,
                cx,
                |shell, on, cx| {
                    shell.terrain.settings.flatten_fixed = on;
                    cx.notify();
                },
            ));
            if s.flatten_fixed {
                rows.push(number_row("Plane Y", vec![flatten_y], cx));
            }
        } else {
            rows.push(chips(
                "terrain-plane-lock",
                &[
                    (PlaneLock::Off, "No plane"),
                    (PlaneLock::Auto, "Auto"),
                    (PlaneLock::Manual, "Manual"),
                ],
                s.plane_lock,
                cx,
                |shell, lock, cx| {
                    shell.terrain.settings.plane_lock = lock;
                    cx.notify();
                },
            ));
            if s.plane_lock == PlaneLock::Manual {
                rows.push(number_row("Plane position", plane_origin, cx));
                rows.push(number_row("Plane tilt (X, Z)", plane_tilt, cx));
            }
        }
        rows.push(switch_row(
            "terrain-ignore-water",
            "Ignore Water",
            s.ignore_water,
            cx,
            |shell, on, cx| {
                shell.terrain.settings.ignore_water = on;
                cx.notify();
            },
        ));
        rows.push(switch_row(
            "terrain-ignore-parts",
            "Ignore Parts",
            s.ignore_parts,
            cx,
            |shell, on, cx| {
                shell.terrain.settings.ignore_parts = on;
                cx.notify();
            },
        ));
        section("BRUSH", rows).into_any_element()
    }

    pub(super) fn brush_material_section(
        &mut self,
        tool: TerrainTool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = self.terrain.settings.clone();
        let mut rows = Vec::new();
        match tool {
            TerrainTool::Paint => {
                rows.push(chips(
                    "terrain-paint-mode",
                    &[(PaintMode::Paint, "Paint"), (PaintMode::Replace, "Replace")],
                    s.paint_mode,
                    cx,
                    |shell, mode, cx| {
                        shell.terrain.settings.paint_mode = mode;
                        cx.notify();
                    },
                ));
                if s.paint_mode == PaintMode::Replace {
                    rows.push(self.material_picker(
                        "Replace",
                        MaterialChoice::PaintSource,
                        false,
                        cx,
                    ));
                    rows.push(self.material_picker("With", MaterialChoice::PaintTarget, false, cx));
                } else {
                    rows.push(self.material_picker(
                        "Paint",
                        MaterialChoice::PaintTarget,
                        false,
                        cx,
                    ));
                }
            }
            TerrainTool::Smooth => return div().into_any_element(),
            _ => {
                rows.push(switch_row(
                    "terrain-auto-material",
                    "Auto Material",
                    s.auto_material,
                    cx,
                    |shell, on, cx| {
                        shell.terrain.settings.auto_material = on;
                        cx.notify();
                    },
                ));
                if !s.auto_material {
                    rows.push(self.material_picker(
                        "Source Material",
                        MaterialChoice::Brush,
                        false,
                        cx,
                    ));
                }
            }
        }
        section("MATERIAL", rows).into_any_element()
    }
}
