//! glTF 2.0 as a single `.gltf` file: the JSON document with its one binary
//! buffer inline as a base64 data URI, so the export is one file to move
//! around rather than a `.gltf` and a `.bin` that break apart.
//!
//! One node, mesh and material per part. Vertices are already in world space
//! (see [`super::meshes_of`]), so no node carries a transform.

use base64::Engine as _;
use serde_json::{json, Value};

use super::ExportMesh;

// glTF's enumerations, from the 2.0 specification.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const TRIANGLES: u32 = 4;

pub fn gltf(meshes: &[ExportMesh]) -> String {
    let mut buffer: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    // Every element below is four bytes wide, so each view starts aligned
    // without padding, as the specification requires of an accessor.
    let mut view = |buffer: &mut Vec<u8>, bytes: &[u8], target: u32| {
        views.push(json!({
            "buffer": 0,
            "byteOffset": buffer.len(),
            "byteLength": bytes.len(),
            "target": target,
        }));
        buffer.extend_from_slice(bytes);
        views.len() - 1
    };

    let mut primitives = Vec::new();
    for mesh in meshes {
        let positions: Vec<u8> = mesh
            .positions
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let normals: Vec<u8> = mesh
            .normals
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let indices: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let (min, max) = bounds(&mesh.positions);

        let base = accessors.len();
        accessors.push(json!({
            "bufferView": view(&mut buffer, &positions, ARRAY_BUFFER),
            "componentType": FLOAT,
            "count": mesh.positions.len(),
            "type": "VEC3",
            // Required on POSITION by the specification.
            "min": min,
            "max": max,
        }));
        accessors.push(json!({
            "bufferView": view(&mut buffer, &normals, ARRAY_BUFFER),
            "componentType": FLOAT,
            "count": mesh.normals.len(),
            "type": "VEC3",
        }));
        accessors.push(json!({
            "bufferView": view(&mut buffer, &indices, ELEMENT_ARRAY_BUFFER),
            "componentType": UNSIGNED_INT,
            "count": mesh.indices.len(),
            "type": "SCALAR",
        }));
        primitives.push(base);
    }

    let nodes: Vec<Value> = meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| json!({ "name": mesh.name, "mesh": index }))
        .collect();
    let gltf_meshes: Vec<Value> = meshes
        .iter()
        .zip(&primitives)
        .enumerate()
        .map(|(index, (mesh, &base))| {
            json!({
                "name": mesh.name,
                "primitives": [{
                    "attributes": { "POSITION": base, "NORMAL": base + 1 },
                    "indices": base + 2,
                    "material": index,
                    "mode": TRIANGLES,
                }],
            })
        })
        .collect();
    let materials: Vec<Value> = meshes.iter().map(material).collect();

    let document = json!({
        "asset": { "version": "2.0", "generator": "rbxstudio" },
        "scene": 0,
        "scenes": [{ "nodes": (0..meshes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{
            "byteLength": buffer.len(),
            "uri": format!(
                "data:application/octet-stream;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&buffer)
            ),
        }],
    });
    // `json!` cannot fail to serialize: every key is a string and every
    // number a finite one (a NaN would have become `null`).
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

/// The part's own colour, matte and non-metallic: the closest a plain PBR
/// material gets to a `Plastic` part without its material texture.
fn material(mesh: &ExportMesh) -> Value {
    let mut material = json!({
        "name": mesh.name,
        "pbrMetallicRoughness": {
            "baseColorFactor": mesh.color,
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0,
        },
    });
    if mesh.color[3] < 1.0 {
        material["alphaMode"] = json!("BLEND");
    }
    material
}

fn bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    positions.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(min, max), p| {
            (
                [min[0].min(p[0]), min[1].min(p[1]), min[2].min(p[2])],
                [max[0].max(p[0]), max[1].max(p[1]), max[2].max(p[2])],
            )
        },
    )
}
