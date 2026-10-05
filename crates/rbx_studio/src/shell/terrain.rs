//! The Terrain Editor's writes: brush strokes and region gestures from the
//! viewport, and the panel's buttons, each landing in `Workspace.Terrain`
//! as a new `SmoothGrid` (and, once a gesture ends, `PhysicsGrid`).
//!
//! A stroke is one undo step, the way a transform drag is: opened by the
//! press, its later steps overwriting the entry's change log (see
//! `shell::sun` for the same pattern). The voxels are edited in a working
//! copy decoded once at the press, and written back each step so the
//! viewport follows the brush.

mod fields;
mod ops;
mod panel;

use glam::{Mat3, Vec3};
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_terrain::edit::clip::{self, Clip, Placement};
use rbx_terrain::edit::region::StudBox;
use rbx_terrain::VoxelGrid;
use rbx_viewer::pick::{self, PartSurface, Ray};

use crate::terrain::{
    self, Aim, Effect, GridRead, Plane, RegionDrag, RegionGrab, Settings, Surfaces, Tab,
    TerrainTool,
};
use crate::transform::{Action, Tool};
use crate::workspace_view::{Dial, TerrainInput, TerrainPhase};

use super::Shell;

pub(super) use fields::TerrainFields;

/// A brush stroke in progress.
struct Stroke {
    terrain: Ref,
    grid: VoxelGrid,
    plane: Option<Plane>,
    start_y: f32,
}

/// A Transform drag's starting point: the terrain before it, and the
/// region it lifts, so every step re-applies the move from scratch.
struct Lift {
    terrain: Ref,
    base: VoxelGrid,
    source: StudBox,
}

/// The Terrain Editor's state: which tool, its settings, and the gesture in
/// progress.
pub(crate) struct TerrainEditor {
    pub(super) tab: Tab,
    pub(super) tool: Option<TerrainTool>,
    pub(super) settings: Settings,
    /// Transform's target turn.
    pub(super) rotation: Mat3,
    stroke: Option<Stroke>,
    region_drag: Option<RegionDrag>,
    lift: Option<Lift>,
    /// The region Copy or Cut took, waiting for a Paste.
    pub(super) clipboard: Option<Clip>,
    pub(super) heightmap: Option<std::path::PathBuf>,
    pub(super) colormap: Option<std::path::PathBuf>,
    /// The last ray the cursor cast, so a settings change can redraw the
    /// brush where it already is.
    last_ray: Option<(Ray, Option<rbx_viewer::Pose>, bool)>,
    /// Where the terrain Transform moves now stands, which the next move
    /// lifts from: the region as the tool was entered, or as the last move
    /// left it.
    transform_source: Option<StudBox>,
    /// The voxels hovers aim against, and the bytes they were decoded from.
    overlay_grid: Option<VoxelGrid>,
    overlay_source: Vec<u8>,
}

impl Default for TerrainEditor {
    fn default() -> Self {
        TerrainEditor {
            tab: Tab::Edit,
            tool: None,
            settings: Settings::default(),
            rotation: Mat3::IDENTITY,
            stroke: None,
            region_drag: None,
            lift: None,
            clipboard: None,
            heightmap: None,
            colormap: None,
            last_ray: None,
            transform_source: None,
            overlay_grid: None,
            overlay_source: Vec::new(),
        }
    }
}

impl TerrainEditor {
    /// Transform's turn as X, Y, Z degrees (applied in that order).
    pub(super) fn rotation_euler(&self) -> (f32, f32, f32) {
        let (x, y, z) = self.rotation.to_euler(glam::EulerRot::XYZ);
        (x.to_degrees(), y.to_degrees(), z.to_degrees())
    }
}

