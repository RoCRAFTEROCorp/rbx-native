
// Smooth terrain's solid surface, concatenated after `material.wgsl` (bind
// groups 0 and 1, `MaterialInput`, `material_mapped`, `mapped_shade`).
//
// Each vertex names up to three materials and its weight for each (see
// `rbx_terrain::mesh::Blend`); all three are projected and sampled, mixed
// by the weights, and shaded once. Roblox does not publish how it blends
// terrain materials; this follows the common height-blend technique, where
// the brighter texture wins near the border, which keeps the transition
// narrow and ragged rather than a wide linear smear across the 4-stud cell.

struct TerrainMaterial {
    // Linear tint in xyz, studs per tile in w.
    tint: vec4<f32>,
    // Texture-array layer in x, shading kind in y.
    slot: vec4<u32>,
}

// One entry per terrain material slot, so a colour edit rewrites this table
// and never the chunks' vertices.
@group(2) @binding(0) var<uniform> terrain_materials: array<TerrainMaterial, 23>;

struct TerrainVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) materials: vec4<u32>,
    @location(3) weights: vec3<f32>,
}

struct TerrainOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // The same on all three corners: a triangle never mixes blend lists.
    @location(2) @interpolate(flat) materials: vec3<u32>,
    @location(3) weights: vec3<f32>,
}

// How far a texture's brightness can push its share up or down (0 to 1).
const TERRAIN_HEIGHT_INFLUENCE: f32 = 0.5;
// The band, in weight, over which two materials cross-fade. Weights change
// by about 1/8 per stud across a border cell, so this is about a stud.
const TERRAIN_BLEND_DEPTH: f32 = 0.12;

@vertex
fn vs_main(vertex: TerrainVertex) -> TerrainOut {
    var out: TerrainOut;
    out.clip_position = uniforms.view_projection * vec4<f32>(vertex.position, 1.0);
    out.world_position = vertex.position;
    out.normal = vertex.normal;
    out.materials = vertex.materials.xyz;
    out.weights = vertex.weights;
    return out;
}

/// One of the vertex's materials as a surface input. The mesh is in world
/// studs, so world space is object space: the projection runs continuously
/// from chunk to chunk.
fn terrain_input(in: TerrainOut, slot: u32) -> MaterialInput {
    let material = terrain_materials[slot];
    var input: MaterialInput;
    input.object_studs = in.world_position;
    input.object_normal = normalize(in.normal);
    input.rotation = mat3x3<f32>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
    );
    input.world_normal = in.normal;
    input.world_position = in.world_position;
    input.albedo = material.tint.rgb;
    input.reflectance = 0.0;
    input.layer = material.slot.x;
    input.studs_per_tile = material.tint.w;
    input.kind = material.slot.y;
    return input;
}

/// The colour map's brightness, independent of the tint.
fn terrain_height(mapped: Mapped) -> f32 {
    let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
    return clamp(dot(mapped.albedo, luma) / max(dot(mapped.base_albedo, luma), 0.001), 0.0, 1.0);
}

/// Narrows the interpolated weights to a height-driven border: each share is
/// scaled by its texture's brightness, and only shares within
/// `TERRAIN_BLEND_DEPTH` of the strongest survive.
fn terrain_sharpen(weights: vec3<f32>, heights: vec3<f32>) -> vec3<f32> {
    let scaled = weights
        * mix(vec3<f32>(1.0 - TERRAIN_HEIGHT_INFLUENCE), vec3<f32>(1.0 + TERRAIN_HEIGHT_INFLUENCE), heights);
    let top = max(scaled.x, max(scaled.y, scaled.z));
    let kept = max(scaled - vec3<f32>(top - TERRAIN_BLEND_DEPTH), vec3<f32>(0.0));
    return kept / max(kept.x + kept.y + kept.z, 0.0001);
}

@fragment
fn fs_main(in: TerrainOut) -> @location(0) vec4<f32> {
    // All three sampled whatever their weight: the samples must stay in
    // uniform control flow. An unused entry is Air's, at weight 0.
    let a = material_mapped(terrain_input(in, in.materials.x));
    let b = material_mapped(terrain_input(in, in.materials.y));
    let c = material_mapped(terrain_input(in, in.materials.z));
    let w = terrain_sharpen(
        in.weights,
        vec3<f32>(terrain_height(a), terrain_height(b), terrain_height(c)),
    );

    var mapped = a;
    mapped.base_albedo = a.base_albedo * w.x + b.base_albedo * w.y + c.base_albedo * w.z;
    mapped.albedo = a.albedo * w.x + b.albedo * w.y + c.albedo * w.z;
    mapped.normal = normalize(a.normal * w.x + b.normal * w.y + c.normal * w.z);
    mapped.metalness = a.metalness * w.x + b.metalness * w.y + c.metalness * w.z;
    mapped.roughness = a.roughness * w.x + b.roughness * w.y + c.roughness * w.z;
    // One shading model per fragment: the strongest material's (only
    // Plastic and Textured occur, Plastic while a pack is still loading).
    if w.y > w.x && w.y >= w.z {
        mapped.kind = b.kind;
    } else if w.z > w.x && w.z > w.y {
        mapped.kind = c.kind;
    }
    return vec4<f32>(mapped_shade(mapped), 1.0);
}
