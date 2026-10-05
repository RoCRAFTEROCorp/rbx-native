//! The 23 materials a terrain voxel can hold, in the order `SmoothGrid`
//! stores them.
//!
//! The order is Roblox's own terrain material table. It is the same order
//! `MaterialColors` lays its 23 colour slots out in (rbx-dom's
//! `docs/binary-strings.md`), and decoding a real place's `SmoothGrid` with it
//! gives a CrackedLava/Rock/Slate/Sandstone/Grass layering that matches what
//! the place shows. Two independent readers of the format use the same table.

/// A terrain voxel's material. The discriminant is the slot `SmoothGrid`
/// stores in the low six bits of a run's lead byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Material {
    #[default]
    Air = 0,
    Water,
    Grass,
    Slate,
    Concrete,
    Brick,
    Sand,
    WoodPlanks,
    Rock,
    Glacier,
    Snow,
    Sandstone,
    Mud,
    Basalt,
    Ground,
    CrackedLava,
    Asphalt,
    Cobblestone,
    Ice,
    LeafyGrass,
    Salt,
    Limestone,
    Pavement,
}

/// `(material, name, Enum.Material value)` for every slot, in slot order.
/// The enum values come from the API dump, not from the slot numbers: the two
/// orders are unrelated.
const TABLE: [(Material, &str, u32); 23] = [
    (Material::Air, "Air", 1792),
    (Material::Water, "Water", 2048),
    (Material::Grass, "Grass", 1280),
    (Material::Slate, "Slate", 800),
    (Material::Concrete, "Concrete", 816),
    (Material::Brick, "Brick", 848),
    (Material::Sand, "Sand", 1296),
    (Material::WoodPlanks, "WoodPlanks", 528),
    (Material::Rock, "Rock", 896),
    (Material::Glacier, "Glacier", 1552),
    (Material::Snow, "Snow", 1328),
    (Material::Sandstone, "Sandstone", 912),
    (Material::Mud, "Mud", 1344),
    (Material::Basalt, "Basalt", 788),
    (Material::Ground, "Ground", 1360),
    (Material::CrackedLava, "CrackedLava", 804),
    (Material::Asphalt, "Asphalt", 1376),
    (Material::Cobblestone, "Cobblestone", 880),
    (Material::Ice, "Ice", 1536),
    (Material::LeafyGrass, "LeafyGrass", 1284),
    (Material::Salt, "Salt", 1392),
    (Material::Limestone, "Limestone", 820),
    (Material::Pavement, "Pavement", 836),
];

impl Material {
    pub const ALL: [Material; 23] = {
        let mut all = [Material::Air; 23];
        let mut slot = 0;
        while slot < 23 {
            all[slot] = TABLE[slot].0;
            slot += 1;
        }
        all
    };

    /// The materials a brush can lay down: everything but Air.
    pub fn paintable() -> impl Iterator<Item = Material> {
        Self::ALL.into_iter().skip(1)
    }

    pub fn from_slot(slot: u8) -> Option<Material> {
        TABLE.get(usize::from(slot)).map(|entry| entry.0)
    }

    pub fn slot(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        TABLE[usize::from(self.slot())].1
    }

    pub fn from_name(name: &str) -> Option<Material> {
        TABLE
            .iter()
            .find(|entry| entry.1 == name)
            .map(|entry| entry.0)
    }

    /// The `Enum.Material` value, which is what `rbx_materials` and part
    /// rendering key their textures by.
    pub fn enum_value(self) -> u32 {
        TABLE[usize::from(self.slot())].2
    }

    pub fn from_enum_value(value: u32) -> Option<Material> {
        TABLE
            .iter()
            .find(|entry| entry.2 == value)
            .map(|entry| entry.0)
    }

    /// Anything that is neither Air nor Water: what the solid surface is made
    /// of.
    pub fn is_solid(self) -> bool {
        self > Material::Water
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_round_trip_and_stop_at_pavement() {
        for (slot, material) in Material::ALL.iter().enumerate() {
            assert_eq!(material.slot() as usize, slot);
            assert_eq!(Material::from_slot(slot as u8), Some(*material));
            assert_eq!(Material::from_name(material.name()), Some(*material));
            assert_eq!(
                Material::from_enum_value(material.enum_value()),
                Some(*material)
            );
        }
        assert_eq!(Material::from_slot(23), None);
        assert_eq!(Material::Pavement.slot(), 22);
    }

    #[test]
    fn enum_values_match_the_api_dump() {
        // Spot checks on the ones whose slot and enum order differ most.
        assert_eq!(Material::WoodPlanks.enum_value(), 528);
        assert_eq!(Material::Glacier.enum_value(), 1552);
        assert_eq!(Material::Ice.enum_value(), 1536);
        assert_eq!(Material::Water.enum_value(), 2048);
    }

    #[test]
    fn only_air_and_water_are_not_solid() {
        assert!(!Material::Air.is_solid());
        assert!(!Material::Water.is_solid());
        assert!(Material::paintable().all(|m| m != Material::Air));
        assert!(Material::Grass.is_solid());
    }
}
