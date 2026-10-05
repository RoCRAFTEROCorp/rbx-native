use rbx_dom::Instance;
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::properties::{edit, Properties};

fn terrain_ref() -> Ref {
    Ref::new(2)
}

fn terrain(values: &[(&str, Variant)]) -> (WeakDom, Properties) {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    let mut instance = Instance::new(terrain_ref(), TERRAIN, "Terrain");
    for (name, value) in values {
        instance
            .properties_mut()
            .insert((*name).to_owned(), value.clone());
    }
    dom.insert(instance);
    dom.set_parent(terrain_ref(), Some(Ref::new(1)));
    (dom, Properties::new(ReflectionDatabase::embedded()))
}

fn material_colors_row(dom: &WeakDom, properties: &Properties) -> PropertyRow {
    properties
        .rows(dom, &[terrain_ref()], None)
        .into_iter()
        .find(|row| row.name == PROPERTY)
        .expect("a MaterialColors row")
}

/// A blob whose slot `n` holds `(n, n + 1, n + 2)`, so every child's value
/// says which slot it was read from.
fn numbered() -> Vec<u8> {
    (0..23u8)
        .flat_map(|slot| [slot, slot + 1, slot + 2])
        .collect()
}

#[test]
fn one_colour_row_per_solid_material_read_from_the_blob() {
    let (dom, properties) = terrain(&[(
        PROPERTY,
        Variant::Unknown {
            type_id: 1,
            raw: numbered(),
        },
    )]);
    let row = material_colors_row(&dom, &properties);

    assert_eq!(row.edit, None);
    assert_eq!(row.children.len(), 21);
    let labels: Vec<&str> = row.children.iter().map(PropertyRow::label).collect();
    assert_eq!(labels.first(), Some(&"Grass"));
    assert_eq!(labels.last(), Some(&"Pavement"));
    assert!(!labels.contains(&"Air") && !labels.contains(&"Water"));

    let snow = &row.children[8];
    assert_eq!(snow.name, "MaterialColors.Snow");
    assert_eq!(snow.depth(), 1);
    assert_eq!(snow.value, "(10, 11, 12)");
    assert_eq!(
        snow.edit,
        Some(EditKind::Color {
            r: 10,
            g: 11,
            b: 12
        })
    );
}

#[test]
fn a_missing_or_malformed_blob_shows_the_defaults() {
    let grass = MaterialColors::default().get(Material::Grass);
    for values in [vec![], vec![(PROPERTY, Variant::String("abc".into()))]] {
        let (dom, properties) = terrain(&values);
        let row = material_colors_row(&dom, &properties);
        assert_eq!(row.children.len(), 21);
        assert_eq!(
            row.children[0].edit,
            Some(EditKind::Color {
                r: grass[0],
                g: grass[1],
                b: grass[2]
            })
        );
    }
}

#[test]
fn an_edit_rewrites_only_that_slot_as_a_binary_string() {
    let (mut dom, properties) = terrain(&[(
        PROPERTY,
        Variant::String(String::from_utf8(numbered()).unwrap()),
    )]);
    let db = ReflectionDatabase::embedded();
    edit::commit_all(
        &mut dom,
        &db,
        &[terrain_ref()],
        "MaterialColors.Snow",
        "200, 100, 50",
    )
    .unwrap();

    let Some(Variant::Unknown { type_id: 1, raw }) =
        dom.get(terrain_ref()).unwrap().properties().get(PROPERTY)
    else {
        panic!("not a BinaryString blob");
    };
    let mut expected = numbered();
    expected[30..33].copy_from_slice(&[200, 100, 50]);
    assert_eq!(raw, &expected);
    assert_eq!(
        material_colors_row(&dom, &properties).children[8].value,
        "(200, 100, 50)"
    );
}

#[test]
fn an_edit_to_a_terrain_without_the_blob_creates_a_valid_one() {
    let (mut dom, _) = terrain(&[]);
    let db = ReflectionDatabase::embedded();
    edit::commit_all(
        &mut dom,
        &db,
        &[terrain_ref()],
        "MaterialColors.Grass",
        "1, 2, 3",
    )
    .unwrap();

    let mut expected = MaterialColors::default();
    expected.set(Material::Grass, [1, 2, 3]);
    assert_eq!(
        dom.get(terrain_ref()).unwrap().properties().get(PROPERTY),
        Some(&blob(&expected))
    );
    assert!(edit::commit_all(
        &mut dom,
        &db,
        &[terrain_ref()],
        "MaterialColors.Grass",
        "red"
    )
    .is_err());
}

#[test]
fn only_solid_materials_name_a_row() {
    assert_eq!(material_of_row("MaterialColors.Snow"), Some(Material::Snow));
    assert_eq!(material_of_row("MaterialColors.Water"), None);
    assert_eq!(material_of_row("MaterialColors"), None);
    assert_eq!(material_of_row("Snow"), None);
}

#[test]
fn the_voxel_stores_never_get_a_row_nor_match_the_filter() {
    let blob = Variant::Unknown {
        type_id: 1,
        raw: vec![1, 2, 3],
    };
    let (dom, properties) = terrain(&[("SmoothGrid", blob.clone()), ("PhysicsGrid", blob)]);

    let rows = properties.rows(&dom, &[terrain_ref()], None);
    assert!(rows.iter().all(|row| !is_voxel_store(&row.name)));
    for filter in ["grid", "smooth", "physics"] {
        assert!(properties
            .rows_matching(&dom, &[terrain_ref()], filter, None)
            .is_empty());
    }
}

#[test]
fn the_filter_finds_a_material_and_narrows_the_row_to_it() {
    let (dom, properties) = terrain(&[]);
    let rows = properties.rows_matching(&dom, &[terrain_ref()], "snow", None);
    assert_eq!(rows.len(), 1);
    let labels: Vec<&str> = rows[0].children.iter().map(PropertyRow::label).collect();
    assert_eq!(labels, ["Snow"]);

    let rows = properties.rows_matching(&dom, &[terrain_ref()], "materialcolors", None);
    assert_eq!(rows[0].children.len(), 21);
}

#[test]
fn grass_length_sits_with_the_rest_of_terrains_look() {
    let (dom, properties) = terrain(&[("GrassLength", Variant::Float32(0.7))]);
    let rows = properties.rows(&dom, &[terrain_ref()], None);
    let row = rows.iter().find(|row| row.name == "GrassLength").unwrap();
    assert_eq!(row.category, "Appearance");
}
