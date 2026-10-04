//! Geometry: what each part exports as, where, and how the formats index it.

use super::*;

#[test]
fn a_unit_cube_is_twelve_triangles_around_its_position() {
    let mut dom = WeakDom::new();
    let cube = part(&mut dom, "Part", "Cube", None, [10.0, 0.0, 0.0]);

    let exported = export(&dom, &Meshes::default(), &[cube]);

    assert_eq!(exported.meshes.len(), 1);
    let mesh = &exported.meshes[0];
    assert_eq!(mesh.name, "Cube");
    assert_eq!(mesh.indices.len(), 36);
    for [x, y, z] in &mesh.positions {
        assert!((x - 10.0).abs() <= 0.5 + 1e-5 && y.abs() <= 0.5 + 1e-5 && z.abs() <= 0.5 + 1e-5);
    }

    let text = obj(&exported.meshes, "x");
    assert_eq!(count(&text, "o "), 1);
    assert_eq!(count(&text, "v "), mesh.positions.len());
    assert_eq!(count(&text, "vn "), mesh.normals.len());
    assert_eq!(count(&text, "f "), 12);
}

/// The second object's faces point past the first object's vertices: OBJ
/// indices are global to the file, not per object.
#[test]
fn obj_face_indices_continue_across_objects() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Pair", None);
    part(&mut dom, "Part", "Red Brick", Some(model), [0.0, 0.0, 0.0]);
    part(&mut dom, "Part", "B", Some(model), [5.0, 0.0, 0.0]);

    let exported = export(&dom, &Meshes::default(), &[model]);
    let text = obj(&exported.meshes, "x");

    assert_eq!(count(&text, "o "), 2);
    assert!(text.contains("o Red_Brick\n"));
    let first = exported
        .meshes
        .iter()
        .position(|m| m.name == "Red Brick")
        .unwrap();
    let offset = exported.meshes[first].positions.len();
    let second_object = text.split("\no ").nth(2).unwrap();
    let smallest = second_object
        .lines()
        .filter_map(|line| line.strip_prefix("f "))
        .flat_map(|faces| faces.split(' '))
        .map(|corner| corner.split("//").next().unwrap().parse::<usize>().unwrap())
        .min()
        .unwrap();
    assert_eq!(smallest, offset + 1);
}

/// A part with parts under it exports them too, and a selection holding both
/// a model and one of its parts exports that part once.
#[test]
fn a_subtree_exports_every_part_in_it_once() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let outer = part(&mut dom, "Part", "Outer", Some(model), [0.0, 0.0, 0.0]);
    part(&mut dom, "WedgePart", "Inner", Some(outer), [0.0, 2.0, 0.0]);
    dom.new_instance("Folder", "NotGeometry", Some(model));

    let exported = export(&dom, &Meshes::default(), &[model, outer]);

    let mut names: Vec<_> = exported.meshes.iter().map(|m| m.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Inner", "Outer"]);
}

/// A `MeshPart` exports its downloaded triangles, fitted from `InitialSize`
/// to `size` exactly as it is drawn.
#[test]
fn a_mesh_part_exports_its_own_triangles_scaled_to_its_size() {
    let mut dom = WeakDom::new();
    let rock = part(&mut dom, "MeshPart", "Rock", None, [0.0, 3.0, 0.0]);
    dom.set_property(rock, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();
    dom.set_property(rock, "InitialSize", vector3(2.0, 2.0, 2.0))
        .unwrap();
    dom.set_property(rock, "size", vector3(4.0, 4.0, 4.0))
        .unwrap();
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(triangle()))]),
        HashMap::new(),
    );

    let exported = export(&dom, &meshes, &[rock]);

    assert_eq!(exported.meshes.len(), 1);
    assert_eq!(exported.meshes[0].indices, [0, 1, 2]);
    assert_eq!(
        exported.meshes[0].positions,
        [[-2.0, 1.0, 0.0], [2.0, 1.0, 0.0], [0.0, 5.0, 0.0]]
    );
    assert_eq!(exported.meshes[0].normals, [[0.0, 0.0, 1.0]; 3]);

    let text = obj(&exported.meshes, "x");
    assert_eq!(count(&text, "v "), 3);
    assert!(text.contains("f 1/1/1 2/2/2 3/3/3"));
}

