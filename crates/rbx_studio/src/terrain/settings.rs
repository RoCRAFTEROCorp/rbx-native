//! Every Terrain Editor option `terrain-editor.md` lists, with Studio's
//! ranges: brush size 1–64 studs, strength 0.1–1.

use rbx_terrain::edit::brush::Shape;
use rbx_terrain::edit::region::StudBox;
use rbx_terrain::edit::stroke::FlattenMode;
use rbx_terrain::generate::GenerateSettings;
use rbx_terrain::Material;

pub(crate) const MIN_BRUSH: f32 = 1.0;
pub(crate) const MAX_BRUSH: f32 = 64.0;
pub(crate) const MIN_STRENGTH: f32 = 0.1;
pub(crate) const MAX_STRENGTH: f32 = 1.0;

/// Draw's and Sculpt's two modes; `Ctrl` held flips to the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BrushMode {
    #[default]
    Add,
    Subtract,
}

/// Where the brush sits against the surface under the cursor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Pivot {
    /// Its bottom on the surface: the whole brush above it.
    Bottom,
    #[default]
    Center,
    /// Its top on the surface: the whole brush below it.
    Top,
}

/// The plane a brush can be locked to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PlaneLock {
    #[default]
    Off,
    /// The plane through where the stroke began, facing the camera
    /// ("tilts and rotates with the camera").
    Auto,
    /// A plane set by hand: [`Settings::plane_origin`], facing
    /// [`Settings::plane_normal`].
    Manual,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FillMode {
    #[default]
    Fill,
    Replace,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PaintMode {
    #[default]
    Paint,
    Replace,
}

/// Which material slot a picker writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterialChoice {
    /// Draw/Sculpt's Source Material.
    Brush,
    PaintTarget,
    PaintSource,
    FillSource,
    FillTarget,
    ImportDefault,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settings {
    pub(crate) shape: Shape,
    pub(crate) size: f32,
    /// Box and cylinder only.
    pub(crate) height: f32,
    /// Whether height follows size ("proportionally or independently").
    pub(crate) height_linked: bool,
    pub(crate) strength: f32,
    pub(crate) pivot: Pivot,
    pub(crate) snap: bool,
    pub(crate) plane_lock: PlaneLock,
    pub(crate) plane_origin: [f32; 3],
    pub(crate) plane_normal: [f32; 3],
    pub(crate) ignore_water: bool,
    pub(crate) ignore_parts: bool,
    pub(crate) mode: BrushMode,
    pub(crate) auto_material: bool,
    pub(crate) material: Material,
    pub(crate) flatten_mode: FlattenMode,
    /// Flatten Plane "Fixed": flatten to `flatten_y` rather than to where
    /// the stroke began.
    pub(crate) flatten_fixed: bool,
    pub(crate) flatten_y: f32,
    pub(crate) paint_mode: PaintMode,
    pub(crate) paint_material: Material,
    pub(crate) paint_from: Material,
    pub(crate) fill_mode: FillMode,
    pub(crate) fill_material: Material,
    pub(crate) fill_to: Material,
    /// The selection region every region tool shares.
    pub(crate) region: StudBox,
    pub(crate) region_snap: bool,
    /// Transform's own: the region's target placement turn, in degrees.
    pub(crate) rotation: [f32; 3],
    pub(crate) merge_empty: bool,
    pub(crate) live_edit: bool,
    pub(crate) generate: GenerateSettings,
    pub(crate) import_material: Material,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shape: Shape::Sphere,
            size: 8.0,
            height: 8.0,
            height_linked: true,
            strength: 0.5,
            pivot: Pivot::Center,
            snap: false,
            plane_lock: PlaneLock::Off,
            plane_origin: [0.0; 3],
            plane_normal: [0.0, 1.0, 0.0],
            ignore_water: false,
            ignore_parts: true,
            mode: BrushMode::Add,
            auto_material: false,
            material: Material::Grass,
            flatten_mode: FlattenMode::Both,
            flatten_fixed: false,
            flatten_y: 0.0,
            paint_mode: PaintMode::Paint,
            paint_material: Material::Grass,
            paint_from: Material::Grass,
            fill_mode: FillMode::Fill,
            fill_material: Material::Grass,
            fill_to: Material::Rock,
            region: StudBox::from_center_size([0.0, 0.0, 0.0], [64.0, 32.0, 64.0]),
            region_snap: true,
            rotation: [0.0; 3],
            merge_empty: false,
            live_edit: true,
            generate: GenerateSettings::default(),
            import_material: Material::Grass,
        }
    }
}

