//! `Terrain.MaterialColors` the way Studio's panel lists it: one row that
//! expands into a Color3 row per solid terrain material (creator-docs
//! `parts/terrain.md`, "Custom terrain colors"), rather than the 69-byte blob
//! the property really is.
//!
//! Also names Terrain's two voxel stores, which no panel row edits: the
//! terrain tools are their editor, and their bytes mean nothing typed.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_terrain::{Material, MaterialColors};

use super::{EditKind, PropertyRow};

pub(super) const PROPERTY: &str = "MaterialColors";
const TERRAIN: &str = "Terrain";

/// The wire id XML's `BinaryString` round-trips, so a written blob saves as
/// one in either format — the same reasoning as
/// `attributes::STRING_TYPE_ID`. A `Variant::String` cannot hold it: the
/// bytes are rarely valid UTF-8.
const STRING_TYPE_ID: u8 = 0x01;

/// `SmoothGrid` and `PhysicsGrid` are not in the reflection dump, so without
/// this they list under "Other" as `<n bytes>`; Studio lists neither.
pub(super) fn is_voxel_store(name: &str) -> bool {
    name == "SmoothGrid" || name == "PhysicsGrid"
}

/// The section for a Terrain property newer than the bundled API dump:
/// `GrassLength` is `Appearance` in creator-docs' `Terrain.yaml`, beside the
/// rest of Terrain's look, rather than "Other".
pub(super) fn unreflected_category(class: &str, name: &str) -> Option<&'static str> {
    (class == TERRAIN && name == "GrassLength").then_some("Appearance")
}

pub(super) fn applies(class: &str, name: &str) -> bool {
    class == TERRAIN && name == PROPERTY
}

/// What a Terrain without the property shows and starts from: Studio's own
/// current palette, since that is what a place saved now would carry.
pub(super) fn default_blob() -> Variant {
    blob(&MaterialColors::default())
}

fn blob(colors: &MaterialColors) -> Variant {
    Variant::Unknown {
        type_id: STRING_TYPE_ID,
        raw: colors.encode(),
    }
}

/// A blob of any other length (or a missing one) reads as the defaults
/// rather than as nothing, so every child row still has a colour to edit.
fn decode(value: Option<&Variant>) -> MaterialColors {
    let bytes: &[u8] = match value {
        Some(Variant::String(text)) => text.as_bytes(),
        Some(Variant::Unknown { raw, .. }) => raw,
        _ => &[],
    };
    MaterialColors::decode(bytes).unwrap_or_default()
}

/// One Color3 row per solid material, in Roblox's terrain material order,
/// each committing through [`material_of_row`].
pub(super) fn children(value: &Variant, category: &str) -> Vec<PropertyRow> {
    let colors = decode(Some(value));
    Material::ALL
        .into_iter()
        .filter(|material| material.is_solid())
        .map(|material| {
            let [r, g, b] = colors.get(material);
            PropertyRow {
                name: format!("{PROPERTY}{}{}", PropertyRow::CHILD, material.name()),
                value: format!("({r}, {g}, {b})"),
                category: category.to_owned(),
                edit: Some(EditKind::Color { r, g, b }),
                mixed: false,
                children: Vec::new(),
            }
        })
        .collect()
}

/// The material a child row built by [`children`] edits.
pub(super) fn material_of_row(name: &str) -> Option<Material> {
    name.strip_prefix(PROPERTY)?
        .strip_prefix(PropertyRow::CHILD)
        .and_then(Material::from_name)
        .filter(|material| material.is_solid())
}

/// Writes one material's colour (`r, g, b`, 0–255) into every selected
/// Terrain's blob, re-encoded whole. An instance whose blob already says
/// exactly that is left alone, so a no-op pick adds no undo step.
pub(super) fn commit_all(
    dom: &mut WeakDom,
    selection: &[Ref],
    material: Material,
    text: &str,
) -> Result<(), String> {
    let (r, g, b) = super::edit::parse_color3uint8(text.trim())?;
    for &reference in selection {
        let instance = dom
            .get(reference)
            .ok_or_else(|| "the instance no longer exists".to_string())?;
        let stored = instance.properties().get(PROPERTY);
        let mut colors = decode(stored);
        colors.set(material, [r, g, b]);
        let next = blob(&colors);
        if stored != Some(&next) {
            dom.set_property(reference, PROPERTY, next)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
