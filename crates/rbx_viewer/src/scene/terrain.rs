//! The place's smooth terrain: `Workspace.Terrain`'s voxels decoded from
//! `SmoothGrid`, the tint of each material from `MaterialColors`, and how its
//! water looks.
//!
//! Pure DOM extraction; the chunk meshes are built and uploaded by
//! `renderer::terrain`. A `SmoothGrid` this cannot read draws no terrain and
//! says why, rather than guessing at bytes.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use glam::Vec3;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_terrain::{ChunkKey, Material, MaterialColors, VoxelGrid, VOXEL_STUDS};

use super::material::{Catalog, Kind, Slot};
use super::props::{bool_or, float_or, linear_color_or, vector3_or, Properties};
use super::Bounds;

pub(crate) const TERRAIN_CLASS: &str = "Terrain";

/// How the place's water looks: `Terrain`'s Water* properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Water {
    /// Linear.
    pub(crate) color: [f32; 3],
    pub(crate) transparency: f32,
    pub(crate) reflectance: f32,
    pub(crate) wave_size: f32,
    pub(crate) wave_speed: f32,
}

/// `Terrain.Decoration`'s animated grass, and the wind it sways in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Grass {
    pub(crate) enabled: bool,
    /// `GrassLength`, 0.1 to 1.
    pub(crate) length: f32,
    /// `Workspace.GlobalWind`, in studs per second.
    pub(crate) wind: Vec3,
}

pub(crate) struct Terrain {
    pub(crate) referent: Ref,
    /// The `SmoothGrid` bytes `grid` was decoded from, so a write that left
    /// them alone (a colour edit) skips the decode and the diff.
    source: Vec<u8>,
    pub(crate) grid: VoxelGrid,
    pub(crate) colors: MaterialColors,
    pub(crate) water: Water,
    pub(crate) grass: Grass,
    /// The texture-array slot each solid material present draws with.
    slots: HashMap<Material, Slot>,
    /// Why the voxels could not be read, when they could not.
    pub(crate) error: Option<String>,
}

/// The `Terrain` directly under `Workspace`, the only one Roblox draws.
pub(crate) fn find(dom: &WeakDom) -> Option<Ref> {
    let workspace =
        super::descendants(dom).find(|&r| dom.get(r).is_some_and(|i| i.class() == "Workspace"))?;
    dom.get(workspace)?
        .children()
        .iter()
        .copied()
        .find(|&child| dom.get(child).is_some_and(|i| i.class() == TERRAIN_CLASS))
}

/// The bytes of a `BinaryString` property, whichever variant the reader
/// chose for it (valid UTF-8 comes back as a `String`).
pub(crate) fn blob<'a>(properties: &'a Properties, key: &str) -> Option<&'a [u8]> {
    match properties.get(key)? {
        Variant::String(text) => Some(text.as_bytes()),
        Variant::Unknown { raw, .. } => Some(raw),
        _ => None,
    }
}

impl Terrain {
    pub(crate) fn plan(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        catalog: &mut Catalog,
    ) -> Option<Terrain> {
        let referent = find(dom)?;
        let properties = dom.get(referent)?.properties();
        let source = blob(properties, "SmoothGrid").unwrap_or_default().to_vec();
        let (grid, error) = match blob(properties, "SmoothGrid") {
            None => (VoxelGrid::new(), None),
            Some(bytes) => match rbx_terrain::smooth_grid::decode(bytes) {
                Ok(grid) => (grid, None),
                Err(error) => (VoxelGrid::new(), Some(error.to_string())),
            },
        };
        let mut terrain = Terrain {
            referent,
            source,
            grid,
            colors: read_colors(properties),
            water: read_water(properties),
            grass: read_grass(properties, read_wind(dom)),
            slots: HashMap::new(),
            error,
        };
        terrain.claim_slots(catalog, database, None);
        Some(terrain)
    }

    /// Re-reads the terrain from its (edited) `properties`, answering which
    /// chunks need new meshes. A colour or water edit needs none.
    pub(crate) fn resync(&mut self, properties: &Properties) -> BTreeSet<ChunkKey> {
        self.colors = read_colors(properties);
        self.water = read_water(properties);
        self.grass = read_grass(properties, self.grass.wind);
        let bytes = blob(properties, "SmoothGrid").unwrap_or_default();
        if bytes == self.source.as_slice() {
            return BTreeSet::new();
        }
        self.source = bytes.to_vec();
        let (grid, error) = if bytes.is_empty() {
            (VoxelGrid::new(), None)
        } else {
            match rbx_terrain::smooth_grid::decode(bytes) {
                Ok(grid) => (grid, None),
                Err(error) => (VoxelGrid::new(), Some(error.to_string())),
            }
        };
        let changed = self.grid.changed_meshes(&grid);
        self.grid = grid;
        self.error = error;
        changed
    }

