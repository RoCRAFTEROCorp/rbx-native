//! Which properties are legacy ContentIds: `Decal.Texture`, `MeshPart.MeshId`
//! and the like, which Roblox saves as a String column. The DOM may hold one as
//! a `Content` (Luau and the Properties panel set it that way), and a Content
//! column under such a name is a shape no real file has.
//!
//! The bundled API dump predates release 645's split: it spells every ContentId
//! `Content` and lists none of the true Content properties (`TextureContent`,
//! `MeshContent`), which therefore never match here. A newer dump spells them
//! `ContentId`, so that spelling is used whenever the dump has any.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::Value;

use super::service::API_DUMP_JSON;

struct Class {
    superclass: Option<String>,
    content_ids: HashSet<String>,
}

fn classes() -> &'static HashMap<String, Class> {
    static CLASSES: OnceLock<HashMap<String, Class>> = OnceLock::new();
    CLASSES.get_or_init(|| {
        let dump: Value =
            serde_json::from_str(API_DUMP_JSON).expect("bundled API-Dump.json must parse");
        let classes = dump["Classes"].as_array().map_or(&[][..], Vec::as_slice);
        let typed = |member: &Value, name: &str| {
            member["MemberType"] == "Property" && member["ValueType"]["Name"] == name
        };
        let any_content_id = classes.iter().any(|class| {
            class["Members"]
                .as_array()
                .is_some_and(|members| members.iter().any(|m| typed(m, "ContentId")))
        });
        let spelling = if any_content_id {
            "ContentId"
        } else {
            "Content"
        };
        classes
            .iter()
            .filter_map(|class| {
                let content_ids = class["Members"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|member| typed(member, spelling))
                    .filter_map(|member| member["Name"].as_str().map(str::to_owned))
                    .collect();
                let class_entry = Class {
                    superclass: class["Superclass"].as_str().map(str::to_owned),
                    content_ids,
                };
                Some((class["Name"].as_str()?.to_owned(), class_entry))
            })
            .collect()
    })
}

/// Whether `class` (or a superclass) declares `property` as a ContentId.
pub(crate) fn is_content_id(class: &str, property: &str) -> bool {
    let classes = classes();
    let mut current = classes.get(class);
    while let Some(entry) = current {
        if entry.content_ids.contains(property) {
            return true;
        }
        current = entry
            .superclass
            .as_deref()
            .and_then(|name| classes.get(name));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_asset_ids_are_content_ids_and_their_content_twins_are_not() {
        assert!(is_content_id("Decal", "Texture"));
        assert!(is_content_id("MeshPart", "MeshId"));
        // Declared on `Decal`, inherited by `Texture`.
        assert!(is_content_id("Texture", "Texture"));
        assert!(!is_content_id("Decal", "TextureContent"));
        assert!(!is_content_id("Decal", "Transparency"));
        assert!(!is_content_id("NotARealClass", "Texture"));
    }
}