/// Without its download, a `MeshPart` is the box it is drawn as.
#[test]
fn a_mesh_part_that_has_not_downloaded_exports_its_box() {
    let mut dom = WeakDom::new();
    let rock = part(&mut dom, "MeshPart", "Rock", None, [0.0, 0.0, 0.0]);
    dom.set_property(rock, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();

    let exported = export(&dom, &Meshes::default(), &[rock]);

    assert_eq!(exported.meshes[0].indices.len(), 36);
}

/// The document's shape against the glTF 2.0 schema's required members, and
/// the buffer's bytes against the vertices they were written from.
#[test]
fn gltf_is_a_valid_two_point_oh_document_with_the_vertices_inline() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Pair", None);
    let a = part(&mut dom, "Part", "A", Some(model), [0.0, 0.0, 0.0]);
    part(&mut dom, "Part", "B", Some(model), [5.0, 0.0, 0.0]);
    dom.set_property(a, "Transparency", Variant::Float32(0.5))
        .unwrap();
    let exported = export(&dom, &Meshes::default(), &[model]);

    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();

    assert_eq!(document["asset"]["version"], "2.0");
    // The model is the one root, its parts its children.
    let roots = document["scenes"][0]["nodes"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    let root = &document["nodes"][roots[0].as_u64().unwrap() as usize];
    assert_eq!(root["name"], "Pair");
    assert_eq!(root["extras"]["RobloxInstanceType"], "Model");
    assert_eq!(root["children"].as_array().unwrap().len(), 2);
    assert_eq!(document["meshes"].as_array().unwrap().len(), 2);
    assert_eq!(document["accessors"].as_array().unwrap().len(), 6);
    assert_eq!(document["bufferViews"].as_array().unwrap().len(), 6);

    let uri = document["buffers"][0]["uri"].as_str().unwrap();
    let encoded = uri
        .strip_prefix("data:application/octet-stream;base64,")
        .unwrap();
    let buffer = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    assert_eq!(document["buffers"][0]["byteLength"], buffer.len());

    for (index, mesh) in exported.meshes.iter().enumerate() {
        let primitive = &document["meshes"][index]["primitives"][0];
        assert_eq!(primitive["mode"], 4);
        let position =
            &document["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
        assert_eq!(position["count"], mesh.positions.len());
        assert_eq!(position["componentType"], 5126);
        assert!(position["min"].is_array() && position["max"].is_array());
        let indices = &document["accessors"][primitive["indices"].as_u64().unwrap() as usize];
        assert_eq!(indices["count"], 36);
        assert_eq!(indices["componentType"], 5125);

        let view = &document["bufferViews"][position["bufferView"].as_u64().unwrap() as usize];
        let offset = view["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(offset % 4, 0);
        let first: Vec<f32> = buffer[offset..offset + 12]
            .chunks(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        // In the part's own frame, which its node places.
        let node = exported.nodes.iter().find(|n| n.meshes == [index]).unwrap();
        let (_, at) = node.placement.unwrap();
        let world: Vec<f32> = (0..3).map(|i| first[i] + at[i]).collect();
        assert_eq!(world, mesh.positions[0]);
    }

    let blended = exported.meshes.iter().position(|m| m.name == "A").unwrap();
    assert_eq!(document["materials"][blended]["alphaMode"], "BLEND");
    assert!(document["materials"][1 - blended]
        .get("alphaMode")
        .is_none());
}

/// A legacy union exports the boolean `scene::union` carved, keyed by its
/// `AssetId` and placed the way it is drawn — `CFrame`, then the resize
/// since `InitialSize` — rather than its box.
#[test]
fn a_union_exports_its_computed_boolean() {
    let mut dom = WeakDom::new();
    let union = part(&mut dom, "UnionOperation", "Arch", None, [0.0, 3.0, 0.0]);
    dom.set_property(union, "AssetId", Variant::String("rbxassetid://77".into()))
        .unwrap();
    dom.set_property(union, "InitialSize", vector3(2.0, 2.0, 2.0))
        .unwrap();
    dom.set_property(union, "size", vector3(4.0, 4.0, 4.0))
        .unwrap();
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(77), Arc::new(triangle()))]),
        HashMap::new(),
    );

    let exported = export(&dom, &meshes, &[union]);

    assert_eq!(exported.meshes[0].indices, [0, 1, 2]);
    assert_eq!(
        exported.meshes[0].positions,
        [[-2.0, 1.0, 0.0], [2.0, 1.0, 0.0], [0.0, 5.0, 0.0]]
    );
    // Before its boolean resolves, the box it is drawn as.
    assert_eq!(
        export(&dom, &Meshes::default(), &[union]).meshes[0]
            .indices
            .len(),
        36
    );
}
