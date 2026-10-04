//! Roblox's own baked union mesh (`MeshData`/`MeshData2`, the CSGMDL
//! format), read only for a union whose operation tree is gone: no
//! `ChildData` inline and none in the `PartOperationAsset` its `AssetId`
//! names. Anything with a tree is still carved from it (`super::csg`).
//!
//! Nothing here comes from Roblox documentation, which does not describe the
//! format. It was worked out by reading real blobs (asset 305197512, and the
//! inline blobs of the places this project is tested against):
//!
//! - **Verified:** a stored blob is the plain document XORed with a 31-byte
//!   repeating [`KEY`]. Unscrambled it opens with `CSGMDL` and a `u32` version,
//!   so a wrong key cannot pass [`plain`] unnoticed.
//! - **Verified, version 2:** after the magic and version come a 16-character
//!   hex digest and 16 more digest bytes, then `u32` vertex count, `u32`
//!   vertex stride, the vertices, `u32` index count and the `u32` indices, with
//!   nothing after. Each vertex opens with position and normal (`3 × f32`
//!   each) and an RGBA colour; positions are in the union's own studs at its
//!   `InitialSize`, triangles wound counter-clockwise like every other mesh
//!   here.
//! - **Not read:** the rest of a vertex (what looks like texture coordinates
//!   and a tangent) — texture coordinates are box-projected the way
//!   `super::csg` does, so a decoded union tiles its material like a carved
//!   one. Version 5, which the same places also hold, is compressed past the
//!   XOR and not decoded; every version-5 union seen also carries its tree.

use glam::Vec3;

const MAGIC: &[u8; 6] = b"CSGMDL";
const KEY: [u8; 31] = [
    0x56, 0x2e, 0x6e, 0x58, 0x31, 0x20, 0x30, 0x04, 0x34, 0x69, 0x0c, 0x77, 0x0c, 0x01, 0x5e, 0x00,
    0x1a, 0x60, 0x37, 0x69, 0x1d, 0x52, 0x2b, 0x07, 0x4f, 0x24, 0x59, 0x65, 0x53, 0x04, 0x7a,
];
/// Magic, version and the two digests.
const HEADER: usize = 6 + 4 + 16 + 16;
/// Position, normal and colour: all a vertex must hold to be read.
const MIN_STRIDE: usize = 28;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    /// Neither the bytes nor their unscrambled form open with `CSGMDL`.
    NotCsg,
    /// A version this module does not read (see the module doc).
    Version(u32),
    /// The document ends before what its own counts promise.
    Truncated,
    /// A vertex too small to hold a position, normal and colour.
    Stride(u32),
    /// An index past the last vertex, or a count that is not whole triangles.
    Index,
}

