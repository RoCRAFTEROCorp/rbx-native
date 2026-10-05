//! Smooth terrain on the GPU: each chunk's surfaces (see
//! `rbx_terrain::mesh`) as vertex/index buffers, drawn through the untextured
//! file-mesh pipelines with one instance per material.
//!
//! The vertices are already in world studs, so every instance's model matrix
//! is the identity — which is also what makes the material packs' projection
//! (in object space, see `material.wgsl`) land in world space, continuous
//! from one chunk to the next. One instance per material, shared by every
//! chunk, carries that material's tint and texture layer; water's carries the
//! place's water look and draws in the translucent pass.

use std::collections::{BTreeSet, HashMap};

use glam::Vec3;
use rbx_terrain::mesh::{mesh_chunk, meshable_chunks, ChunkMesh, Surface};
use rbx_terrain::{ChunkKey, Material, CHUNK, VOXEL_STUDS};
use wgpu::util::DeviceExt;

use super::instance::InstanceRaw;
use super::mesh::Vertex;
use super::shadow::casters::MeshGeometry;
use crate::scene::terrain::Terrain;
use crate::scene::{Catalog, Slot};

const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];
/// One instance per material slot; water's own slot (1) holds the water.
const INSTANCES: usize = 23;
const STRIDE: u64 = std::mem::size_of::<InstanceRaw>() as u64;

struct Geometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct ChunkGpu {
    solids: Vec<(Material, Geometry)>,
    water: Option<Geometry>,
    /// Every solid surface merged, positions only: the shadow pass's.
    caster: Option<MeshGeometry>,
    center: Vec3,
}

pub(super) struct TerrainGpu {
    chunks: HashMap<ChunkKey, ChunkGpu>,
    instances: wgpu::Buffer,
    /// The identity, as the shadow pass's per-instance record.
    caster_instance: wgpu::Buffer,
    has_water: bool,
}

/// Half a chunk's diagonal: the sphere a chunk is culled against.
fn chunk_radius() -> f32 {
    (CHUNK as f32 * VOXEL_STUDS) * 0.5 * 3f32.sqrt()
}

impl TerrainGpu {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        terrain: Option<&Terrain>,
        catalog: &Catalog,
    ) -> Self {
        let mut gpu = TerrainGpu {
            chunks: HashMap::new(),
            instances: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview terrain instances"),
                size: STRIDE * INSTANCES as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            caster_instance: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview terrain caster"),
                contents: bytemuck::cast_slice(&IDENTITY),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            has_water: false,
        };
        if let Some(terrain) = terrain {
            let keys = meshable_chunks(&terrain.grid);
            gpu.remesh(device, terrain, &keys);
            gpu.write_instances(queue, terrain, catalog);
        }
        gpu
    }

    pub(super) fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub(super) fn has_water(&self) -> bool {
        self.has_water
    }

    /// Re-meshes `keys` and uploads the result, dropping chunks left empty.
    /// The meshing is spread over the machine's cores: a large map is
    /// thousands of independent chunks.
    pub(super) fn remesh(
        &mut self,
        device: &wgpu::Device,
        terrain: &Terrain,
        keys: &BTreeSet<ChunkKey>,
    ) {
        let keys: Vec<ChunkKey> = keys.iter().copied().collect();
        let meshes = mesh_parallel(terrain, &keys);
        for (key, mesh) in keys.into_iter().zip(meshes) {
            if mesh.is_empty() {
                self.chunks.remove(&key);
            } else {
                self.chunks.insert(key, upload(device, key, &mesh));
            }
        }
        self.has_water = self.chunks.values().any(|chunk| chunk.water.is_some());
    }

    /// Rewrites every material's tint and slot, and the water's look. Cheap:
    /// 23 records.
    pub(super) fn write_instances(
        &self,
        queue: &wgpu::Queue,
        terrain: &Terrain,
        catalog: &Catalog,
    ) {
        queue.write_buffer(
            &self.instances,
            0,
            bytemuck::cast_slice(&instances(terrain, catalog)),
        );
    }

    /// The opaque surfaces. Expects one of the untextured file-mesh
    /// pipelines bound, with the frame and material groups.
    pub(super) fn draw_opaque(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        visible: impl Fn(Vec3, f32) -> bool,
    ) {
        let radius = chunk_radius();
        for chunk in self.chunks.values().filter(|c| visible(c.center, radius)) {
            for (material, geometry) in &chunk.solids {
                let offset = u64::from(material.slot()) * STRIDE;
                pass.set_vertex_buffer(1, self.instances.slice(offset..offset + STRIDE));
                draw(pass, geometry);
            }
        }
    }

    /// The water surfaces, furthest chunk first so overlapping water blends
    /// in order. Expects the blended untextured pipeline bound.
    pub(super) fn draw_water(&self, pass: &mut wgpu::RenderPass<'_>, eye: Vec3) {
        let offset = u64::from(Material::Water.slot()) * STRIDE;
        pass.set_vertex_buffer(1, self.instances.slice(offset..offset + STRIDE));
        let mut wet: Vec<&ChunkGpu> = self.chunks.values().filter(|c| c.water.is_some()).collect();
        wet.sort_by(|a, b| {
            (b.center - eye)
                .length_squared()
                .total_cmp(&(a.center - eye).length_squared())
        });
        for chunk in wet {
            if let Some(geometry) = &chunk.water {
                draw(pass, geometry);
            }
        }
    }

    /// The solid surfaces for a depth-only shadow pass, whose mesh pipeline
    /// is already bound.
    pub(super) fn draw_casters(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(1, self.caster_instance.slice(..));
        for caster in self.chunks.values().filter_map(|c| c.caster.as_ref()) {
            pass.set_vertex_buffer(0, caster.vertices.slice(..));
            pass.set_index_buffer(caster.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..caster.index_count, 0, 0..1);
        }
    }
}

