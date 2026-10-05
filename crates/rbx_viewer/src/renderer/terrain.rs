//! Smooth terrain on the GPU: each chunk's surfaces (see
//! `rbx_terrain::mesh`) as vertex/index buffers.
//!
//! The solid surface draws through its own pipeline (see [`pipeline`]):
//! every vertex names the materials it blends and their weights, and the
//! shader looks each one's tint and texture layer up in a 23-entry table, so
//! a colour edit rewrites the table and no chunk. The vertices are in world
//! studs, which makes the material packs' projection continuous from one
//! chunk to the next. Water draws through the untextured file-mesh pipeline,
//! with one instance carrying the place's water look, in the translucent
//! pass.

mod pipeline;

use std::collections::{BTreeSet, HashMap};

use glam::Vec3;
use rbx_terrain::mesh::{mesh_chunk, meshable_chunks, ChunkMesh};
use rbx_terrain::{ChunkKey, Material, CHUNK, VOXEL_STUDS};
use wgpu::util::DeviceExt;

use super::instance::InstanceRaw;
use super::mesh::Vertex;
use super::pipeline::{Bindings, Target};
use super::shadow::casters::MeshGeometry;
use crate::scene::terrain::Terrain;
use crate::scene::{Catalog, Slot};
use pipeline::{MaterialRaw, TerrainVertex, MATERIALS};

const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

struct Geometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct ChunkGpu {
    solid: Option<Geometry>,
    water: Option<Geometry>,
    /// The solid surface's positions, with its indices: the shadow pass's.
    caster: Option<MeshGeometry>,
    center: Vec3,
}

pub(super) struct TerrainGpu {
    chunks: HashMap<ChunkKey, ChunkGpu>,
    pipeline: wgpu::RenderPipeline,
    /// Every material's tint and slot, one `MaterialRaw` per slot.
    table: wgpu::Buffer,
    table_group: wgpu::BindGroup,
    water_instance: wgpu::Buffer,
    /// The identity, as the shadow pass's per-instance record.
    caster_instance: wgpu::Buffer,
    has_water: bool,
}

/// Half a chunk's diagonal: the sphere a chunk is culled against.
fn chunk_radius() -> f32 {
    (CHUNK as f32 * VOXEL_STUDS) * 0.5 * 3f32.sqrt()
}