impl Settings {
    pub(crate) fn set_size(&mut self, size: f32) {
        self.size = size.clamp(MIN_BRUSH, MAX_BRUSH);
        if self.height_linked {
            self.height = self.size;
        }
    }

    pub(crate) fn set_height(&mut self, height: f32) {
        self.height = height.clamp(MIN_BRUSH, MAX_BRUSH);
    }

    pub(crate) fn set_strength(&mut self, strength: f32) {
        self.strength = strength.clamp(MIN_STRENGTH, MAX_STRENGTH);
    }

    /// The height the brush really has: a sphere is as tall as it is wide.
    pub(crate) fn brush_height(&self) -> f32 {
        match self.shape {
            Shape::Sphere => self.size,
            Shape::Box | Shape::Cylinder => self.height,
        }
    }

    pub(crate) fn material(&self, choice: MaterialChoice) -> Material {
        match choice {
            MaterialChoice::Brush => self.material,
            MaterialChoice::PaintTarget => self.paint_material,
            MaterialChoice::PaintSource => self.paint_from,
            MaterialChoice::FillSource => self.fill_material,
            MaterialChoice::FillTarget => self.fill_to,
            MaterialChoice::ImportDefault => self.import_material,
        }
    }

    pub(crate) fn set_material(&mut self, choice: MaterialChoice, material: Material) {
        let slot = match choice {
            MaterialChoice::Brush => &mut self.material,
            MaterialChoice::PaintTarget => &mut self.paint_material,
            MaterialChoice::PaintSource => &mut self.paint_from,
            MaterialChoice::FillSource => &mut self.fill_material,
            MaterialChoice::FillTarget => &mut self.fill_to,
            MaterialChoice::ImportDefault => &mut self.import_material,
        };
        *slot = material;
    }

    /// The region as the tools use it: whole voxels with Snap to Voxels on.
    pub(crate) fn active_region(&self) -> StudBox {
        if self.region_snap {
            self.region.snapped()
        } else {
            self.region
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_strength_stay_in_studios_ranges() {
        let mut settings = Settings::default();
        settings.set_size(200.0);
        assert_eq!(settings.size, MAX_BRUSH);
        assert_eq!(settings.height, MAX_BRUSH, "linked height follows");
        settings.height_linked = false;
        settings.set_size(0.0);
        assert_eq!(settings.size, MIN_BRUSH);
        assert_eq!(settings.height, MAX_BRUSH);
        settings.set_strength(5.0);
        assert_eq!(settings.strength, MAX_STRENGTH);
        settings.set_strength(0.0);
        assert_eq!(settings.strength, MIN_STRENGTH);
    }

    #[test]
    fn a_sphere_ignores_the_height_setting() {
        let mut settings = Settings {
            height_linked: false,
            ..Settings::default()
        };
        settings.set_height(30.0);
        assert_eq!(settings.brush_height(), settings.size);
        settings.shape = Shape::Cylinder;
        assert_eq!(settings.brush_height(), 30.0);
    }

    #[test]
    fn each_picker_writes_its_own_slot() {
        let mut settings = Settings::default();
        settings.set_material(MaterialChoice::FillTarget, Material::Snow);
        assert_eq!(settings.fill_to, Material::Snow);
        assert_eq!(
            settings.material(MaterialChoice::FillTarget),
            Material::Snow
        );
        assert_eq!(settings.material(MaterialChoice::Brush), Material::Grass);
    }
}
