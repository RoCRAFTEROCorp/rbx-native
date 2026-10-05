//! The terrain's own pipeline (`terrain.wgsl`): its vertex format, which
//! carries each vertex's material blend, and the per-material table the
//! shader looks the blend's materials up in.

use bytemuck::{Pod, Zeroable};
use rbx_terrain::mesh::Blend;
use rbx_terrain::Material;

use super::super::pipeline::{self, Surface, Target, TERRAIN_SHADER};
use crate::scene::Slot;

/// One entry per terrain material slot.
pub(super) const MATERIALS: usize = Material::ALL.len();

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct TerrainVertex {
    position: [f32; 3],
    normal: [f32; 3],
    /// Material slots, indices into the table; the fourth byte is padding.
    materials: [u8; 4],
    weights: [f32; 3],
}

impl TerrainVertex {
    pub(super) fn new(position: [f32; 3], normal: [f32; 3], blend: &Blend) -> Self {
        let [a, b, c] = blend.materials.map(Material::slot);
        TerrainVertex {
            position,
            normal,
            materials: [a, b, c, 0],
            weights: blend.weights,
        }
    }
}

const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Uint8x4,
    3 => Float32x3,
];

/// `TerrainMaterial` in `terrain.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(super) struct MaterialRaw {
    tint: [f32; 4],
    slot: [u32; 4],
}

impl MaterialRaw {
    pub(super) fn new(tint: [f32; 3], slot: Slot) -> Self {
        MaterialRaw {
            tint: [tint[0], tint[1], tint[2], slot.studs_per_tile],
            slot: [slot.layer, slot.kind as u32, 0, 0],
        }
    }
}

pub(super) fn table_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview terrain materials"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

pub(super) fn build(
    device: &wgpu::Device,
    target: Target,
    (frame, materials, table): (
        &wgpu::BindGroupLayout,
        &wgpu::BindGroupLayout,
        &wgpu::BindGroupLayout,
    ),
) -> wgpu::RenderPipeline {
    let layouts = [Some(frame), Some(materials), Some(table)];
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    })];
    // Culled: the mesher winds every triangle out of the solid side.
    pipeline::surface(
        device,
        target,
        &Surface::new("rbxview terrain", TERRAIN_SHADER, &layouts, &buffers),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing ties the WGSL structs to these, so their sizes are pinned here:
    // a uniform array's stride and the vertex stride must match byte for byte.
    #[test]
    fn the_gpu_records_have_the_sizes_the_shader_reads() {
        assert_eq!(std::mem::size_of::<MaterialRaw>(), 32);
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 40);
        assert!(TERRAIN_SHADER.contains("array<TerrainMaterial, 23>"));
        assert_eq!(MATERIALS, 23);
    }
}
