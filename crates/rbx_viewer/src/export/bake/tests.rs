use std::sync::Arc;

use super::*;
use crate::scene::srgb_to_linear;

fn image(width: u32, height: u32, texel: impl Fn(u32, u32) -> [u8; 4]) -> Arc<Image> {
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| texel(x, y))
        .collect();
    Arc::new(Image {
        width,
        height,
        pixels,
    })
}

fn pack(color: Option<Arc<Image>>, normal: Option<Arc<Image>>) -> Pack {
    Pack {
        maps: [color, normal, None, None],
        studs_per_tile: 4.0,
    }
}

/// A flat triangle in the plane through the origin facing `normal`, about
/// `size` studs across.
fn facet(normal: Vec3, size: f32) -> [Corner; 3] {
    let normal = normal.normalize();
    let u = normal.any_orthonormal_vector();
    let v = normal.cross(u);
    [Vec3::ZERO, u * size, v * size].map(|studs| Corner {
        studs: studs + Vec3::splat(0.3),
        unit_normal: normal,
        normal,
        uv: Vec2::new(studs.dot(u), studs.dot(v)) / size,
    })
}

fn texel(image: &Image, uv: [f32; 2]) -> [u8; 4] {
    let x = ((uv[0] * image.width as f32) as u32).min(image.width - 1);
    let y = ((uv[1] * image.height as f32) as u32).min(image.height - 1);
    let at = ((y * image.width + x) * 4) as usize;
    image.pixels[at..at + 4].try_into().unwrap()
}

/// The baked texel at the centroid of the first triangle.
fn centroid(baked: &Baked, map: &Image) -> [u8; 4] {
    let uv = (Vec2::from(baked.uvs[0]) + Vec2::from(baked.uvs[1]) + Vec2::from(baked.uvs[2])) / 3.0;
    texel(map, uv.to_array())
}

#[test]
fn a_box_face_is_one_projection_and_a_tilted_facet_is_not() {
    assert!(single_axis(&facet(Vec3::Y, 1.0)));
    assert!(single_axis(&facet(Vec3::NEG_X, 1.0)));
    // ~8 degrees off still takes the shader's fast path.
    assert!(single_axis(&facet(Vec3::new(0.0, 1.0, 0.1), 1.0)));
    assert!(!single_axis(&facet(Vec3::new(1.0, 1.0, 0.0), 1.0)));
    assert!(!single_axis(&facet(Vec3::ONE, 1.0)));
}

