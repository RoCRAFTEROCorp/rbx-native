//! Version 2, the layout asset 305197512 (2015) was baked in. After the
//! magic and version: a 16-character hex digest and 16 more digest bytes,
//! then `u32` vertex count, `u32` vertex stride, the vertices, `u32` index
//! count and the `u32` indices, with nothing after. Each vertex opens with
//! position and normal (`3 × f32` each) and an RGBA colour; the rest of its
//! stride (texture coordinates and what looks like a tangent) is not read.

use super::{f32s, first_lod, Decoded, Error, Reader};

/// The two digests after the magic and version.
const DIGESTS: usize = 16 + 16;
/// Position, normal and colour: all a vertex must hold to be read.
const MIN_STRIDE: usize = 28;

pub(super) fn read(reader: &mut Reader) -> Result<Decoded, Error> {
    reader.take(DIGESTS)?;
    let count = reader.u32()? as usize;
    let stride = reader.u32()?;
    if (stride as usize) < MIN_STRIDE {
        return Err(Error::Stride(stride));
    }
    let raw = reader.items(count, stride as usize)?;
    let mut decoded = Decoded {
        positions: Vec::with_capacity(count),
        normals: Vec::with_capacity(count),
        colors: Vec::with_capacity(count),
        indices: Vec::new(),
    };
    for vertex in raw.chunks_exact(stride as usize) {
        decoded.positions.push(f32s(&vertex[0..12]));
        decoded.normals.push(f32s(&vertex[12..24]));
        decoded
            .colors
            .push([vertex[24], vertex[25], vertex[26], vertex[27]]);
    }

    let index_count = reader.u32()? as usize;
    decoded.indices = reader
        .items(index_count, 4)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    Ok(decoded)
}

/// Version 4: version 2's layout, then a `u32` count and that many `u32`
/// offsets into the indices, the last being their end. Each span is a whole
/// closed mesh, the first the finest; only it is kept.
pub(super) fn read_v4(reader: &mut Reader) -> Result<Decoded, Error> {
    let mut decoded = read(reader)?;
    let lods = reader.u32()? as usize;
    first_lod(reader, lods, &mut decoded)?;
    Ok(decoded)
}