impl TerrainGpu {
    /// Builds the pipeline, once per renderer: [`TerrainGpu::replace`] swaps
    /// the place under it.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (target, frame_layout, material_layout): (
            Target,
            &wgpu::BindGroupLayout,
            &wgpu::BindGroupLayout,
        ),
        terrain: Option<&Terrain>,
        catalog: &Catalog,
    ) -> Self {
        let table_layout = pipeline::table_layout(device);
        let table = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview terrain materials"),
            size: (std::mem::size_of::<MaterialRaw>() * MATERIALS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut gpu = TerrainGpu {
            chunks: HashMap::new(),
            pipeline: pipeline::build(
                device,
                target,
                (frame_layout, material_layout, &table_layout),
            ),
            table_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rbxview terrain materials"),
                layout: &table_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: table.as_entire_binding(),
                }],
            }),
            table,
            water_instance: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview terrain water"),
                size: std::mem::size_of::<InstanceRaw>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            caster_instance: buffer(
                device,
                "rbxview terrain caster",
                &IDENTITY,
                wgpu::BufferUsages::VERTEX,
            ),
            has_water: false,
        };
        gpu.replace(device, queue, terrain, catalog);
        gpu
    }

    /// Drops every chunk and meshes `terrain` from scratch.
    pub(super) fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        terrain: Option<&Terrain>,
        catalog: &Catalog,
    ) {
        self.chunks.clear();
        self.has_water = false;
        if let Some(terrain) = terrain {
            let keys = meshable_chunks(&terrain.grid);
            self.remesh(device, terrain, &keys);
            self.write_instances(queue, terrain, catalog);
        }
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
    /// 24 small records.
    pub(super) fn write_instances(
        &self,
        queue: &wgpu::Queue,
        terrain: &Terrain,
        catalog: &Catalog,
    ) {
        let table: [MaterialRaw; MATERIALS] = std::array::from_fn(|slot| {
            let material = Material::ALL[slot];
            MaterialRaw::new(terrain.tint(material), terrain.slot(catalog, material))
        });
        queue.write_buffer(&self.table, 0, bytemuck::cast_slice(&table));
        queue.write_buffer(
            &self.water_instance,
            0,
            bytemuck::bytes_of(&water_instance(terrain)),
        );
    }

    /// The solid surfaces, through the terrain pipeline, which this binds
    /// along with every group it reads.
    pub(super) fn draw_opaque(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bindings: Bindings<'_>,
        visible: impl Fn(Vec3, f32) -> bool,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bindings.frame, &[]);
        pass.set_bind_group(1, bindings.materials, &[]);
        pass.set_bind_group(2, &self.table_group, &[]);
        let radius = chunk_radius();
        for chunk in self.chunks.values().filter(|c| visible(c.center, radius)) {
            if let Some(geometry) = &chunk.solid {
                draw(pass, geometry);
            }
        }
    }

    /// The water surfaces, furthest chunk first so overlapping water blends
    /// in order. Expects the blended untextured pipeline bound.
    pub(super) fn draw_water(&self, pass: &mut wgpu::RenderPass<'_>, eye: Vec3) {
        pass.set_vertex_buffer(1, self.water_instance.slice(..));
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

fn water_instance(terrain: &Terrain) -> InstanceRaw {
    let water = terrain.water;
    InstanceRaw::new(
        IDENTITY,
        water.color,
        1.0 - water.transparency,
        water.reflectance,
        water_slot(terrain.water_slot(), water.wave_size, water.wave_speed),
    )
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

fn buffer<T: bytemuck::Pod>(
    device: &wgpu::Device,
    label: &str,
    contents: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(contents),
        usage,
    })
}

fn geometry<T: bytemuck::Pod>(device: &wgpu::Device, vertices: &[T], indices: &[u32]) -> Geometry {
    Geometry {
        vertices: buffer(
            device,
            "rbxview terrain vertices",
            vertices,
            wgpu::BufferUsages::VERTEX,
        ),
        indices: buffer(
            device,
            "rbxview terrain indices",
            indices,
            wgpu::BufferUsages::INDEX,
        ),
        index_count: indices.len() as u32,
    }
}

fn upload(device: &wgpu::Device, key: ChunkKey, mesh: &ChunkMesh) -> ChunkGpu {
    let surface = &mesh.solid;
    let solid = (!surface.is_empty()).then(|| {
        let vertices: Vec<TerrainVertex> = surface
            .positions
            .iter()
            .zip(&surface.normals)
            .zip(&mesh.blends)
            .map(|((p, n), blend)| TerrainVertex::new(*p, *n, blend))
            .collect();
        geometry(device, &vertices, &surface.indices)
    });
    // The same triangles, positions only; the index buffer is shared.
    let caster = solid.as_ref().map(|solid| MeshGeometry {
        vertices: buffer(
            device,
            "rbxview terrain caster vertices",
            &surface.positions,
            wgpu::BufferUsages::VERTEX,
        ),
        indices: solid.indices.clone(),
        index_count: solid.index_count,
    });
    let water = (!mesh.water.is_empty()).then(|| {
        let vertices: Vec<Vertex> = mesh
            .water
            .positions
            .iter()
            .zip(&mesh.water.normals)
            .map(|(p, n)| Vertex::new(*p, *n))
            .collect();
        geometry(device, &vertices, &mesh.water.indices)
    });
    let origin = Vec3::from(key.origin().map(|v| v as f32 * VOXEL_STUDS));
    ChunkGpu {
        solid,
        water,
        caster,
        center: origin + Vec3::splat(CHUNK as f32 * VOXEL_STUDS * 0.5),
    }
}
