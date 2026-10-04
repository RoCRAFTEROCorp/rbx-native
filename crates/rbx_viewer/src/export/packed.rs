//! A part whose `Material` has a texture pack: tiled straight from the
//! pack's own images on every face one projection covers, as
//! `renderer/material.wgsl`'s fast path draws it, and baked (see
//! [`super::bake`]) wherever the shader does more than one UV set can say —
//! a facet blending three projections, or a mesh multiplying the pack under
//! its own image.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{Vec2, Vec3};

use super::bake::{self, Corner};
use super::part::Geometry;
use super::{rigid, surface, transform_normals, transform_points, ExportMesh, Maps, Textures};
use crate::assets::Image;
use crate::pick::Pack;

/// One page of a bake: its triangles in the part's own studs and the maps
/// they read. A bake is every page, shared by every part with the same
/// shape, size and look.
pub(super) struct Bake {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    tangents: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    maps: Maps,
}

pub(super) fn dress(
    template: &ExportMesh,
    geometry: Geometry,
    pack: &Pack,
    image: Option<&Arc<Image>>,
    textures: &mut Textures,
) -> Vec<ExportMesh> {
    let (placement, extent) = rigid(geometry.model);
    let corner = |index: u32| {
        let i = index as usize;
        let unit_normal = Vec3::from(geometry.normals[i]);
        Corner {
            studs: Vec3::from(geometry.positions[i]) * extent,
            unit_normal,
            normal: (unit_normal / extent.max(Vec3::splat(f32::EPSILON))).normalize_or(Vec3::Y),
            uv: geometry.uvs.get(i).map_or(Vec2::ZERO, |&uv| Vec2::from(uv)),
        }
    };
    let triangles: Vec<[Corner; 3]> = geometry
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| triangle.map(corner))
        .collect();
    // A mesh's image has to be multiplied by the pack texel for texel, so
    // all of it bakes; a bare part bakes only what one projection misses.
    let (tiled, blended): (Vec<_>, Vec<_>) = triangles
        .into_iter()
        .partition(|triangle| image.is_none() && bake::single_axis(triangle));

    let mut out = Vec::new();
    if !tiled.is_empty() {
        let Projected {
            positions,
            normals,
            tangents,
            uvs,
        } = project(&tiled, extent, pack.studs_per_tile);
        let mut mesh = ExportMesh {
            indices: (0..positions.len() as u32).collect(),
            positions: transform_points(geometry.model, positions),
            normals: transform_normals(geometry.model, normals),
            tangents: tangents
                .iter()
                .map(|t| {
                    placement
                        .transform_vector3(t.truncate())
                        .extend(t.w)
                        .to_array()
                })
                .collect(),
            uvs,
            ..template.clone()
        };
        // The part's colour times the colour map, as `sample_axis` shades it.
        let [color, normal, metalness, roughness] = &pack.maps;
        mesh.maps.color = color.as_ref().and_then(|map| textures.of(map));
        surface::data_maps(&mut mesh, [normal, metalness, roughness], textures);
        out.push(mesh);
    }
    if !blended.is_empty() && textures.measuring {
        let key = bake_key(&blended, pack, image);
        textures.planned.entry(key).or_insert_with(|| Planned {
            triangles: blended,
            pack: pack.clone(),
            image: image.cloned(),
        });
    } else if !blended.is_empty() {
        let key = bake_key(&blended, pack, image);
        let pages = match textures.bakes.get(&key) {
            Some(pages) => Arc::clone(pages),
            None => {
                let pages = Arc::new(bake_maps(&blended, pack, image.map(Arc::as_ref), textures));
                textures.bakes.insert(key, Arc::clone(&pages));
                pages
            }
        };
        for baked in pages.iter() {
            let rotate = |v: &Vec3| placement.transform_vector3(*v);
            out.push(ExportMesh {
                indices: (0..baked.positions.len() as u32).collect(),
                positions: baked
                    .positions
                    .iter()
                    .map(|p| placement.transform_point3(*p).to_array())
                    .collect(),
                normals: baked.normals.iter().map(|n| rotate(n).to_array()).collect(),
                tangents: baked
                    .tangents
                    .iter()
                    .map(|t| rotate(t).extend(1.0).to_array())
                    .collect(),
                uvs: baked.uvs.clone(),
                maps: baked.maps,
                // The image's alpha rides in the baked colour map.
                blend: template.blend || image.is_some_and(|image| image.has_alpha()),
                ..template.clone()
            });
        }
    }
    out
}

