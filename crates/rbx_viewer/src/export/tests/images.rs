//! Images and colour: textures, `SurfaceAppearance` maps, packs and plain colour.

use super::*;

/// A `MeshPart` with a `TextureID` image: the `.obj` names a `.mtl` that maps
/// the `.png` written beside it, and the glTF material carries that PNG.
#[test]
fn a_textured_mesh_part_brings_its_image_to_both_formats() {
    let mut dom = WeakDom::new();
    let rock = part(&mut dom, "MeshPart", "Mossy Rock", None, [0.0, 0.0, 0.0]);
    dom.set_property(rock, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();
    dom.set_property(rock, "TextureID", Variant::String("rbxassetid://43".into()))
        .unwrap();
    let image = Image {
        width: 2,
        height: 1,
        pixels: vec![255, 0, 0, 255, 0, 0, 255, 255],
    };
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(triangle()))]),
        HashMap::from([(rock, Arc::new(image.clone()))]),
    );

    let exported = export(&dom, &meshes, &[rock]);
    let png = exported.textures[exported.meshes[0].maps.color.unwrap()].clone();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&png))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
    decoder.next_frame(&mut pixels).unwrap();
    assert_eq!(pixels, image.pixels);
    assert_eq!(exported.meshes[0].uvs, [[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]]);

    let files = obj_files(&exported, "Mossy Rock");
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["Mossy Rock.obj", "Mossy_Rock.mtl", "Mossy_Rock_0.png"]
    );
    assert_eq!(files[2].1, png);
    let text = std::str::from_utf8(&files[0].1).unwrap();
    assert!(text.contains("mtllib Mossy_Rock.mtl\n"));
    assert!(text.contains("usemtl Mossy_Rock_0\n"));
    // V flipped to OBJ's bottom-up convention.
    assert!(text.contains("vt 0 0\nvt 1 0\nvt 0.5 1\n"));
    assert!(text.contains("f 1/1/1 2/2/2 3/3/3"));
    let material = std::str::from_utf8(&files[1].1).unwrap();
    assert!(material.contains("newmtl Mossy_Rock_0\n"));
    assert!(material.contains("map_Kd Mossy_Rock_0.png\n"));

    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();
    let primitive = &document["meshes"][0]["primitives"][0];
    let uv =
        &document["accessors"][primitive["attributes"]["TEXCOORD_0"].as_u64().unwrap() as usize];
    assert_eq!(uv["type"], "VEC2");
    assert_eq!(uv["count"], 3);
    let texture = &document["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"];
    let source = &document["textures"][texture.as_u64().unwrap() as usize]["source"];
    let uri = document["images"][source.as_u64().unwrap() as usize]["uri"]
        .as_str()
        .unwrap();
    let encoded = uri.strip_prefix("data:image/png;base64,").unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap(),
        png
    );
}

/// Three parts wearing one image: it is encoded once, written as one `.png`
/// all three `.mtl` materials name, and embedded as one glTF image all three
/// materials sample.
#[test]
fn a_texture_shared_by_several_parts_is_written_once() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Rocks", None);
    let image = Arc::new(Image {
        width: 1,
        height: 1,
        pixels: vec![0, 128, 255, 255],
    });
    let mut textures = HashMap::new();
    for x in [0.0, 5.0, 10.0] {
        let rock = part(&mut dom, "MeshPart", "Rock", Some(model), [x, 0.0, 0.0]);
        dom.set_property(rock, "MeshId", Variant::String("rbxassetid://42".into()))
            .unwrap();
        textures.insert(rock, Arc::clone(&image));
    }
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(triangle()))]),
        textures,
    );

    let exported = export(&dom, &meshes, &[model]);

    assert_eq!(exported.meshes.len(), 3);
    assert_eq!(exported.textures.len(), 1);
    assert!(exported.meshes.iter().all(|m| m.maps.color == Some(0)));

    let files = obj_files(&exported, "Rocks");
    let pngs: Vec<_> = files.iter().filter(|(n, _)| n.ends_with(".png")).collect();
    assert_eq!(pngs.len(), 1);
    assert_eq!(pngs[0].0, "Rocks_0.png");
    let material = std::str::from_utf8(&files[1].1).unwrap();
    assert_eq!(count(material, "map_Kd Rocks_0.png"), 3);

    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();
    assert_eq!(document["images"].as_array().unwrap().len(), 1);
    assert_eq!(document["textures"].as_array().unwrap().len(), 1);
    for material in document["materials"].as_array().unwrap() {
        assert_eq!(
            material["pbrMetallicRoughness"]["baseColorTexture"]["index"],
            0
        );
    }
    let uri = document["images"][0]["uri"].as_str().unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(uri.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&png))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
    decoder.next_frame(&mut pixels).unwrap();
    assert_eq!(pixels, image.pixels);
}

