//! One 4×4×4-stud voxel: a material, how full it is, and any water sharing it.

use crate::Material;

/// A voxel as `SmoothGrid` stores it.
///
/// `occupancy` is the raw byte: a non-Air voxel is `(occupancy + 1) / 256`
/// full, so even byte 0 holds a sliver and a full voxel is 255. Air always
/// stores 0. Roblox's own reader forces both, and [`Cell::new`] does too.
///
/// `liquid` is the extra byte the format carries for a partly filled solid
/// voxel (a run record with its run flag set and a count of zero is followed
/// by one more byte). Its placement in the stream is established; that it is
/// the voxel's water share (`LiquidOccupancy` in `ReadVoxelChannels`, which
/// the docs say is nonzero only beside a solid that is not full) is inferred
/// from where Roblox keeps it, not proven. It is preserved byte for byte and
/// drawn as water filling the rest of the voxel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cell {
    pub material: Material,
    pub occupancy: u8,
    pub liquid: u8,
}

pub const FULL: u8 = 255;

impl Cell {
    pub const AIR: Cell = Cell {
        material: Material::Air,
        occupancy: 0,
        liquid: 0,
    };

    /// Applies the invariants Roblox's reader enforces: Air is empty and holds
    /// no liquid, and only a partly filled solid voxel can share with water.
    pub fn new(material: Material, occupancy: u8, liquid: u8) -> Cell {
        if material == Material::Air {
            return Cell::AIR;
        }
        let liquid = if material.is_solid() && occupancy != FULL {
            liquid
        } else {
            0
        };
        Cell {
            material,
            occupancy,
            liquid,
        }
    }

    pub fn full(material: Material) -> Cell {
        Cell::new(material, FULL, 0)
    }

    /// A voxel `fraction` full of `material`. Zero (or less) is Air, since no
    /// stored byte means empty for a non-Air material.
    pub fn with_fraction(material: Material, fraction: f32) -> Cell {
        if material == Material::Air || fraction.is_nan() || fraction <= 0.0 {
            return Cell::AIR;
        }
        Cell::new(material, quantize(fraction), 0)
    }

    pub fn is_air(self) -> bool {
        self.material == Material::Air
    }

    /// How full of its own material the voxel is, 0 to 1.
    pub fn fraction(self) -> f32 {
        if self.is_air() {
            0.0
        } else {
            (f32::from(self.occupancy) + 1.0) / 256.0
        }
    }

    /// How full of solid (not water) the voxel is.
    pub fn solid_fraction(self) -> f32 {
        if self.material.is_solid() {
            self.fraction()
        } else {
            0.0
        }
    }

    /// How full of water the voxel is: a water voxel's own fill, or the
    /// shared water in a partly filled solid one.
    pub fn water_fraction(self) -> f32 {
        match self.material {
            Material::Water => self.fraction(),
            _ if self.liquid != 0 => {
                (f32::from(self.liquid) / 255.0).min(1.0 - self.solid_fraction())
            }
            _ => 0.0,
        }
    }
}

/// Roblox's own fraction-to-byte rule: `trunc(f * 256 - 0.5)`, clamped. It
/// is the inverse of `(b + 1) / 256` up to rounding, so a fraction read back
/// from a byte re-quantizes to the same byte (0.5 → 127, 1 → 255).
pub fn quantize(fraction: f32) -> u8 {
    (fraction * 256.0 - 0.5).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_inverts_fraction_for_every_byte() {
        for byte in 0..=255u8 {
            let cell = Cell::new(Material::Rock, byte, 0);
            assert_eq!(quantize(cell.fraction()), byte);
        }
        assert_eq!(quantize(0.5), 127);
        assert_eq!(quantize(1.0), 255);
        assert_eq!(quantize(2.0), 255);
    }

    #[test]
    fn air_and_full_cells_drop_liquid() {
        assert_eq!(Cell::new(Material::Air, 200, 9), Cell::AIR);
        assert_eq!(Cell::new(Material::Grass, FULL, 9).liquid, 0);
        assert_eq!(Cell::new(Material::Water, 100, 9).liquid, 0);
        assert_eq!(Cell::new(Material::Grass, 100, 9).liquid, 9);
    }

    #[test]
    fn zero_fraction_is_air() {
        assert_eq!(Cell::with_fraction(Material::Sand, 0.0), Cell::AIR);
        assert_eq!(Cell::with_fraction(Material::Sand, f32::NAN), Cell::AIR);
        assert!(!Cell::with_fraction(Material::Sand, 0.001).is_air());
    }

    #[test]
    fn water_share_never_overfills_the_voxel() {
        let cell = Cell::new(Material::Rock, 191, 255);
        assert!((cell.solid_fraction() + cell.water_fraction() - 1.0).abs() < 1e-6);
        assert_eq!(Cell::full(Material::Water).water_fraction(), 1.0);
        assert_eq!(Cell::full(Material::Water).solid_fraction(), 0.0);
    }
}
