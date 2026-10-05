//! The Terrain Editor's one-shot operations: the panel's buttons, and the
//! Select tool's clipboard keys. Each is one undo step.

use std::path::Path;

use gpui_kit::*;
use rbx_terrain::edit::clip::{self, Placement};
use rbx_terrain::edit::region;
use rbx_terrain::import::{self, Colormap, Heightmap, MAX_SIDE};

use crate::terrain::{FillMode, TerrainTool};

use super::super::Shell;

impl Shell {
    /// Fill's Apply: the region filled with the source material, or (in
    /// Replace mode) the source swapped for the target.
    pub(in crate::shell) fn terrain_fill(&mut self, cx: &mut Context<Self>) {
        let settings = self.terrain.settings.clone();
        let area = settings.active_region();
        self.edit_terrain(
            |grid| match settings.fill_mode {
                FillMode::Fill => region::fill(grid, &area, settings.fill_material),
                FillMode::Replace => {
                    region::replace(grid, &area, settings.fill_material, settings.fill_to)
                }
            },
            cx,
        );
    }

    /// Sea Level's Create (`true`) or Evaporate (`false`).
    pub(in crate::shell) fn terrain_sea_level(&mut self, create: bool, cx: &mut Context<Self>) {
        let area = self.terrain.settings.active_region();
        self.edit_terrain(
            |grid| {
                if create {
                    region::sea_level_create(grid, &area);
                } else {
                    region::sea_level_evaporate(grid, &area);
                }
            },
            cx,
        );
    }

    pub(in crate::shell) fn terrain_generate(&mut self, cx: &mut Context<Self>) {
        let settings = self.terrain.settings.generate.clone();
        if settings.biomes.is_empty() {
            self.output
                .push_warning("Terrain Editor: Generate needs at least one biome ticked");
            cx.notify();
            return;
        }
        let area = self.terrain.settings.active_region();
        self.edit_terrain(
            |grid| rbx_terrain::generate::generate(grid, &area, &settings),
            cx,
        );
    }

    /// Clear's button: every voxel in the place.
    pub(in crate::shell) fn terrain_clear(&mut self, cx: &mut Context<Self>) {
        self.edit_terrain(|grid| grid.clear(), cx);
    }

    /// The Select tool's Delete key.
    pub(in crate::shell) fn terrain_delete_region(&mut self, cx: &mut Context<Self>) {
        let area = self.terrain.settings.active_region();
        self.edit_terrain(|grid| region::delete(grid, &area), cx);
    }

    /// Ctrl+C (`cut` false) or Ctrl+X: the region's voxels to the clipboard.
    pub(in crate::shell) fn terrain_copy(&mut self, cut: bool, cx: &mut Context<Self>) {
        let area = self.terrain.settings.active_region();
        let Some((_, grid)) = self.terrain_grid() else {
            return;
        };
        self.terrain.clipboard = Some(clip::copy(&grid, &area));
        if cut {
            self.edit_terrain(|grid| region::delete(grid, &area), cx);
        }
        cx.notify();
    }

    /// Ctrl+V and Ctrl+D: the clipboard (or, for a duplicate, the region
    /// itself) put down where the region is, then the Transform tool to
    /// move it on — `terrain-editor.md`'s "swap to the Transform tool".
    pub(in crate::shell) fn terrain_paste(&mut self, duplicate: bool, cx: &mut Context<Self>) {
        let area = self.terrain.settings.active_region();
        if duplicate {
            if let Some((_, grid)) = self.terrain_grid() {
                self.terrain.clipboard = Some(clip::copy(&grid, &area));
            }
        }
        let Some(clip) = self.terrain.clipboard.clone() else {
            return;
        };
        let size = clip.studs();
        let center = area.center();
        self.terrain.settings.region =
            rbx_terrain::edit::region::StudBox::from_center_size(center, size);
        let placement = Placement::at(&self.terrain.settings.region);
        let merge = self.terrain.settings.merge_empty;
        self.edit_terrain(|grid| clip::paste(grid, &clip, &placement, merge), cx);
        self.use_terrain_tool(TerrainTool::Transform, cx);
    }

    /// Import's button: the heightmap (and colormap) over the region.
    pub(in crate::shell) fn terrain_import(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.terrain.heightmap.clone() else {
            self.output
                .push_warning("Terrain Editor: choose a heightmap image first");
            cx.notify();
            return;
        };
        let heightmap = match read_heightmap(&path) {
            Ok(map) => map,
            Err(why) => {
                self.output.push_warning(&format!("Terrain Editor: {why}"));
                cx.notify();
                return;
            }
        };
        let colormap = match self
            .terrain
            .colormap
            .clone()
            .map(|path| read_colormap(&path))
        {
            Some(Ok(map)) => Some(map),
            Some(Err(why)) => {
                self.output.push_warning(&format!("Terrain Editor: {why}"));
                cx.notify();
                return;
            }
            None => None,
        };
        let area = self.terrain.settings.active_region();
        let material = self.terrain.settings.import_material;
        self.edit_terrain(
            |grid| import::import(grid, &area, &heightmap, colormap.as_ref(), material),
            cx,
        );
    }

    /// Asks for an image file and keeps its path as the heightmap
    /// (`colormap` false) or the colormap.
    pub(in crate::shell) fn terrain_choose_image(
        &mut self,
        colormap: bool,
        cx: &mut Context<Self>,
    ) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(if colormap {
                "Colormap".into()
            } else {
                "Heightmap".into()
            }),
        });
        cx.spawn(async move |shell, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = shell.update(cx, |shell, cx| {
                if colormap {
                    shell.terrain.colormap = Some(path);
                } else {
                    shell.terrain.heightmap = Some(path);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn open_image(path: &Path) -> Result<image::DynamicImage, String> {
    let image = image::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    if image.width() > MAX_SIDE || image.height() > MAX_SIDE {
        return Err(format!(
            "{} is {}×{}; Studio takes at most {MAX_SIDE}×{MAX_SIDE}",
            path.display(),
            image.width(),
            image.height()
        ));
    }
    Ok(image)
}

/// Brightness, 16-bit where the file has it: a 16-bit heightmap's extra
/// steps are real height.
fn read_heightmap(path: &Path) -> Result<Heightmap, String> {
    let image = open_image(path)?.into_luma16();
    Ok(Heightmap {
        width: image.width(),
        height: image.height(),
        values: image
            .pixels()
            .map(|p| f32::from(p.0[0]) / 65535.0)
            .collect(),
    })
}

fn read_colormap(path: &Path) -> Result<Colormap, String> {
    let image = open_image(path)?.into_rgb8();
    Ok(Colormap {
        width: image.width(),
        height: image.height(),
        pixels: image.pixels().map(|p| p.0).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heightmaps_read_brightness_and_colormaps_rgb() {
        let dir = std::env::temp_dir().join(format!("rbx-terrain-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ramp.png");
        let ramp =
            image::GrayImage::from_fn(2, 1, |x, _| image::Luma([if x == 0 { 0 } else { 255 }]));
        ramp.save(&path).unwrap();
        let heightmap = read_heightmap(&path).unwrap();
        assert_eq!(heightmap.values, vec![0.0, 1.0]);
        let colors = read_colormap(&path).unwrap();
        assert_eq!(colors.pixels, vec![[0, 0, 0], [255, 255, 255]]);
        assert!(read_heightmap(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
