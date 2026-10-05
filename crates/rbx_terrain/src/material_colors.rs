//! `Terrain.MaterialColors`: the tint each solid terrain material is drawn
//! with, 23 RGB triples in material slot order (rbx-dom's
//! `docs/binary-strings.md`). The first two, Air and Water, are always zero:
//! water takes `Terrain.WaterColor` instead.

use crate::Material;

/// One colour per material slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialColors(pub [[u8; 3]; 23]);

/// What a place made by current Studio stores: every fixture place and the
/// real terrain place agree on these bytes. (Older places carry a different
/// set, the colormap key's colours, and keep it, since they are read rather
/// than assumed.)
const CURRENT_DEFAULTS: [[u8; 3]; 23] = [
    [0, 0, 0],
    [0, 0, 0],
    [111, 126, 62],
    [88, 89, 86],
    [152, 152, 152],
    [138, 97, 73],
    [207, 203, 167],
    [172, 148, 108],
    [99, 100, 102],
    [221, 228, 229],
    [235, 253, 255],
    [148, 124, 95],
    [121, 112, 98],
    [75, 74, 74],
    [140, 130, 104],
    [255, 24, 67],
    [80, 84, 84],
    [134, 134, 118],
    [204, 210, 223],
    [106, 134, 64],
    [255, 255, 254],
    [255, 243, 192],
    [143, 144, 135],
];

impl Default for MaterialColors {
    fn default() -> Self {
        MaterialColors(CURRENT_DEFAULTS)
    }
}

impl MaterialColors {
    /// `None` unless the blob is exactly 69 bytes.
    pub fn decode(bytes: &[u8]) -> Option<MaterialColors> {
        if bytes.len() != 69 {
            return None;
        }
        Some(MaterialColors(std::array::from_fn(|slot| {
            [bytes[slot * 3], bytes[slot * 3 + 1], bytes[slot * 3 + 2]]
        })))
    }

    pub fn encode(&self) -> Vec<u8> {
        self.0.iter().flatten().copied().collect()
    }

    pub fn get(&self, material: Material) -> [u8; 3] {
        self.0[usize::from(material.slot())]
    }

    pub fn set(&mut self, material: Material, color: [u8; 3]) {
        if material.is_solid() {
            self.0[usize::from(material.slot())] = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_other_lengths() {
        let colors = MaterialColors::default();
        let bytes = colors.encode();
        assert_eq!(bytes.len(), 69);
        assert_eq!(&bytes[..6], &[0; 6]);
        assert_eq!(MaterialColors::decode(&bytes), Some(colors));
        assert_eq!(MaterialColors::decode(&bytes[..68]), None);
    }

    #[test]
    fn air_and_water_stay_zero() {
        let mut colors = MaterialColors::default();
        colors.set(Material::Water, [1, 2, 3]);
        assert_eq!(colors.get(Material::Water), [0, 0, 0]);
        colors.set(Material::Snow, [1, 2, 3]);
        assert_eq!(colors.get(Material::Snow), [1, 2, 3]);
        assert_eq!(
            MaterialColors::default().get(Material::Grass),
            [111, 126, 62]
        );
    }
}
