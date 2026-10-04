//! Wavefront `.obj`: one `o` object per part, positions and normals, faces
//! indexed 1-based across the whole file. No `.mtl` — colour lives in the
//! glTF export, and an `.obj` that names a material file it does not ship is
//! a warning in every importer.

use std::fmt::Write as _;

use super::ExportMesh;

pub fn obj(meshes: &[ExportMesh]) -> String {
    let mut out = String::from("# Exported by rbxstudio; units are studs, Y up\n");
    let mut base = 1;
    for mesh in meshes {
        // Most importers split an `o` line on whitespace and keep the first
        // word, so a part called "Red Brick" would arrive as "Red".
        let name: String = mesh
            .name
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect();
        let _ = writeln!(out, "o {name}");
        for [x, y, z] in &mesh.positions {
            let _ = writeln!(out, "v {x} {y} {z}");
        }
        for [x, y, z] in &mesh.normals {
            let _ = writeln!(out, "vn {x} {y} {z}");
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| index as usize + base);
            let _ = writeln!(out, "f {a}//{a} {b}//{b} {c}//{c}");
        }
        base += mesh.positions.len();
    }
    out
}
