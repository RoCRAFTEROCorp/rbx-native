use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Vector3Data};
use serde_json::Value;

use super::*;
use crate::pick::{Pack, Surface};

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn vector3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn part(dom: &mut WeakDom, class: &str, name: &str, parent: Option<Ref>, at: [f32; 3]) -> Ref {
    let referent = dom.new_instance(class, name, parent);
    let [x, y, z] = at;
    dom.set_property(
        referent,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y, z },
            rotation: IDENTITY,
        }),
    )
    .unwrap();
    dom.set_property(referent, "size", vector3(1.0, 1.0, 1.0))
        .unwrap();
    referent
}

fn export(dom: &WeakDom, meshes: &Meshes, roots: &[Ref]) -> Export {
    meshes_of(dom, &ReflectionDatabase::embedded(), meshes, roots)
}

/// One triangle two units across, its UVs spanning the image.
fn triangle() -> rbx_mesh::Mesh {
    let vertex = |position: [f32; 3], uv: [f32; 2]| rbx_mesh::Vertex {
        position,
        normal: [0.0, 0.0, 1.0],
        uv,
        color: [255; 4],
    };
    rbx_mesh::Mesh {
        version: (4, 1),
        vertices: vec![
            vertex([-1.0, -1.0, 0.0], [0.0, 1.0]),
            vertex([1.0, -1.0, 0.0], [1.0, 1.0]),
            vertex([0.0, 1.0, 0.0], [0.5, 0.0]),
        ],
        indices: vec![0, 1, 2],
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-1.0, -1.0, 0.0],
            max: [1.0, 1.0, 0.0],
        },
    }
}

fn count(text: &str, prefix: &str) -> usize {
    text.lines().filter(|line| line.starts_with(prefix)).count()
}

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
    assert_eq!(document["scenes"][0]["nodes"].as_array().unwrap().len(), 2);
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
        assert_eq!(first, mesh.positions[0]);
    }

    let blended = exported.meshes.iter().position(|m| m.name == "A").unwrap();
    assert_eq!(document["materials"][blended]["alphaMode"], "BLEND");
    assert!(document["materials"][1 - blended]
        .get("alphaMode")
        .is_none());
}

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

fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut pixels).unwrap();
    (info.width, info.height, pixels)
}

/// The PNG a glTF texture index embeds.
fn embedded(document: &Value, texture: &Value) -> Vec<u8> {
    let source = &document["textures"][texture["index"].as_u64().unwrap() as usize]["source"];
    let uri = document["images"][source.as_u64().unwrap() as usize]["uri"]
        .as_str()
        .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(uri.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap()
}

fn image(width: u32, height: u32, pixels: &[u8]) -> Arc<Image> {
    Arc::new(Image {
        width,
        height,
        pixels: pixels.to_vec(),
    })
}

/// A red `MeshPart` wearing a `SurfaceAppearance` with `alpha_mode`, a
/// two-texel colour map (clear blue, then opaque green), a normal map, a
/// one-texel metalness map and a two-texel roughness map.
fn dressed(alpha_mode: AlphaMode) -> (Export, [Arc<Image>; 4]) {
    let mut dom = WeakDom::new();
    let statue = part(&mut dom, "MeshPart", "Statue", None, [0.0, 0.0, 0.0]);
    dom.set_property(statue, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();
    dom.set_property(
        statue,
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    let maps = [
        image(2, 1, &[0, 0, 255, 0, 0, 255, 0, 255]),
        image(1, 1, &[128, 128, 255, 255]),
        image(1, 1, &[200, 0, 0, 255]),
        image(2, 1, &[10, 0, 0, 255, 90, 0, 0, 255]),
    ];
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(triangle()))]),
        HashMap::new(),
    )
    .with_surfaces(HashMap::from([(
        statue,
        Surface {
            maps: maps.clone().map(Some),
            tint: [0.5, 1.0, 1.0],
            alpha_mode,
        },
    )]));
    (export(&dom, &meshes, &[statue]), maps)
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

/// A real place through the same streamed meshes and images the editor
/// exports from: every textured mesh comes out with its PNG, and every union
/// whose boolean carved with its own triangles rather than a box. Set
/// `RBX_EXPORT_OUT=<dir>` to keep the `.obj`/`.mtl`/`.png`s and `.gltf` it
/// wrote, to open in another tool.
#[test]
#[ignore = "needs RBX_EXPORT_FIXTURE=<place> and the network or an asset cache"]
fn a_real_place_exports_its_textures_and_unions() {
    let path = std::env::var("RBX_EXPORT_FIXTURE").expect("RBX_EXPORT_FIXTURE");
    let mut viewer = crate::Headless::load(std::path::Path::new(&path), true).unwrap();
    // Streams in on ticks; quiet for a few seconds means everything landed.
    let mut quiet = 0;
    for _ in 0..1200 {
        viewer.tick(std::time::Duration::from_millis(50));
        quiet = if viewer.pick_meshes_changed() {
            0
        } else {
            quiet + 1
        };
        if quiet > 100 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let dom = crate::read_place(std::path::Path::new(&path)).unwrap();
    let database = ReflectionDatabase::embedded();
    let exported = meshes_of(&dom, &database, &viewer.pick_meshes(), dom.root_refs());

    let textured = exported
        .meshes
        .iter()
        .filter(|m| m.maps.color.is_some())
        .count();
    let unions: Vec<_> = dom
        .root_refs()
        .iter()
        .flat_map(|&root| descendants_of(&dom, root))
        .filter(|&r| dom.get(r).is_some_and(|i| i.class() == "UnionOperation"))
        .filter_map(|r| union_fit(&dom, &database, r).map(|(asset, _)| (r, asset)))
        .collect();
    let carved = unions
        .iter()
        .filter(|(_, asset)| viewer.pick_meshes().get(asset).is_some())
        .count();
    let surfaced = exported
        .meshes
        .iter()
        .filter(|m| m.maps.normal.is_some() || m.maps.metallic_roughness.is_some())
        .count();
    eprintln!(
        "{} parts, {textured} textured, {surfaced} with PBR maps, \
         {} distinct images, {carved}/{} legacy unions carved",
        exported.meshes.len(),
        exported.textures.len(),
        unions.len()
    );
    // Before the asserts, so a place that fails one can still be looked at.
    if let Ok(out) = std::env::var("RBX_EXPORT_OUT") {
        let out = std::path::Path::new(&out);
        for (name, bytes) in obj_files(&exported, "export") {
            std::fs::write(out.join(name), bytes).unwrap();
        }
        std::fs::write(out.join("export.gltf"), gltf(&exported)).unwrap();
    }

    assert!(textured > 0);
    assert!(carved > 0);
}
