//! Version 5, the layout Studio writes unions in now. After the plain magic
//! and version, every per-vertex array opens with its own `u16` vertex count
//! (all must agree), in this order:
//!
//! 1. positions, `3 × f32`;
//! 2. a `u32` byte length, then normals as `3 × u16`, where 0 is −1, 32767
//!    is 0 and 65534 is +1;
//! 3. RGBA colours;
//! 4. one byte per vertex, numbering the source face it came from (not read);
//! 5. texture coordinates, `2 × f32` (not read);
//! 6. a `u32` byte length, then tangents in the normals' encoding (not read).
//!
//! Then `u32` index count, `u32` byte length and the index stream: each index
//! is the previous one plus a delta, the first counted from 0. A byte below
//! 0x80 is a 7-bit two's-complement delta (0x7f is −1); a byte with its high
//! bit set opens a 3-byte big-endian word whose other 23 bits are the delta,
//! two's complement (`80 00 ba` is +186, `ff ff 47` is −185). Every blob seen
//! ends its stream exactly on its stated length, which the reader requires.
//! Last, a `u8` LOD count and that many `u32` offsets into the indices; only
//! the first LOD is kept.

use super::{f32s, first_lod, Decoded, Error, Reader};

/// The bit that marks an index delta as 3 bytes wide.
pub(super) const WIDE: u8 = 0x80;

pub(super) fn read(reader: &mut Reader) -> Result<Decoded, Error> {
    let count = reader.u16()? as usize;
    let positions = reader
        .items(count, 12)?
        .as_chunks::<12>()
        .0
        .iter()
        .map(|raw| f32s(raw))
        .collect();

    let normals = units(reader, count)?;
    expect(reader, count)?;
    let colors = reader.items(count, 4)?.as_chunks::<4>().0.to_vec();
    expect(reader, count)?;
    reader.items(count, 1)?;
    expect(reader, count)?;
    reader.items(count, 8)?;
    units(reader, count)?;

    let index_count = reader.u32()? as usize;
    let length = reader.u32()? as usize;
    let mut stream = Reader {
        bytes: reader.take(length)?,
        at: 0,
    };
    let mut indices = Vec::with_capacity(index_count.min(length));
    let mut current: i64 = 0;
    for _ in 0..index_count {
        let code = stream.u8()?;
        let delta = match code & WIDE {
            0 => i64::from(((code << 1) as i8) >> 1),
            _ => {
                let [high, low] = stream.array()?;
                let word = u32::from_be_bytes([0, code & !WIDE, high, low]);
                // Up to the top bit and back, to sign-extend 23 bits.
                i64::from(((word << 9) as i32) >> 9)
            }
        };
        current += delta;
        indices.push(u32::try_from(current).map_err(|_| Error::Index)?);
    }
    if stream.at != length {
        return Err(Error::Index);
    }

    let lods = reader.u8()?;
    let mut decoded = Decoded {
        positions,
        normals,
        colors,
        indices,
    };
    first_lod(reader, lods.into(), &mut decoded)?;
    Ok(decoded)
}

/// A `u16` count that must be the document's vertex count.
fn expect(reader: &mut Reader, count: usize) -> Result<(), Error> {
    match reader.u16()? as usize == count {
        true => Ok(()),
        false => Err(Error::Count),
    }
}

/// A counted, length-prefixed array of unit vectors in `u16` encoding.
fn units(reader: &mut Reader, count: usize) -> Result<Vec<[f32; 3]>, Error> {
    expect(reader, count)?;
    let length = reader.u32()? as usize;
    if length != count * 6 {
        return Err(Error::Count);
    }
    Ok(reader
        .items(count, 6)?
        .as_chunks::<6>()
        .0
        .iter()
        .map(|raw| {
            let axis = |i: usize| {
                let value = u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
                (f32::from(value) - 32767.0) / 32767.0
            };
            let vector = glam::Vec3::new(axis(0), axis(1), axis(2));
            vector.normalize_or_zero().to_array()
        })
        .collect())
}
