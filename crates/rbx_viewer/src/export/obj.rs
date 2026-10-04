//! Wavefront `.obj`: one `o` object per part, positions, UVs where the part
//! has an image, and normals, faces indexed 1-based across the whole file.
//! Its materials go in a companion `.mtl`, one per part: the part's colour,
//! and its image as a `.png` beside both when it is drawn with one — the
//! three files [`obj_files`] names together. An image several parts share
//! is one `.png` that each of their materials names.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::gltf::{FORCE_FIELD_GLOW, FORCE_FIELD_OPACITY, GLASS_IOR};
use super::{Export, ExportMesh, Finish};

/// Every file an `.obj` export is, named for `stem` (the `.obj`'s own file
/// name without its extension): the `.obj` itself, its `.mtl`, and one
/// `.png` per distinct image the parts are drawn with, all meant for the
/// same directory. Only the `.obj` keeps a space in `stem`: the others are
/// named from inside it.
pub fn obj_files(export: &Export, stem: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![
        (
            format!("{stem}.obj"),
            obj(&export.meshes, stem).into_bytes(),
        ),
        (
            format!("{}.mtl", no_spaces(stem)),
            mtl(&export.meshes, stem).into_bytes(),
        ),
    ];
    let used: BTreeSet<usize> = export
        .meshes
        .iter()
        .flat_map(|m| {
            [
                m.maps.color,
                m.maps.normal,
                m.maps.metalness,
                m.maps.roughness,
                m.maps.pattern,
            ]
        })
        .flatten()
        .collect();
    for index in used {
        files.push((
            texture_file(&no_spaces(stem), index),
            export.textures[index].clone(),
        ));
    }
    files
}

pub fn obj(meshes: &[ExportMesh], stem: &str) -> String {
    let mut out = String::from("# Exported by rbxstudio; units are studs, Y up\n");
    let _ = writeln!(out, "mtllib {}.mtl", no_spaces(stem));
    let mut base = 1;
    let mut uv_base = 1;
    for (index, mesh) in meshes.iter().enumerate() {
        let _ = writeln!(out, "o {}", no_spaces(&mesh.name));
        let _ = writeln!(out, "usemtl {}", material_name(mesh, index));
        for [x, y, z] in &mesh.positions {
            let _ = writeln!(out, "v {x} {y} {z}");
        }
        // OBJ's V runs up from the image's bottom row; Roblox's runs down
        // from its top.
        for [u, v] in &mesh.uvs {
            let _ = writeln!(out, "vt {u} {}", 1.0 - v);
        }
        for [x, y, z] in &mesh.normals {
            let _ = writeln!(out, "vn {x} {y} {z}");
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| index as usize + base);
            if mesh.uvs.is_empty() {
                let _ = writeln!(out, "f {a}//{a} {b}//{b} {c}//{c}");
            } else {
                let [ta, tb, tc] = triangle.map(|index| index as usize + uv_base);
                let _ = writeln!(out, "f {a}/{ta}/{a} {b}/{tb}/{b} {c}/{tc}/{c}");
            }
        }
        base += mesh.positions.len();
        uv_base += mesh.uvs.len();
    }
    out
}

/// The `.mtl` [`obj`] names: each part's colour as `Kd`, its transparency as
/// `d`, and its images, when it has them, as `map_Kd` and the rest.
pub fn mtl(meshes: &[ExportMesh], stem: &str) -> String {
    let stem = no_spaces(stem);
    let mut out = String::from("# Exported by rbxstudio\n");
    for (index, mesh) in meshes.iter().enumerate() {
        let [r, g, b, alpha] = mesh.color;
        let _ = writeln!(out, "\nnewmtl {}", material_name(mesh, index));
        let _ = writeln!(out, "Kd {r} {g} {b}");
        // OBJ has no strength for its emission, nor any Fresnel: Neon emits
        // its colour as is, a ForceField its mean rim glow (see `gltf`).
        match mesh.finish {
            Finish::Plain => {
                let _ = writeln!(out, "d {alpha}");
            }
            Finish::Neon => {
                let _ = writeln!(out, "d {alpha}\nKe {r} {g} {b}");
                if let Some(texture) = mesh.maps.color {
                    let _ = writeln!(out, "map_Ke {}", texture_file(&stem, texture));
                }
            }
            Finish::Glass => {
                let _ = writeln!(
                    out,
                    "d {alpha}\nTr {}\nNi {GLASS_IOR}\nillum 4",
                    1.0 - alpha
                );
            }
            Finish::ForceField => {
                let glow = |c: f32| c * FORCE_FIELD_GLOW;
                let _ = writeln!(
                    out,
                    "d {}\nKe {} {} {}",
                    (alpha * FORCE_FIELD_OPACITY).min(1.0),
                    glow(r),
                    glow(g),
                    glow(b)
                );
                // The pattern glows where it shows; OBJ has nothing for
                // the clear rest.
                if let Some(texture) = mesh.maps.pattern {
                    let _ = writeln!(out, "map_Ke {}", texture_file(&stem, texture));
                }
            }
        }
        // `map_Bump` rather than the PBR extension's `norm`: it is the key
        // Blender's importer (and most others) turns into a tangent-space
        // normal map. `map_Pm`/`map_Pr` are that extension's own.
        for (key, map) in [
            ("map_Kd", mesh.maps.color),
            ("map_Bump", mesh.maps.normal),
            ("map_Pm", mesh.maps.metalness),
            ("map_Pr", mesh.maps.roughness),
        ] {
            if let Some(texture) = map {
                let _ = writeln!(out, "{key} {}", texture_file(&stem, texture));
            }
        }
    }
    out
}

/// Indexed, since two parts often share a name.
fn material_name(mesh: &ExportMesh, index: usize) -> String {
    format!("{}_{index}", no_spaces(&mesh.name))
}

fn texture_file(stem: &str, index: usize) -> String {
    format!("{stem}_{index}.png")
}

/// Most importers split `o`, `mtllib` and `map_Kd` lines on whitespace and
/// keep the first word, so a part called "Red Brick" would arrive as "Red".
fn no_spaces(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect()
}