/// On a facet facing all three axes alike the shader takes a third of each
/// projection; the bake has to land on that mix, not on any one of them.
#[test]
fn a_tilted_facet_bakes_the_three_way_blend() {
    // Red rising and blue falling across the image, so every projection
    // reads a colour of its own.
    let map = image(256, 4, |x, _| [x as u8, 0, 255 - x as u8, 255]);
    let pack = pack(Some(map.clone()), None);
    // Moved so the three projections read far-apart columns.
    let triangle = facet(Vec3::ONE, 1.0).map(|corner| Corner {
        studs: corner.studs + Vec3::new(0.0, 1.3, 2.6),
        ..corner
    });

    let baked = bake(&[triangle], &pack, None).remove(0);

    // Each leg's own projection of the centroid, as `sample_axis` takes it.
    let studs = (triangle[0].studs + triangle[1].studs + triangle[2].studs) / 3.0;
    let legs: Vec<Vec3> = [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .map(|axis| {
            let (u, v) = face_frame(axis);
            let uv = [studs.dot(u) / 4.0, studs.dot(v) / 4.0].map(|c| c.rem_euclid(1.0));
            let [r, g, b, _] = texel(&map, uv);
            (Vec3::new(r as f32, g as f32, b as f32) / 255.0).map(srgb_to_linear)
        })
        .collect();
    let expected = legs.iter().sum::<Vec3>() / 3.0;
    let [r, g, b, _] = centroid(&baked, &baked.color);
    let got = Vec3::new(r as f32, g as f32, b as f32) / 255.0;
    let got = got.map(srgb_to_linear);
    assert!(
        (got - expected).abs().max_element() < 0.03,
        "baked {got} where the shader mixes {expected}"
    );
    // Not any one projection alone.
    for leg in legs {
        assert!(
            (got - leg).abs().max_element() > 0.05,
            "{got} is one leg, {leg}"
        );
    }
}

/// A pack with a flat normal map changes nothing: the baked map decodes to
/// the surface's own normal in the chart's frame.
#[test]
fn a_flat_normal_map_bakes_flat() {
    let flat = image(4, 4, |_, _| [128, 128, 255, 255]);
    let baked = bake(
        &[facet(Vec3::new(1.0, 2.0, 3.0), 2.0)],
        &pack(None, Some(flat)),
        None,
    )
    .remove(0);

    let [x, y, z, _] = centroid(&baked, &baked.normal);
    assert!(
        (x as i32 - 128).abs() <= 2 && (y as i32 - 128).abs() <= 2 && z >= 253,
        "{x} {y} {z}"
    );
}

/// The tangent each baked vertex carries runs along its chart's U, and the
/// bitangent `normal × tangent` up the image (V shrinking), as glTF reads a
/// normal map.
#[test]
fn baked_tangents_follow_the_chart() {
    let baked = bake(
        &[facet(Vec3::new(0.3, 1.0, -0.7), 3.0)],
        &pack(None, None),
        None,
    )
    .remove(0);

    let [a, b, c] = [0, 1, 2].map(|i| (baked.positions[i], Vec2::from(baked.uvs[i])));
    let tangent = baked.tangents[0];
    let normal = baked.normals[0];
    // Gradients of U and V across the triangle, in studs.
    let (e1, e2) = (b.0 - a.0, c.0 - a.0);
    let (d1, d2) = (b.1 - a.1, c.1 - a.1);
    let r = 1.0 / d1.perp_dot(d2);
    let along_u = (e1 * d2.y - e2 * d1.y) * r;
    let along_v = (e2 * d1.x - e1 * d2.x) * r;
    assert!(along_u.normalize().dot(tangent) > 0.999);
    assert!(along_v.normalize().dot(normal.cross(tangent)) < -0.999);
}

/// A mesh's own image multiplies the pack texel for texel, its alpha kept.
#[test]
fn a_mesh_image_multiplies_the_pack() {
    let white = image(8, 8, |_, _| [255, 255, 255, 255]);
    let half = image(8, 8, |_, _| [188, 188, 188, 128]);
    let baked = bake(
        &[facet(Vec3::ONE, 1.0)],
        &pack(Some(white), None),
        Some(&half),
    )
    .remove(0);

    let [r, _, _, a] = centroid(&baked, &baked.color);
    assert!((r as i32 - 188).abs() <= 2, "{r}");
    assert!((a as i32 - 128).abs() <= 1, "{a}");
}

/// Every chart lies inside the atlas and no two overlap.
#[test]
fn charts_are_packed_apart() {
    let triangles: Vec<[Corner; 3]> = (0..40)
        .map(|i| {
            facet(
                Vec3::new(1.0, i as f32 * 0.1 - 2.0, 0.5),
                0.5 + i as f32 * 0.05,
            )
        })
        .collect();
    let baked = bake(&triangles, &pack(None, None), None).remove(0);

    let boxes: Vec<(Vec2, Vec2)> = baked
        .uvs
        .chunks(3)
        .map(|uv| {
            let p = uv.iter().map(|&u| Vec2::from(u));
            (
                p.clone().fold(Vec2::MAX, Vec2::min),
                p.fold(Vec2::MIN, Vec2::max),
            )
        })
        .collect();
    for (i, (min, max)) in boxes.iter().enumerate() {
        assert!(min.cmpge(Vec2::ZERO).all() && max.cmple(Vec2::ONE).all());
        for (other_min, other_max) in &boxes[i + 1..] {
            let apart = max.x <= other_min.x
                || other_max.x <= min.x
                || max.y <= other_min.y
                || other_max.y <= min.y;
            assert!(
                apart,
                "charts {min}..{max} and {other_min}..{other_max} overlap"
            );
        }
    }
}

/// A facet far larger than one page bakes at the pack's own density anyway:
/// it is split and spread over several power-of-two pages, never coarsened.
#[test]
fn an_oversized_facet_spills_onto_pages_at_full_density() {
    let map = image(512, 512, |x, y| [x as u8, y as u8, 0, 255]);
    let pack = Pack {
        maps: [Some(map), None, None, None],
        studs_per_tile: 1.0,
    };
    let pages = bake(&[facet(Vec3::ONE, 6.0)], &pack, None);

    assert!(pages.len() > 1, "{} page(s)", pages.len());
    for page in &pages {
        let size = page.color.width;
        assert!(size.is_power_of_two() && size <= atlas::PAGE_SIZE);
        assert_eq!(page.color.height, size);
        for triangle in page.uvs.chunks(3).zip(page.positions.chunks(3)) {
            let (uv, at) = triangle;
            let uv: Vec<Vec2> = uv.iter().map(|&c| Vec2::from(c) * size as f32).collect();
            let texels = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs();
            let studs = (at[1] - at[0]).cross(at[2] - at[0]).length();
            let density = (texels / studs).sqrt();
            assert!((density - 512.0).abs() < 1.0, "{density} texels per stud");
        }
    }
}
