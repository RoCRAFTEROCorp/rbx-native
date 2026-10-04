use rbx_dom::Variant;

use super::*;
use crate::explorer::find_by_name;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// A model with a part, and two `ObjectValue`s: one pointing inside the
/// model, one at a part left outside it.
fn place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let outside = dom.new_instance("Part", "Outside", Some(workspace));
    let model = dom.new_instance("Model", "Car", Some(workspace));
    let wheel = dom.new_instance("Part", "Wheel", Some(model));
    let inner = dom.new_instance("ObjectValue", "Inner", Some(model));
    let outer = dom.new_instance("ObjectValue", "Outer", Some(model));
    dom.set_property(inner, "Value", Variant::Ref(wheel))
        .unwrap();
    dom.set_property(outer, "Value", Variant::Ref(outside))
        .unwrap();
    (dom, model, wheel)
}

fn value_of(dom: &WeakDom, name: &str) -> Option<Variant> {
    let referent = find_by_name(dom, name)?;
    dom.get(referent)?.properties().get("Value").cloned()
}

#[test]
fn save_to_file_writes_the_subtree_as_a_binary_model() {
    let (dom, model, wheel) = place();

    let bytes = encode(
        Export::Model,
        &dom,
        &database(),
        &pick::Meshes::default(),
        &[model, wheel],
        Path::new("Car.rbxm"),
    )
    .unwrap()
    .remove(0)
    .1;
    let read = rbx_binary::deserialize(&bytes).unwrap();

    assert_eq!(
        read.root_refs().len(),
        1,
        "the wheel comes with its model, once"
    );
    let root = read.get(read.root_refs()[0]).unwrap();
    assert_eq!((root.class(), root.name()), ("Model", "Car"));
    assert!(find_by_name(&read, "Outside").is_none());
    assert_eq!(
        value_of(&read, "Inner"),
        Some(Variant::Ref(find_by_name(&read, "Wheel").unwrap()))
    );
    assert_eq!(
        value_of(&read, "Outer"),
        None,
        "a reference left behind is nil"
    );
}

#[test]
fn an_rbxmx_name_saves_xml() {
    let (dom, model, _) = place();

    let bytes = encode(
        Export::Model,
        &dom,
        &database(),
        &pick::Meshes::default(),
        &[model],
        Path::new("Car.rbxmx"),
    )
    .unwrap()
    .remove(0)
    .1;

    let read = rbx_xml::deserialize(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert!(find_by_name(&read, "Wheel").is_some());
}

#[test]
fn a_mesh_export_of_no_parts_is_an_error_not_an_empty_file() {
    let (mut dom, _, _) = place();
    let folder = dom.new_instance("Folder", "Empty", None);

    for kind in [Export::Obj, Export::Gltf] {
        let result = encode(
            kind,
            &dom,
            &database(),
            &pick::Meshes::default(),
            &[folder],
            Path::new("x"),
        );
        assert!(result.is_err());
    }
    assert!(!has_geometry(&dom, &database(), &[folder]));
}