/// `Overlay`: the colour map is baked over the part's red where it is clear,
/// the tint is the base colour factor, and the other maps come as authored —
/// metalness and roughness packed for glTF, apart for OBJ.
#[test]
fn a_surface_appearance_exports_its_four_maps() {
    let (exported, [_, normal, metalness, roughness]) = dressed(AlphaMode::Overlay);
    let mesh = &exported.meshes[0];
    assert_eq!(mesh.color, [0.5, 1.0, 1.0, 1.0]);
    assert!(!mesh.blend);

    let png_of = |index: Option<usize>| exported.textures[index.unwrap()].clone();
    assert_eq!(
        decode(&png_of(mesh.maps.color)).2,
        [255, 0, 0, 255, 0, 255, 0, 255]
    );
    assert_eq!(decode(&png_of(mesh.maps.normal)).2, normal.pixels);
    // The frame the viewport reads it in comes along: the triangle's U runs
    // along +X (see `triangle`), and V down the image means up the mesh.
    assert_eq!(mesh.tangents.len(), mesh.positions.len());
    assert!(mesh.tangents.iter().all(|t| t[0] > 0.99 && t[3] == 1.0));
    assert_eq!(decode(&png_of(mesh.maps.metalness)).2, metalness.pixels);
    assert_eq!(decode(&png_of(mesh.maps.roughness)).2, roughness.pixels);
    let (width, height, packed) = decode(&png_of(mesh.maps.metallic_roughness));
    assert_eq!((width, height), (2, 1));
    assert_eq!(packed, [255, 10, 200, 255, 255, 90, 200, 255]);

    let files = obj_files(&exported, "Statue");
    assert_eq!(files.len(), 2 + 4);
    let material = std::str::from_utf8(&files[1].1).unwrap();
    for (key, index) in [
        ("map_Kd", mesh.maps.color),
        ("map_Bump", mesh.maps.normal),
        ("map_Pm", mesh.maps.metalness),
        ("map_Pr", mesh.maps.roughness),
    ] {
        let line = format!("{key} Statue_{}.png\n", index.unwrap());
        assert!(material.contains(&line), "{line} in {material}");
    }

    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();
    let material = &document["materials"][0];
    let pbr = &material["pbrMetallicRoughness"];
    assert_eq!(
        pbr["baseColorFactor"],
        serde_json::json!([0.5, 1.0, 1.0, 1.0])
    );
    assert_eq!(pbr["metallicFactor"], 1.0);
    assert_eq!(pbr["roughnessFactor"], 1.0);
    assert!(material.get("alphaMode").is_none());
    assert_eq!(
        embedded(&document, &pbr["baseColorTexture"]),
        png_of(mesh.maps.color)
    );
    assert_eq!(
        embedded(&document, &material["normalTexture"]),
        png_of(mesh.maps.normal)
    );
    assert_eq!(
        embedded(&document, &pbr["metallicRoughnessTexture"]),
        png_of(mesh.maps.metallic_roughness)
    );
    // The two greyscale maps are OBJ's alone.
    assert_eq!(document["images"].as_array().unwrap().len(), 3);
}

/// `Transparency`: the colour map goes out as it is, and its alpha makes the
/// material blend.
#[test]
fn a_transparency_surface_appearance_blends_its_colour_map() {
    let (exported, [color, ..]) = dressed(AlphaMode::Transparency);
    let mesh = &exported.meshes[0];
    assert!(mesh.blend);
    assert_eq!(
        decode(&exported.textures[mesh.maps.color.unwrap()]).2,
        color.pixels
    );
    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();
    assert_eq!(document["materials"][0]["alphaMode"], "BLEND");
}