impl Shell {
    /// Home's Terrain group: one tile opening the Terrain Editor (with its
    /// last tool, Draw the first time) or, while it is up, closing it.
    pub(super) fn terrain_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use gpui_kit::prelude::FluentBuilder as _;
        let showing = self.is_panel_showing(super::layout::Panel::TerrainEditor);
        vec![super::ribbon::tile(
            &self.ribbon_nav,
            "ribbon-terrain-editor",
            gpui_kit::assets::IconName::Mountain,
            "Terrain Editor",
            cx,
        )
        .when(showing, |this| {
            super::ribbon::selected(this, crate::tokens::tool_scale())
        })
        .tooltip(|window, cx| {
            super::tooltip::text("Terrain Editor — create and sculpt terrain", window, cx)
        })
        .on_click(cx.listener(move |shell, _, _, cx| shell.toggle_terrain_editor(!showing, cx)))
        .into_any_element()]
    }

    /// Whether Ctrl+C/X/V/D and Delete mean the terrain region: the Terrain
    /// Editor's Select tool in use and nothing selected in the Explorer
    /// (`terrain-editor.md`'s own condition).
    pub(super) fn terrain_select_keys(&self) -> bool {
        self.transform.tool == Tool::Terrain
            && self.terrain.tool == Some(TerrainTool::Select)
            && self.selected_all().is_empty()
    }

    pub(super) fn toggle_terrain_editor(&mut self, open: bool, cx: &mut Context<Self>) {
        self.set_panel_open(super::layout::Panel::TerrainEditor, open, cx);
        if open {
            let tool = self.terrain.tool.unwrap_or(TerrainTool::Draw);
            self.use_terrain_tool(tool, cx);
        } else if self.transform.tool == Tool::Terrain {
            self.transform_action(Action::Use(Tool::Select), cx);
        }
    }

    /// `RBX_STUDIO_TERRAIN=<tool>[,apply]` opens the Terrain Editor on that
    /// tool at startup, and with `apply` presses its main button (Generate,
    /// Fill, Sea Level's Create, Clear, Import) — a screenshot aid, as
    /// `RBX_STUDIO_TOOL` is for the transform tools.
    pub(super) fn apply_debug_terrain(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var("RBX_STUDIO_TERRAIN") else {
            return;
        };
        let mut words = spec.split(',').map(|w| w.trim().to_ascii_lowercase());
        let Some(name) = words.next() else { return };
        let all = TerrainTool::CREATE.into_iter().chain(TerrainTool::EDIT);
        let Some(tool) = all
            .into_iter()
            .find(|t| t.label().replace(' ', "").eq_ignore_ascii_case(&name))
        else {
            eprintln!("rbxstudio: RBX_STUDIO_TERRAIN: no tool called {name:?}");
            return;
        };
        self.set_panel_open(super::layout::Panel::TerrainEditor, true, cx);
        self.use_terrain_tool(tool, cx);
        if words.any(|w| w == "apply") {
            match tool {
                TerrainTool::Generate => self.terrain_generate(cx),
                TerrainTool::Fill => self.terrain_fill(cx),
                TerrainTool::SeaLevel => self.terrain_sea_level(true, cx),
                TerrainTool::Clear => self.terrain_clear(cx),
                TerrainTool::Import => self.terrain_import(cx),
                _ => {}
            }
        }
    }

    /// Picks a Terrain Editor tool and puts the viewport in its hands.
    pub(super) fn use_terrain_tool(&mut self, tool: TerrainTool, cx: &mut Context<Self>) {
        self.terrain.tool = Some(tool);
        self.terrain.tab = tool.tab();
        self.terrain.stroke = None;
        self.terrain.region_drag = None;
        if tool == TerrainTool::Transform {
            self.terrain.rotation = Mat3::IDENTITY;
            self.terrain.transform_source = Some(self.terrain.settings.active_region());
        }
        self.transform_action(Action::Use(Tool::Terrain), cx);
        self.redraw_terrain_overlay(cx);
    }

    /// The terrain instance and its voxels, or a warning in Output saying
    /// why there is nothing to edit.
    fn terrain_grid(&mut self) -> Option<(Ref, VoxelGrid)> {
        let Some(terrain) = terrain::find_terrain(&self.dom) else {
            self.output
                .push_warning("Terrain Editor: this place has no Workspace.Terrain");
            return None;
        };
        match terrain::read_grid(&self.dom, terrain) {
            GridRead::Grid(grid) => Some((terrain, grid)),
            GridRead::Unreadable(why) => {
                self.output.push_warning(&format!(
                    "Terrain Editor: the terrain's voxels could not be read ({why}); \
                     editing would discard them, so nothing was changed"
                ));
                None
            }
        }
    }

    /// Writes `grid` back and reflects it; `physics` adds `PhysicsGrid`.
    fn store_grid(
        &mut self,
        terrain: Ref,
        grid: &VoxelGrid,
        physics: bool,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = terrain::write_grid(&mut self.dom, terrain, grid, physics) {
            self.output.push_warning(&format!("Terrain Editor: {err}"));
        }
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
    }

    /// One whole edit as one undo step: read, change, write.
    pub(super) fn edit_terrain(
        &mut self,
        edit: impl FnOnce(&mut VoxelGrid),
        cx: &mut Context<Self>,
    ) {
        let Some((terrain, mut grid)) = self.terrain_grid() else {
            cx.notify();
            return;
        };
        self.push_history();
        edit(&mut grid);
        self.store_grid(terrain, &grid, true, cx);
        cx.notify();
    }

    /// Everything the viewport sends while the Terrain Editor has it.
    pub(super) fn terrain_step(&mut self, input: TerrainInput, cx: &mut Context<Self>) {
        let Some(tool) = self.terrain.tool else {
            return;
        };
        if let Some(ray) = input.ray {
            self.terrain.last_ray = Some((ray, input.pose, input.orthographic));
        }
        match input.phase {
            TerrainPhase::Adjust { dial, notches } => self.adjust_dial(dial, notches, cx),
            _ if tool.is_brush() => self.brush_step(tool, input, cx),
            _ if tool.uses_region() => self.region_step(tool, input, cx),
            _ => {}
        }
    }

    fn adjust_dial(&mut self, dial: Dial, notches: f32, cx: &mut Context<Self>) {
        let settings = &mut self.terrain.settings;
        match dial {
            Dial::Size => settings.set_size(settings.size + notches),
            Dial::Height => settings.set_height(settings.height + notches),
            Dial::Strength => settings.set_strength(settings.strength + notches * 0.05),
        }
        self.redraw_terrain_overlay(cx);
        cx.notify();
    }

    fn aim_brush(&self, ray: Ray, plane: Option<Plane>, cx: &App) -> Option<Aim> {
        let meshes = self.viewport.read(cx).meshes().clone();
        let part = |ray: Ray| {
            pick::parts_along(&self.dom, &self.database, &meshes, ray)
                .into_iter()
                .find_map(|part| {
                    PartSurface::read(&self.dom, &self.database, &meshes, part)?.raycast(ray)
                })
        };
        let empty = VoxelGrid::new();
        let grid = match &self.terrain.stroke {
            Some(stroke) => &stroke.grid,
            None => self.terrain.overlay_grid.as_ref().unwrap_or(&empty),
        };
        let surfaces = Surfaces {
            grid,
            part: Some(&part),
        };
        terrain::aim(&self.terrain.settings, &surfaces, ray, plane)
    }

    fn brush_step(&mut self, tool: TerrainTool, input: TerrainInput, cx: &mut Context<Self>) {
        match input.phase {
            TerrainPhase::Hover => {
                self.refresh_overlay_grid();
                let aim = input.ray.and_then(|ray| self.aim_brush(ray, None, cx));
                self.show_brush(aim, input.ctrl, input.shift, cx);
            }
            TerrainPhase::Press => {
                let Some(ray) = input.ray else { return };
                let Some((terrain, grid)) = self.terrain_grid() else {
                    return;
                };
                self.terrain.stroke = Some(Stroke {
                    terrain,
                    grid,
                    plane: None,
                    start_y: 0.0,
                });
                let Some(aim) = self.aim_brush(ray, None, cx) else {
                    self.terrain.stroke = None;
                    return;
                };
                let forward = input.pose.map_or(Vec3::NEG_Z, |pose| pose.basis().2);
                let plane = terrain::stroke_plane(&self.terrain.settings, aim.hit, forward);
                if let Some(stroke) = &mut self.terrain.stroke {
                    stroke.plane = plane;
                    stroke.start_y = aim.hit.y;
                }
                self.push_history();
                self.apply_stroke(tool, aim, input, cx);
            }
            TerrainPhase::Drag => {
                let Some(ray) = input.ray else { return };
                let plane = self.terrain.stroke.as_ref().and_then(|s| s.plane);
                if self.terrain.stroke.is_none() {
                    return;
                }
                if let Some(aim) = self.aim_brush(ray, plane, cx) {
                    self.apply_stroke(tool, aim, input, cx);
                }
            }
            TerrainPhase::Release => {
                if let Some(stroke) = self.terrain.stroke.take() {
                    self.store_grid(stroke.terrain, &stroke.grid, true, cx);
                }
            }
            TerrainPhase::Adjust { .. } => {}
        }
    }

    fn apply_stroke(
        &mut self,
        tool: TerrainTool,
        aim: Aim,
        input: TerrainInput,
        cx: &mut Context<Self>,
    ) {
        let Some(mut stroke) = self.terrain.stroke.take() else {
            return;
        };
        let settings = &self.terrain.settings;
        if let Some(effect) = Effect::of(tool, settings, input.ctrl, input.shift, stroke.start_y) {
            terrain::apply_brush(&mut stroke.grid, settings, effect, aim.center);
        }
        self.store_grid(stroke.terrain, &stroke.grid, false, cx);
        self.terrain.stroke = Some(stroke);
        self.show_brush(Some(aim), input.ctrl, input.shift, cx);
    }

    fn show_brush(&mut self, aim: Option<Aim>, ctrl: bool, shift: bool, cx: &mut Context<Self>) {
        let subtract = match self.terrain.tool {
            Some(TerrainTool::Draw | TerrainTool::Sculpt) => {
                (self.terrain.settings.mode == terrain::BrushMode::Subtract) != ctrl && !shift
            }
            _ => false,
        };
        let segments = aim
            .map(|aim| terrain::brush_outline(&self.terrain.settings, aim.center, subtract))
            .unwrap_or_default();
        self.viewport
            .update(cx, |viewport, _| viewport.show_terrain(segments));
    }

    /// The voxels a hover aims against, re-decoded only when the terrain's
    /// bytes changed (an undo, a script, another tool), not on every move.
    fn refresh_overlay_grid(&mut self) {
        let bytes = terrain::find_terrain(&self.dom)
            .and_then(|terrain| self.dom.get(terrain))
            .and_then(|instance| match instance.properties().get("SmoothGrid") {
                Some(rbx_dom::Variant::String(text)) => Some(text.as_bytes().to_vec()),
                Some(rbx_dom::Variant::Unknown { raw, .. }) => Some(raw.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if self.terrain.overlay_grid.is_some() && bytes == self.terrain.overlay_source {
            return;
        }
        self.terrain.overlay_grid = if bytes.is_empty() {
            Some(VoxelGrid::new())
        } else {
            rbx_terrain::smooth_grid::decode(&bytes).ok()
        };
        self.terrain.overlay_source = bytes;
    }

    fn region_step(&mut self, tool: TerrainTool, input: TerrainInput, cx: &mut Context<Self>) {
        let pose = input.pose;
        match input.phase {
            TerrainPhase::Hover | TerrainPhase::Adjust { .. } => {}
            TerrainPhase::Press => {
                let (Some(ray), Some(pose)) = (input.ray, pose) else {
                    return;
                };
                self.refresh_overlay_grid();
                let surface = self
                    .aim_brush(ray, None, cx)
                    .map_or(Vec3::ZERO, |aim| aim.hit);
                let transform = tool == TerrainTool::Transform;
                let drag = RegionDrag::press(
                    &self.terrain.settings.region,
                    self.terrain.rotation,
                    (pose, input.orthographic),
                    ray,
                    transform,
                    surface,
                );
                // Transform's own gesture moves terrain; drawing a new box
                // with it only re-selects.
                if transform && !matches!(drag.grab, RegionGrab::Draw { .. }) {
                    if let Some((terrain, base)) = self.terrain_grid() {
                        self.push_history();
                        let source = self
                            .terrain
                            .transform_source
                            .unwrap_or_else(|| self.terrain.settings.active_region());
                        self.terrain.lift = Some(Lift {
                            terrain,
                            base,
                            source,
                        });
                    }
                }
                self.terrain.region_drag = Some(drag);
            }
            TerrainPhase::Drag => {
                let (Some(ray), Some(mut drag)) = (input.ray, self.terrain.region_drag) else {
                    return;
                };
                let snap = self.terrain.settings.region_snap;
                let (region, rotation) = drag.step(ray, input.shift, input.ctrl, snap);
                self.terrain.region_drag = Some(drag);
                self.terrain.settings.region = region;
                self.terrain.rotation = rotation;
                if self.terrain.settings.live_edit {
                    self.apply_lift(false, cx);
                }
            }
            TerrainPhase::Release => {
                self.terrain.region_drag = None;
                if self.terrain.lift.is_some() {
                    self.apply_lift(true, cx);
                    self.terrain.lift = None;
                    if self.terrain.settings.live_edit {
                        self.terrain.transform_source = Some(self.terrain.settings.region);
                    }
                }
            }
        }
        self.redraw_terrain_overlay(cx);
        cx.notify();
    }

    fn placement(&self) -> Placement {
        Placement {
            center: self.terrain.settings.region.center(),
            size: self.terrain.settings.region.size(),
            axes: [
                self.terrain.rotation.x_axis.to_array(),
                self.terrain.rotation.y_axis.to_array(),
                self.terrain.rotation.z_axis.to_array(),
            ],
        }
    }

    /// Transform's Apply (with Live Edit off, or a typed position, size or
    /// turn): the source region's terrain moved to where the region now
    /// stands, as one undo step.
    pub(super) fn terrain_apply_transform(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.terrain.transform_source else {
            return;
        };
        let placement = self.placement();
        let merge = self.terrain.settings.merge_empty;
        self.edit_terrain(|grid| clip::transform(grid, &source, &placement, merge), cx);
        self.terrain.transform_source = Some(self.terrain.settings.region);
        self.terrain.rotation = Mat3::IDENTITY;
        self.redraw_terrain_overlay(cx);
    }

    /// A number typed into the panel.
    pub(super) fn set_terrain_number(
        &mut self,
        number: fields::Number,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        use fields::Number;
        if !value.is_finite() {
            return;
        }
        let settings = &mut self.terrain.settings;
        let mut center = settings.region.center();
        let mut size = settings.region.size();
        match number {
            Number::Position(axis) => center[axis] = value,
            Number::Size(axis) => size[axis] = value.max(rbx_terrain::VOXEL_STUDS),
            Number::Rotation(axis) => {
                let (x, y, z) = self.terrain.rotation_euler();
                let mut angles = [x, y, z];
                angles[axis] = value;
                self.terrain.rotation = Mat3::from_euler(
                    glam::EulerRot::XYZ,
                    angles[0].to_radians(),
                    angles[1].to_radians(),
                    angles[2].to_radians(),
                );
            }
            Number::PlaneY => settings.plane_origin[1] = value,
            Number::FlattenY => settings.flatten_y = value,
            Number::Seed => settings.generate.seed = value.max(0.0) as u32,
        }
        if matches!(number, Number::Position(_) | Number::Size(_)) {
            self.terrain.settings.region = StudBox::from_center_size(center, size);
        }
        let moves = matches!(
            number,
            Number::Position(_) | Number::Size(_) | Number::Rotation(_)
        );
        if moves
            && self.terrain.tool == Some(TerrainTool::Transform)
            && self.terrain.settings.live_edit
        {
            self.terrain_apply_transform(cx);
        }
        self.redraw_terrain_overlay(cx);
        cx.notify();
    }

    /// Re-applies the Transform drag to its starting terrain.
    fn apply_lift(&mut self, physics: bool, cx: &mut Context<Self>) {
        let Some(lift) = &self.terrain.lift else {
            return;
        };
        let mut grid = lift.base.clone();
        let placement = self.placement();
        clip::transform(
            &mut grid,
            &lift.source,
            &placement,
            self.terrain.settings.merge_empty,
        );
        let terrain = lift.terrain;
        self.store_grid(terrain, &grid, physics, cx);
    }

    /// Redraws whatever the active tool shows: the region and its handles,
    /// or the brush where the cursor last was.
    pub(super) fn redraw_terrain_overlay(&mut self, cx: &mut Context<Self>) {
        if self.transform.tool != Tool::Terrain {
            self.viewport
                .update(cx, |viewport, _| viewport.show_terrain(Vec::new()));
            return;
        }
        let Some(tool) = self.terrain.tool else {
            return;
        };
        if tool.uses_region() {
            let region = self.terrain.settings.region;
            let rotation = self.terrain.rotation;
            let model = terrain::region_model(&region, rotation);
            let segments = match self
                .terrain
                .last_ray
                .and_then(|(_, pose, ortho)| Some((pose?, ortho)))
            {
                Some((pose, ortho)) => {
                    let (faces, handles) = terrain::region_handles(&region, rotation, pose, ortho);
                    let transform = tool == TerrainTool::Transform;
                    terrain::region_outline(model, Some(&faces), transform.then_some(&handles))
                }
                None => terrain::region_outline(model, None, None),
            };
            self.viewport
                .update(cx, |viewport, _| viewport.show_terrain(segments));
        } else if tool.is_brush() {
            let aim = self
                .terrain
                .last_ray
                .and_then(|(ray, ..)| self.aim_brush(ray, None, cx));
            self.show_brush(aim, false, false, cx);
        } else {
            self.viewport
                .update(cx, |viewport, _| viewport.show_terrain(Vec::new()));
        }
    }
}