/// The unscrambled document, `bytes` itself when it is already plain.
fn plain(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    if bytes.starts_with(MAGIC) {
        return Ok(bytes.to_vec());
    }
    let out: Vec<u8> = bytes
        .iter()
        .zip(KEY.iter().cycle())
        .map(|(byte, key)| byte ^ key)
        .collect();
    match out.starts_with(MAGIC) {
        true => Ok(out),
        false => Err(Error::NotCsg),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8], Error> {
        let end = self.at.checked_add(len).ok_or(Error::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(Error::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let raw = self.take(4)?;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }
}

fn f32s<const N: usize>(raw: &[u8]) -> [f32; N] {
    std::array::from_fn(|i| {
        f32::from_le_bytes([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]])
    })
}

/// A decoded union mesh and the colour most of its surface was baked in —
/// what paints it where `UsePartColor` is off, as the biggest leaf's colour
/// does for a carved one.
pub(super) struct Baked {
    pub(super) mesh: rbx_mesh::Mesh,
    pub(super) color: Option<[u8; 3]>,
}

/// Decodes one `MeshData` blob, scrambled or not.
pub(super) fn decode(bytes: &[u8]) -> Result<Baked, Error> {
    let document = plain(bytes)?;
    let mut reader = Reader {
        bytes: &document,
        at: MAGIC.len(),
    };
    match reader.u32()? {
        2 => {}
        version => return Err(Error::Version(version)),
    }
    reader.at = HEADER;
    let count = reader.u32()? as usize;
    let stride = reader.u32()?;
    if (stride as usize) < MIN_STRIDE {
        return Err(Error::Stride(stride));
    }
    let raw = reader.take(count.checked_mul(stride as usize).ok_or(Error::Truncated)?)?;
    let mut vertices = Vec::with_capacity(count);
    let mut bounds = rbx_mesh::Aabb {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };
    for vertex in raw.chunks_exact(stride as usize) {
        let position: [f32; 3] = f32s(&vertex[0..12]);
        let normal: [f32; 3] = f32s(&vertex[12..24]);
        bounds.min = std::array::from_fn(|axis| bounds.min[axis].min(position[axis]));
        bounds.max = std::array::from_fn(|axis| bounds.max[axis].max(position[axis]));
        let (u, v) = super::csg::dominant_axes(normal);
        vertices.push(rbx_mesh::Vertex {
            position,
            normal,
            uv: [
                position[u] / rbx_materials::DEFAULT_STUDS_PER_TILE,
                position[v] / rbx_materials::DEFAULT_STUDS_PER_TILE,
            ],
            color: [vertex[24], vertex[25], vertex[26], vertex[27]],
        });
    }

    let index_count = reader.u32()? as usize;
    let raw = reader.take(index_count.checked_mul(4).ok_or(Error::Truncated)?)?;
    let indices: Vec<u32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i as usize >= count) {
        return Err(Error::Index);
    }

    let color = dominant_color(&vertices, &indices);
    // The renderer multiplies vertex colour in; the union's own colour is
    // the instance's, so the baked tint must not darken it a second time.
    for vertex in &mut vertices {
        vertex.color = [255; 4];
    }
    Ok(Baked {
        mesh: rbx_mesh::Mesh {
            // A CSGMDL version, not a `.mesh` header one: there is none to echo.
            version: (0, 0),
            vertices,
            indices,
            lods: Vec::new(),
            bounds,
        },
        color,
    })
}

/// The vertex colour covering the most triangle area, read off each
/// triangle's first corner (a union bakes one colour per source part, so a
/// triangle never straddles two).
fn dominant_color(vertices: &[rbx_mesh::Vertex], indices: &[u32]) -> Option<[u8; 3]> {
    let mut areas: Vec<([u8; 3], f32)> = Vec::new();
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = [0, 1, 2].map(|i| &vertices[triangle[i] as usize]);
        let [pa, pb, pc] = [a, b, c].map(|v| Vec3::from(v.position));
        let area = (pb - pa).cross(pc - pa).length();
        let color = [a.color[0], a.color[1], a.color[2]];
        match areas.iter_mut().find(|(known, _)| *known == color) {
            Some((_, total)) => *total += area,
            None => areas.push((color, area)),
        }
    }
    areas
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(color, _)| color)
}

/// The `MeshData`/`MeshData2` a union or `PartOperationAsset` carries,
/// whichever is filled.
pub(super) fn mesh_data(
    properties: &std::collections::BTreeMap<String, rbx_dom::Variant>,
) -> Option<&[u8]> {
    ["MeshData2", "MeshData"]
        .iter()
        .find_map(|key| match properties.get(*key) {
            // Never valid UTF-8, so always `Unknown` — see `tree::child_data`.
            Some(rbx_dom::Variant::Unknown { raw, .. }) if !raw.is_empty() => Some(raw.as_slice()),
            _ => None,
        })
}

/// The baked mesh of a union whose `bytes` are either a downloaded
/// `PartOperationAsset` document or the union's own inline `MeshData`.
pub(super) fn of_bytes(bytes: &[u8]) -> Option<Baked> {
    if let Ok(baked) = decode(bytes) {
        return Some(baked);
    }
    let dom = rbx_binary::deserialize(bytes).ok()?;
    let root = dom.get(*dom.root_refs().first()?)?;
    decode(mesh_data(root.properties())?).ok()
}

#[cfg(test)]
#[path = "baked/tests.rs"]
mod tests;
