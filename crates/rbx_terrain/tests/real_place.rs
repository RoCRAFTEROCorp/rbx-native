//! Checks the codecs against a real place's terrain. Needs a place with
//! terrain in it (`RBX_TERRAIN_FIXTURE=path/to.rbxl`), so it is ignored by a
//! plain `cargo test`.

use rbx_dom::Variant;

fn terrain_blob(dom: &rbx_dom::WeakDom, property: &str) -> Vec<u8> {
    let mut stack = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let instance = dom.get(referent).unwrap();
        stack.extend(instance.children().iter().copied());
        if instance.class() == "Terrain" {
            return match instance.properties().get(property) {
                Some(Variant::String(text)) => text.as_bytes().to_vec(),
                Some(Variant::Unknown { raw, .. }) => raw.clone(),
                other => panic!("{property} is {other:?}"),
            };
        }
    }
    panic!("no Terrain");
}

#[test]
#[ignore = "needs RBX_TERRAIN_FIXTURE, a place with terrain"]
fn real_terrain_re_encodes_byte_for_byte() {
    let path = std::env::var("RBX_TERRAIN_FIXTURE").expect("RBX_TERRAIN_FIXTURE");
    let dom = rbx_binary::deserialize(&std::fs::read(path).unwrap()).unwrap();
    let smooth = terrain_blob(&dom, "SmoothGrid");
    let grid = rbx_terrain::smooth_grid::decode(&smooth).unwrap();
    assert!(!grid.is_empty(), "the fixture should hold terrain");
    assert_eq!(rbx_terrain::smooth_grid::encode(&grid), smooth);
    assert_eq!(
        rbx_terrain::physics_grid::encode(&grid),
        terrain_blob(&dom, "PhysicsGrid"),
        "PhysicsGrid regenerated from the voxels should match the stored one"
    );
    let colors = terrain_blob(&dom, "MaterialColors");
    assert_eq!(
        rbx_terrain::MaterialColors::decode(&colors)
            .unwrap()
            .encode(),
        colors
    );
}

#[test]
#[ignore = "needs RBX_TERRAIN_FIXTURE, a place with terrain"]
fn real_terrain_meshes_closed() {
    let path = std::env::var("RBX_TERRAIN_FIXTURE").expect("RBX_TERRAIN_FIXTURE");
    let dom = rbx_binary::deserialize(&std::fs::read(path).unwrap()).unwrap();
    let grid = rbx_terrain::smooth_grid::decode(&terrain_blob(&dom, "SmoothGrid")).unwrap();
    let started = std::time::Instant::now();
    let keys = rbx_terrain::mesh::meshable_chunks(&grid);
    let meshes: Vec<_> = keys
        .iter()
        .map(|k| rbx_terrain::mesh::mesh_chunk(&grid, *k))
        .collect();
    let triangles: usize = meshes
        .iter()
        .flat_map(|m| m.solids.iter())
        .map(|(_, s)| s.indices.len() / 3)
        .sum();
    eprintln!(
        "{} chunks, {triangles} triangles in {:?}",
        keys.len(),
        started.elapsed()
    );
    assert!(triangles > 1000);
}