fn draw(pass: &mut wgpu::RenderPass<'_>, geometry: &Geometry) {
    pass.set_vertex_buffer(0, geometry.vertices.slice(..));
    pass.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..geometry.index_count, 0, 0..1);
}

fn instances(terrain: &Terrain, catalog: &Catalog) -> [InstanceRaw; INSTANCES] {
    std::array::from_fn(|slot| {
        let material = Material::from_slot(slot as u8).unwrap_or_default();
        if material == Material::Water {
            let water = terrain.water;
            return InstanceRaw::new(
                IDENTITY,
                water.color,
                1.0 - water.transparency,
                water.reflectance,
                water_slot(terrain.water_slot(), water.wave_size, water.wave_speed),
            );
        }
        InstanceRaw::new(
            IDENTITY,
            terrain.tint(material),
            1.0,
            0.0,
            terrain.slot(catalog, material),
        )
    })
}

/// Water has no pack to tile, so its studs-per-tile field carries the waves
/// instead: whole ripple cycles per shimmer period in the integer part
/// (from `WaterWaveSpeed`, so the loop stays seamless) and the wave height
/// (`WaterWaveSize`, 0 to 1) in the fraction. `material.wgsl`'s water branch
/// unpacks it.
fn water_slot(slot: Slot, size: f32, speed: f32) -> Slot {
    let cycles = (speed * 0.6).round().clamp(0.0, 60.0);
    Slot {
        studs_per_tile: cycles + size.clamp(0.0, 1.0) * 0.999,
        ..slot
    }
}

fn mesh_parallel(terrain: &Terrain, keys: &[ChunkKey]) -> Vec<ChunkMesh> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(keys.len().max(1));
    if threads <= 1 || keys.len() < 4 {
        return keys
            .iter()
            .map(|key| mesh_chunk(&terrain.grid, *key))
            .collect();
    }
    let per = keys.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = keys
            .chunks(per)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|key| mesh_chunk(&terrain.grid, *key))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

fn geometry(device: &wgpu::Device, surface: &Surface) -> Geometry {
    let vertices: Vec<Vertex> = surface
        .positions
        .iter()
        .zip(&surface.normals)
        .map(|(p, n)| Vertex::new(*p, *n))
        .collect();
    Geometry {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview terrain vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview terrain indices"),
            contents: bytemuck::cast_slice(&surface.indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: surface.indices.len() as u32,
    }
}

fn upload(device: &wgpu::Device, key: ChunkKey, mesh: &ChunkMesh) -> ChunkGpu {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (_, surface) in &mesh.solids {
        let base = positions.len() as u32;
        positions.extend_from_slice(&surface.positions);
        indices.extend(surface.indices.iter().map(|i| i + base));
    }
    let caster = (!indices.is_empty()).then(|| MeshGeometry {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview terrain caster vertices"),
            contents: bytemuck::cast_slice(&positions),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview terrain caster indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: indices.len() as u32,
    });
    let origin = Vec3::from(key.origin().map(|v| v as f32 * VOXEL_STUDS));
    ChunkGpu {
        solids: mesh
            .solids
            .iter()
            .map(|(material, surface)| (*material, geometry(device, surface)))
            .collect(),
        water: (!mesh.water.is_empty()).then(|| geometry(device, &mesh.water)),
        caster,
        center: origin + Vec3::splat(CHUNK as f32 * VOXEL_STUDS * 0.5),
    }
}
