//! The flat face of a mesh a ray meets, outlined for the summoned handles to
//! snap onto.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use rbx_mesh::Mesh;

use super::{nearest, Local, Ray};

/// The flat face of a mesh a ray meets: the outline of the triangles lying
/// in one plane with the one it hits, reached from it across shared edges, in
/// world space — keeping only the outline's real edges (see [`CREASE`]). A
/// curved region, where the outline is all soft edges, has no corners or
/// sides at all.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatFace {
    /// Where real sides meet and the outline turns: not every vertex along
    /// it — a vertex only splitting a straight side is left out — nor the end
    /// of a real side running into a soft one.
    pub corners: Vec<Vec3>,
    /// The outline's real sides: the edges only one of the face's triangles
    /// has, which leaves out the diagonals between them, and of those only
    /// the creases and open boundaries.
    pub sides: Vec<[Vec3; 2]>,
}

/// How far a neighbour's normal may turn from the hit triangle's and still
/// count as the same flat face: about 1.8 degrees.
const FLAT: f32 = 0.9995;

/// How far the surface has to turn across an outline edge, between the two
/// triangles' normals, for the edge to be a real one to snap onto: 30
/// degrees. Roblox publishes no such angle; this is this editor's choice. A
/// box's, wedge's or tetrahedron's edges turn 45–110 degrees, while a
/// mesh sphere or a character's body turns a few degrees per triangle — a
/// sphere would need fewer than 12 segments round to reach it.
const CREASE: f32 = 0.866;

/// ponytail: the walk stops after this many triangles, so a key press on a
/// huge flat region can't stall; past it the outline is the walked part's,
/// whose sides inside the face are soft and so dropped. Raise it, or cache
/// adjacency per mesh, if a real place's flat faces outgrow it.
const FACE_CEILING: usize = 4096;

/// The [`FlatFace`] `ray` first meets on `mesh` carried through `model`.
pub(in crate::pick) fn flat_face(mesh: &Mesh, model: Mat4, ray: Ray) -> Option<FlatFace> {
    let local = Local::of(model, ray)?;
    let (.., hit) = nearest(mesh, &local)?;
    let triangles = mesh.lod0().as_chunks::<3>().0;
    let corners_of = |triangle: &[u32; 3]| -> Option<[Vec3; 3]> {
        let world = |index: u32| {
            let vertex = mesh.vertices.get(index as usize)?;
            Some(model.transform_point3(Vec3::from(vertex.position)))
        };
        Some([
            world(triangle[0])?,
            world(triangle[1])?,
            world(triangle[2])?,
        ])
    };
    let seed = corners_of(triangles.get(hit)?)?;
    let normal = (seed[1] - seed[0])
        .cross(seed[2] - seed[0])
        .try_normalize()?;
    let size = (model.x_axis + model.y_axis + model.z_axis)
        .truncate()
        .length();
    let tolerance = 1e-4 * size.max(1.0);
    // Vertices are told apart by where they stand, not by index: a mesh splits
    // a vertex wherever its UVs or normals do, which says nothing about
    // whether two triangles touch.
    let key = |point: Vec3| (point / tolerance).round().to_array().map(|c| c as i64);
    let edge = |a: Vec3, b: Vec3| {
        let (a, b) = (key(a), key(b));
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    };

    // Every triangle in the hit one's plane, and which of them share an edge;
    // and, for the outline's edges, every other triangle on each edge: where
    // its edge starts, in its own winding, and its unit normal.
    let mut flat: Vec<[Vec3; 3]> = Vec::new();
    let mut by_edge: HashMap<_, Vec<usize>> = HashMap::new();
    let mut across: HashMap<_, Vec<([i64; 3], Vec3)>> = HashMap::new();
    for triangle in triangles {
        let Some(points) = corners_of(triangle) else {
            continue;
        };
        let Some(facing) = (points[1] - points[0])
            .cross(points[2] - points[0])
            .try_normalize()
        else {
            continue;
        };
        let level = points
            .iter()
            .all(|&p| (p - seed[0]).dot(normal).abs() <= tolerance);
        if facing.dot(normal).abs() < FLAT || !level {
            for i in 0..3 {
                let (a, b) = (points[i], points[(i + 1) % 3]);
                across.entry(edge(a, b)).or_default().push((key(a), facing));
            }
            continue;
        }
        for i in 0..3 {
            by_edge
                .entry(edge(points[i], points[(i + 1) % 3]))
                .or_default()
                .push(flat.len());
        }
        flat.push(points);
    }
    let start = flat.iter().position(|points| *points == seed)?;

    // Out from the hit triangle across shared edges.
    let mut reached = vec![false; flat.len()];
    reached[start] = true;
    let mut queue = vec![start];
    let mut walked = Vec::new();
    while let Some(at) = queue.pop() {
        walked.push(at);
        if walked.len() >= FACE_CEILING {
            break;
        }
        let points = flat[at];
        for i in 0..3 {
            for &next in &by_edge[&edge(points[i], points[(i + 1) % 3])] {
                if !std::mem::replace(&mut reached[next], true) {
                    queue.push(next);
                }
            }
        }
    }

    // The outline is the edges just one walked triangle has, each kept with
    // that triangle's own normal.
    let mut count: HashMap<_, (usize, [Vec3; 2], Vec3)> = HashMap::new();
    for &at in &walked {
        let points = flat[at];
        let facing = (points[1] - points[0]).cross(points[2] - points[0]);
        for i in 0..3 {
            let (a, b) = (points[i], points[(i + 1) % 3]);
            count
                .entry(edge(a, b))
                .or_insert((0, [a, b], facing.normalize()))
                .0 += 1;
        }
    }
    // An outline edge is real when nothing lies across it (an open boundary)
    // or what does turns away by more than `CREASE`. A neighbour running the
    // shared edge the same way round is wound against this triangle, so its
    // normal is flipped first: that makes the turn independent of how either
    // triangle happens to be wound, and tells a knife-edge fold (normals
    // nearly opposite) from a flat continuation.
    let sides: Vec<[Vec3; 2]> = count
        .into_iter()
        .filter(|&(shared, (seen, [a, _], facing))| {
            seen == 1
                && across.get(&shared).is_none_or(|others| {
                    others.iter().any(|&(from, other)| {
                        let other = if from == key(a) { -other } else { other };
                        facing.dot(other) < CREASE
                    })
                })
        })
        .map(|(_, (_, side, _))| side)
        .collect();

    // A corner is where two real sides meet and the outline turns.
    let mut ends: HashMap<_, (Vec3, Vec<Vec3>)> = HashMap::new();
    for &[a, b] in &sides {
        ends.entry(key(a)).or_insert((a, Vec::new())).1.push(b - a);
        ends.entry(key(b)).or_insert((b, Vec::new())).1.push(a - b);
    }
    let corners = ends
        .into_values()
        .filter(|(_, ways)| match ways.as_slice() {
            [_] => false,
            [one, other] => one.normalize_or_zero().dot(other.normalize_or_zero()) > -0.9999,
            _ => true,
        })
        .map(|(point, _)| point)
        .collect();
    Some(FlatFace { corners, sides })
}