/// A bake the export will make, kept from the first pass so the texel
/// budget can be weighed against all of them before any is shaded.
pub(super) struct Planned {
    triangles: Vec<[Corner; 3]>,
    pack: Pack,
    image: Option<Arc<Image>>,
}

impl Planned {
    pub(super) fn texels(&self, scale: f32) -> u64 {
        bake::texels(&self.triangles, &self.pack, self.image.as_deref(), scale)
    }
}

/// Everything a bake depends on: each corner's studs, normals and UV, the
/// pack and the image (by identity, as [`Textures`] pools them).
fn bake_key(triangles: &[[Corner; 3]], pack: &Pack, image: Option<&Arc<Image>>) -> u64 {
    let mut hasher = DefaultHasher::new();
    for corner in triangles.iter().flatten() {
        for value in [corner.studs, corner.unit_normal, corner.normal]
            .iter()
            .flat_map(|v| v.to_array())
            .chain(corner.uv.to_array())
        {
            value.to_bits().hash(&mut hasher);
        }
    }
    for map in &pack.maps {
        map.as_ref().map(Arc::as_ptr).hash(&mut hasher);
    }
    pack.studs_per_tile.to_bits().hash(&mut hasher);
    image.map(Arc::as_ptr).hash(&mut hasher);
    hasher.finish()
}

fn bake_maps(
    triangles: &[[Corner; 3]],
    pack: &Pack,
    image: Option<&Image>,
    textures: &mut Textures,
) -> Vec<Bake> {
    bake::bake(triangles, pack, image, textures.scale)
        .into_iter()
        .map(|baked| page_maps(baked, pack, image.is_some(), textures))
        .collect()
}

fn page_maps(baked: bake::Baked, pack: &Pack, has_image: bool, textures: &mut Textures) -> Bake {
    let [has_color, has_normal, has_metalness, has_roughness] =
        pack.maps.each_ref().map(Option::is_some);
    let mut maps = Maps::default();
    if has_color || has_image {
        maps.color = textures.fresh(&baked.color);
    }
    if has_normal {
        maps.normal = textures.fresh(&baked.normal);
    }
    if has_metalness {
        maps.metalness = textures.fresh(&baked.metalness);
    }
    if has_roughness {
        maps.roughness = textures.fresh(&baked.roughness);
    }
    if has_metalness || has_roughness {
        let metalness = has_metalness.then_some(&baked.metalness);
        let roughness = has_roughness.then_some(&baked.roughness);
        maps.metallic_roughness = textures.fresh(&surface::pack(metalness, roughness));
    }
    Bake {
        positions: baked.positions,
        normals: baked.normals,
        tangents: baked.tangents,
        uvs: baked.uvs,
        maps,
    }
}

/// [`project`]'s triangles, in the unit frame the caller's model matrix
/// places, with tangents in the part's own frame.
struct Projected {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<glam::Vec4>,
    uvs: Vec<[f32; 2]>,
}

/// The pack's UVs laid on the way `sample_axis` projects it along the one
/// axis each of these triangles faces: in studs along the part, one tile
/// every `studs_per_tile`. Unshared, three vertices a triangle, so two faces
/// meeting at an edge each keep their own projection. Each carries the
/// tangent the shader's own `tangent_normal` builds — image right, the
/// bitangent up the image — so a viewer reads the normal map in that frame
/// rather than guessing one from the UVs.
fn project(triangles: &[[Corner; 3]], extent: Vec3, studs_per_tile: f32) -> Projected {
    let tile = studs_per_tile.max(0.001);
    let mut out = Projected {
        positions: Vec::new(),
        normals: Vec::new(),
        tangents: Vec::new(),
        uvs: Vec::new(),
    };
    for triangle in triangles {
        let axis = bake::dominant_axis(triangle[0].unit_normal);
        let (u, v) = bake::face_frame(axis);
        let handedness = if axis.cross(u).dot(-v) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        for corner in triangle {
            // Back in the unit frame, for the caller's model matrix.
            out.positions
                .push((corner.studs / extent.max(Vec3::splat(f32::EPSILON))).to_array());
            out.normals.push(corner.unit_normal.to_array());
            out.tangents.push(u.extend(handedness));
            out.uvs
                .push([corner.studs.dot(u) / tile, corner.studs.dot(v) / tile]);
        }
    }
    out
}
