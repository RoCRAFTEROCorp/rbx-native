//! The Terrain Editor: its tools, their settings, and the gestures that turn
//! the cursor into edits of `Workspace.Terrain`'s voxels.
//!
//! The toolset is `creator-docs`' `studio/terrain-editor.md`, tab for tab:
//! Create holds Import, Generate and Clear; Edit holds Select, Transform,
//! Fill, Sea Level, Draw, Sculpt, Smooth, Paint and Flatten. The voxel
//! arithmetic itself is `rbx_terrain::edit`'s; this module decides where a
//! brush lands and what each tool does with it. `shell::terrain` writes the
//! result into the DOM and draws the panel.

mod aim;
mod apply;
mod dom;
mod outline;
mod region;
mod settings;

pub(crate) use aim::{aim, plane_normal, plane_tilt, stroke_plane, Aim, Plane, Surfaces};
pub(crate) use apply::{apply_brush, brush, Effect};
pub(crate) use dom::{find_terrain, grid_bytes, write_encoded};
pub(crate) use outline::{brush_outline, region_model, region_outline};
pub(crate) use region::{region_handles, RegionDrag, RegionGrab};
pub(crate) use settings::{
    BrushMode, FillMode, MaterialChoice, PaintMode, Pivot, PlaneLock, Settings,
};

use gpui_kit::assets::IconName;

/// The Terrain Editor's two tabs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    Create,
    #[default]
    Edit,
}

/// One tool of the Terrain Editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TerrainTool {
    Import,
    Generate,
    Clear,
    Select,
    Transform,
    Fill,
    SeaLevel,
    Draw,
    Sculpt,
    Smooth,
    Paint,
    Flatten,
}

impl TerrainTool {
    pub(crate) const CREATE: [TerrainTool; 3] = [
        TerrainTool::Import,
        TerrainTool::Generate,
        TerrainTool::Clear,
    ];
    pub(crate) const EDIT: [TerrainTool; 9] = [
        TerrainTool::Select,
        TerrainTool::Transform,
        TerrainTool::Fill,
        TerrainTool::SeaLevel,
        TerrainTool::Draw,
        TerrainTool::Sculpt,
        TerrainTool::Smooth,
        TerrainTool::Paint,
        TerrainTool::Flatten,
    ];

    pub(crate) fn tab(self) -> Tab {
        if Self::CREATE.contains(&self) {
            Tab::Create
        } else {
            Tab::Edit
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            TerrainTool::Import => "Import",
            TerrainTool::Generate => "Generate",
            TerrainTool::Clear => "Clear",
            TerrainTool::Select => "Select",
            TerrainTool::Transform => "Transform",
            TerrainTool::Fill => "Fill",
            TerrainTool::SeaLevel => "Sea Level",
            TerrainTool::Draw => "Draw",
            TerrainTool::Sculpt => "Sculpt",
            TerrainTool::Smooth => "Smooth",
            TerrainTool::Paint => "Paint",
            TerrainTool::Flatten => "Flatten",
        }
    }

    /// What the tool is for, for its tooltip — `terrain-editor.md`'s own
    /// one-line summaries.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            TerrainTool::Import => "Apply a heightmap and optional colormap to a region",
            TerrainTool::Generate => "Procedurally generate terrain within a region",
            TerrainTool::Clear => "Clear all terrain in the place",
            TerrainTool::Select => "Select a rectangular region of terrain",
            TerrainTool::Transform => "Move, resize or turn the selected region",
            TerrainTool::Fill => {
                "Fill a region with a material, or replace one material with another"
            }
            TerrainTool::SeaLevel => {
                "Create a consistent water level, or remove all water, in a region"
            }
            TerrainTool::Draw => {
                "Add or subtract terrain with the brush (Ctrl subtracts, Shift smooths)"
            }
            TerrainTool::Sculpt => {
                "Add or subtract terrain gently, with a strength (Ctrl subtracts, Shift smooths)"
            }
            TerrainTool::Smooth => "Smooth out abrupt edges with the brush",
            TerrainTool::Paint => {
                "Paint a material over terrain, or replace one material with another"
            }
            TerrainTool::Flatten => "Flatten terrain to a plane",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            TerrainTool::Import => IconName::ImageUp,
            TerrainTool::Generate => IconName::Mountain,
            TerrainTool::Clear => IconName::Eraser,
            TerrainTool::Select => IconName::SquareDashed,
            TerrainTool::Transform => IconName::Move3d,
            TerrainTool::Fill => IconName::PaintBucket,
            TerrainTool::SeaLevel => IconName::Droplets,
            TerrainTool::Draw => IconName::Pencil,
            TerrainTool::Sculpt => IconName::Hand,
            TerrainTool::Smooth => IconName::Spline,
            TerrainTool::Paint => IconName::Paintbrush,
            TerrainTool::Flatten => IconName::Minus,
        }
    }

    /// The tools driven by the brush under the cursor.
    pub(crate) fn is_brush(self) -> bool {
        matches!(
            self,
            TerrainTool::Draw
                | TerrainTool::Sculpt
                | TerrainTool::Smooth
                | TerrainTool::Paint
                | TerrainTool::Flatten
        )
    }

    /// The tools that act on the selection region, and so show it.
    pub(crate) fn uses_region(self) -> bool {
        matches!(
            self,
            TerrainTool::Import
                | TerrainTool::Generate
                | TerrainTool::Select
                | TerrainTool::Transform
                | TerrainTool::Fill
                | TerrainTool::SeaLevel
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_sits_on_exactly_one_tab() {
        for tool in TerrainTool::CREATE {
            assert_eq!(tool.tab(), Tab::Create);
            assert!(!tool.is_brush());
        }
        for tool in TerrainTool::EDIT {
            assert_eq!(tool.tab(), Tab::Edit);
        }
        let brushes = TerrainTool::EDIT.iter().filter(|t| t.is_brush()).count();
        assert_eq!(brushes, 5);
        assert!(TerrainTool::EDIT
            .iter()
            .all(|t| t.is_brush() != t.uses_region()));
    }
}