    /// Gives every solid material the grid holds a texture-array layer,
    /// through the same catalog parts use, so a `MaterialService` override
    /// of Grass repaints terrain grass too. Returns whether any layer was
    /// new to the catalog (which a running renderer cannot add on the fly).
    /// `within` limits the look to the chunks an edit changed; a material
    /// already claimed stays claimed, so only new ones matter.
    pub(crate) fn claim_slots(
        &mut self,
        catalog: &mut Catalog,
        database: &ReflectionDatabase,
        within: Option<&BTreeSet<ChunkKey>>,
    ) -> bool {
        let before = catalog.layers();
        let present = self.grid.materials_in(within);
        for material in
            Material::paintable().filter(|m| m.is_solid() && present[usize::from(m.slot())])
        {
            let properties =
                BTreeMap::from([("Material".to_string(), Variant::Enum(material.enum_value()))]);
            self.slots
                .insert(material, catalog.slot_for(&properties, database));
        }
        catalog.layers() > before
    }

    /// The slot `material` draws with right now (plastic until its pack
    /// arrives, see `Catalog::slot`).
    pub(crate) fn slot(&self, catalog: &Catalog, material: Material) -> Slot {
        let layer = self.slots.get(&material).map_or(0, |slot| slot.layer);
        catalog.slot(layer)
    }

    /// The slot water draws with: no texture pack (Roblox's water is
    /// procedural), its own shading kind.
    pub(crate) fn water_slot(&self) -> Slot {
        Slot {
            layer: 0,
            kind: Kind::Water,
            studs_per_tile: rbx_materials::DEFAULT_STUDS_PER_TILE,
        }
    }

    /// The linear tint `material` is drawn with.
    pub(crate) fn tint(&self, material: Material) -> [f32; 3] {
        self.colors
            .get(material)
            .map(|c| super::srgb_to_linear(f32::from(c) / 255.0))
    }

    /// The extent the stored chunks span, in studs: loose by up to a chunk,
    /// but read without walking every voxel of a large map.
    pub(crate) fn extent(&self) -> Option<Bounds> {
        let (min, max) = self.grid.chunk_bounds()?;
        if self.grid.is_empty() {
            return None;
        }
        Some(Bounds {
            min: Vec3::from(min.map(|v| v as f32 * VOXEL_STUDS)),
            max: Vec3::from(max.map(|v| v as f32 * VOXEL_STUDS)),
        })
    }

    pub(crate) fn has_water(&self) -> bool {
        self.grid
            .voxels()
            .any(|(_, cell)| cell.water_fraction() > 0.0)
    }
}

fn read_colors(properties: &Properties) -> MaterialColors {
    blob(properties, "MaterialColors")
        .and_then(MaterialColors::decode)
        .unwrap_or_default()
}

fn read_grass(properties: &Properties, wind: Vec3) -> Grass {
    Grass {
        // What a fresh place stores; a file without them predates grass.
        enabled: bool_or(properties, "Decoration", true),
        length: float_or(properties, "GrassLength", 0.7).clamp(0.1, 1.0),
        wind,
    }
}

/// `GlobalWind` lives on `Workspace`; an edit to it re-reads the terrain
/// (see `Patcher::present`), which with its voxels unchanged reads nothing
/// else.
pub(crate) fn read_wind(dom: &WeakDom) -> Vec3 {
    find(dom)
        .and_then(|terrain| dom.get(dom.parent(terrain)?))
        .map_or(Vec3::ZERO, |workspace| {
            vector3_or(workspace.properties(), "GlobalWind", Vec3::ZERO)
        })
}

fn read_water(properties: &Properties) -> Water {
    Water {
        // Roblox's defaults, as a fresh place stores them.
        color: linear_color_or(
            properties,
            "WaterColor",
            [12.0 / 255.0, 84.0 / 255.0, 92.0 / 255.0],
        ),
        transparency: float_or(properties, "WaterTransparency", 0.3).clamp(0.0, 1.0),
        reflectance: float_or(properties, "WaterReflectance", 1.0).clamp(0.0, 1.0),
        wave_size: float_or(properties, "WaterWaveSize", 0.15).clamp(0.0, 1.0),
        wave_speed: float_or(properties, "WaterWaveSpeed", 10.0).clamp(0.0, 100.0),
    }
}

impl super::Scene {
    /// Brings the terrain in line with `dom` after an edit to the `Terrain`
    /// instance, answering the chunks to re-mesh. A material the place had
    /// never drawn needs a pack only a reload fetches.
    pub(crate) fn resync_terrain(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        known_layers: usize,
    ) -> Result<BTreeSet<ChunkKey>, crate::changes::Rebuild> {
        let changed = match (find(dom), self.terrain.as_mut()) {
            (Some(referent), Some(terrain)) if terrain.referent == referent => {
                let properties = dom.get(referent).map(|i| i.properties());
                let changed = properties.map(|p| terrain.resync(p)).unwrap_or_default();
                terrain.grass.wind = read_wind(dom);
                terrain.claim_slots(&mut self.materials, database, Some(&changed));
                changed
            }
            (found, _) => {
                let mut changed = self
                    .terrain
                    .take()
                    .map(|old| rbx_terrain::mesh::meshable_chunks(&old.grid))
                    .unwrap_or_default();
                if found.is_some() {
                    self.terrain = Terrain::plan(dom, database, &mut self.materials);
                    if let Some(terrain) = &self.terrain {
                        changed.extend(rbx_terrain::mesh::meshable_chunks(&terrain.grid));
                    }
                }
                changed
            }
        };
        if self.materials.layers() > known_layers {
            return Err(crate::changes::Rebuild::Asset);
        }
        if !changed.is_empty() {
            self.extent_stale = true;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests;
