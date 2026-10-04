//! The texel budget a whole export's bakes are kept to.

use super::*;

/// What every baked slope's pages cover, in texels, each page once.
fn baked_texels(exported: &Export) -> u64 {
    let mut seen = std::collections::HashSet::new();
    exported
        .meshes
        .iter()
        .filter(|mesh| !mesh.tangents.is_empty() && mesh.indices.len() == 6)
        .filter_map(|mesh| mesh.maps.color)
        .filter(|&index| seen.insert(index))
        .map(|index| {
            let (width, height) = size_of(&exported.textures[index]);
            u64::from(width) * u64::from(height)
        })
        .sum()
}

fn size_of(png: &[u8]) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let reader = decoder.read_info().unwrap();
    (reader.info().width, reader.info().height)
}

/// Two wedges of different sizes, each with a 45-degree slope to bake, the
/// checker pack tiling every `studs_per_tile`.
fn two_slopes(studs_per_tile: f32) -> (WeakDom, Ref, Meshes) {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Pair", None);
    let small = part(&mut dom, "WedgePart", [4.0, 2.0, 2.0]);
    let large = part(&mut dom, "WedgePart", [8.0, 4.0, 4.0]);
    dom.set_parent(small, Some(model));
    dom.set_parent(large, Some(model));
    let pack = Arc::new(Pack {
        studs_per_tile,
        ..checker()
    });
    let meshes = Meshes::default()
        .with_materials(HashMap::from([(small, Arc::clone(&pack)), (large, pack)]));
    (dom, model, meshes)
}

/// Under the budget every bake keeps the pack's own density: the checker
/// is 4 texels across 2 studs, so a slope's chart spans 2 texels a stud.
#[test]
fn under_the_texel_budget_a_bake_keeps_the_packs_own_density() {
    let (dom, model, meshes) = two_slopes(2.0);

    let exported = within(
        &dom,
        &ReflectionDatabase::embedded(),
        &meshes,
        &[model],
        u64::MAX,
    );

    let slope = exported
        .meshes
        .iter()
        .find(|mesh| !mesh.tangents.is_empty() && mesh.indices.len() == 6)
        .unwrap();
    let (width, _) = size_of(&exported.textures[slope.maps.color.unwrap()]);
    let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(slope.positions[i]));
    let [ta, tb, tc] = [0, 1, 2].map(|i| glam::Vec2::from(slope.uvs[i]) * width as f32);
    let density = ((tb - ta).perp_dot(tc - ta).abs() / (b - a).cross(c - a).length()).sqrt();
    assert!((density - 2.0).abs() < 0.01, "{density} texels per stud");
}

/// Over the budget every bake is coarsened by one factor until all of them
/// together fit. A dense pack (80 texels a stud), so the pages are large
/// enough to shrink.
#[test]
fn over_the_texel_budget_every_bake_shrinks_to_fit() {
    let (dom, model, meshes) = two_slopes(0.05);
    let database = ReflectionDatabase::embedded();
    let full = baked_texels(&within(&dom, &database, &meshes, &[model], u64::MAX));
    let budget = full / 4;

    let squeezed = baked_texels(&within(&dom, &database, &meshes, &[model], budget));

    assert!(
        squeezed <= budget,
        "{squeezed} texels over a budget of {budget}"
    );
    assert!(squeezed > 0);
}