/// A 4 x 2 x 2 brick whose material tiles every 2 studs: each face carries
/// its own projection, one tile per 2 studs along the face as the shader
/// lays it, and the pack's maps come along with the part's colour as the
/// factor beneath them.
#[test]
fn a_textured_material_is_projected_onto_its_part() {
    let mut dom = WeakDom::new();
    let brick = part(&mut dom, "Part", "Wall", None, [0.0, 0.0, 0.0]);
    dom.set_property(brick, "size", vector3(4.0, 2.0, 2.0))
        .unwrap();
    dom.set_property(
        brick,
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    let color = image(
        2,
        2,
        &[
            200, 50, 50, 255, 90, 40, 40, 255, 90, 40, 40, 255, 200, 50, 50, 255,
        ],
    );
    let normal = image(1, 1, &[128, 128, 255, 255]);
    let meshes = Meshes::default().with_materials(HashMap::from([(
        brick,
        Arc::new(Pack {
            maps: [Some(Arc::clone(&color)), Some(normal), None, None],
            studs_per_tile: 2.0,
        }),
    )]));

    let exported = export(&dom, &meshes, &[brick]);
    let mesh = &exported.meshes[0];

    assert_eq!(mesh.positions.len(), 36);
    assert_eq!(mesh.uvs.len(), 36);
    assert_eq!(mesh.indices, (0..36).collect::<Vec<u32>>());
    let top: Vec<[f32; 2]> = (0..36)
        .filter(|&i| mesh.normals[i][1] > 0.9)
        .map(|i| mesh.uvs[i])
        .collect();
    assert_eq!(top.len(), 6);
    for [u, v] in &top {
        // x spans -2..2 studs, z -1..1: one tile across 2 studs.
        assert!((u.abs() - 1.0).abs() < 1e-5 && (v.abs() - 0.5).abs() < 1e-5);
    }
    // Facing +Z, image right is +X and image down is -Y.
    let front: Vec<([f32; 3], [f32; 2])> = (0..36)
        .filter(|&i| mesh.normals[i][2] > 0.9)
        .map(|i| (mesh.positions[i], mesh.uvs[i]))
        .collect();
    for ([x, y, _], [u, v]) in &front {
        assert!((u - x / 2.0).abs() < 1e-5 && (v + y / 2.0).abs() < 1e-5);
    }

    assert_eq!(mesh.color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(
        decode(&exported.textures[mesh.maps.color.unwrap()]).2,
        color.pixels
    );
    assert!(mesh.maps.normal.is_some() && mesh.maps.metallic_roughness.is_none());

    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();
    let primitive = &document["meshes"][0]["primitives"][0];
    assert!(primitive["attributes"]["TEXCOORD_0"].is_u64());
    let material = &document["materials"][0];
    assert_eq!(
        embedded(
            &document,
            &material["pbrMetallicRoughness"]["baseColorTexture"]
        ),
        exported.textures[mesh.maps.color.unwrap()]
    );
    assert!(material["normalTexture"].is_object());
}

/// Without an image, the `.mtl` still carries the part's colour, and the
/// glTF leaves out the texture arrays the specification forbids empty.
#[test]
fn an_untextured_part_exports_its_colour_as_its_material() {
    let mut dom = WeakDom::new();
    let brick = part(&mut dom, "Part", "Brick", None, [0.0, 0.0, 0.0]);
    dom.set_property(
        brick,
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    let exported = export(&dom, &Meshes::default(), &[brick]);

    let files = obj_files(&exported, "Brick");
    assert_eq!(files.len(), 2);
    let material = std::str::from_utf8(&files[1].1).unwrap();
    assert!(material.contains("newmtl Brick_0\nKd 1 0 0\nd 1\n"));
    assert!(!material.contains("map_Kd"));
    assert!(std::str::from_utf8(&files[0].1)
        .unwrap()
        .contains("f 1//1 "));

    let document: Value = serde_json::from_str(&gltf(&exported)).unwrap();
    assert!(document.get("images").is_none() && document.get("textures").is_none());
    assert!(document["meshes"][0]["primitives"][0]["attributes"]
        .get("TEXCOORD_0")
        .is_none());
}

/// The editor's glTF names its images and buffer as files beside it rather
/// than embedding them, so a large export stays within what a JavaScript
/// viewer can parse; the files' bytes are the very PNG and vertices.
#[test]
fn gltf_files_put_the_buffer_and_images_beside_the_document() {
    let (exported, _) = dressed(AlphaMode::Overlay);

    let files = gltf_files(&exported, "My Rock");
    let document: Value = serde_json::from_slice(&files[0].1).unwrap();

    assert_eq!(files[0].0, "My Rock.gltf");
    assert_eq!(document["buffers"][0]["uri"], "My_Rock.bin");
    let bin = files
        .iter()
        .find(|(name, _)| name == "My_Rock.bin")
        .unwrap();
    assert_eq!(document["buffers"][0]["byteLength"], bin.1.len());
    let images = document["images"].as_array().unwrap();
    assert!(!images.is_empty());
    for image in images {
        let uri = image["uri"].as_str().unwrap();
        assert!(
            uri.starts_with("My_Rock_") && uri.ends_with(".png"),
            "{uri}"
        );
        let (_, bytes) = files.iter().find(|(name, _)| name == uri).unwrap();
        assert!(exported.textures.contains(bytes));
    }
    assert_eq!(files.len(), 2 + images.len());
}
