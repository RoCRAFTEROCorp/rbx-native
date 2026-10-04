//! Test-only looks inside a boolean's result: its connected pieces, and why
//! it does or does not pass `repair::is_watertight`.

use std::collections::HashMap;

use glam::DVec3;

use super::repair::{clean, weld_t_junctions, DirectedEdge, WELD_EPSILON};
use super::{evaluate_node, Node, Solid};

/// Test-only: the same shared-edge grouping [`discard_disconnected_debris`]
/// does, without the filtering — so a test can assert a result is (or, for
/// `discard_disconnected_debris`'s own tests, was) a single connected piece.
#[cfg(test)]
pub(in crate::scene::union) fn connected_components(solid: &Solid) -> Vec<f64> {
    let key = |v: DVec3| {
        (
            (v.x / WELD_EPSILON).round() as i64,
            (v.y / WELD_EPSILON).round() as i64,
            (v.z / WELD_EPSILON).round() as i64,
        )
    };
    let polygons = &solid.polygons;
    let mut parent: Vec<usize> = (0..polygons.len()).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    let mut edge_owner: HashMap<DirectedEdge, usize> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let n = polygon.vertices.len();
        for e in 0..n {
            let a = key(polygon.vertices[e]);
            let b = key(polygon.vertices[(e + 1) % n]);
            let edge = if a <= b { (a, b) } else { (b, a) };
            match edge_owner.get(&edge) {
                Some(&owner) => {
                    let (ra, rb) = (find(&mut parent, owner), find(&mut parent, i));
                    if ra != rb {
                        parent[ra] = rb;
                    }
                }
                None => {
                    edge_owner.insert(edge, i);
                }
            }
        }
    }
    let mut volume: HashMap<usize, f64> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let root = find(&mut parent, i);
        let first = polygon.vertices[0];
        let v: f64 = polygon.vertices[1..]
            .windows(2)
            .map(|pair| first.dot(pair[0].cross(pair[1])))
            .sum();
        *volume.entry(root).or_insert(0.0) += v / 6.0;
    }
    volume.into_values().collect()
}

/// Test-only: why a tree's result is not watertight — how many directed
/// edges, how many lack their reverse, how many repeat, and how many
/// distinct vertices the weld saw.
#[cfg(test)]
pub(in crate::scene::union) fn leak_report(root: &Node) -> String {
    let mut cache = Vec::new();
    let Ok(solid) = evaluate_node(root, &mut cache) else {
        return "boolean failed".into();
    };
    let polygons = clean(weld_t_junctions(clean(solid.polygons)));
    let key = |v: DVec3| {
        (
            (v.x / WELD_EPSILON).round() as i64,
            (v.y / WELD_EPSILON).round() as i64,
            (v.z / WELD_EPSILON).round() as i64,
        )
    };
    let mut directed: HashMap<DirectedEdge, u32> = HashMap::new();
    let mut vertices = std::collections::HashSet::new();
    for polygon in &polygons {
        let n = polygon.vertices.len();
        for i in 0..n {
            vertices.insert(key(polygon.vertices[i]));
            let edge = (key(polygon.vertices[i]), key(polygon.vertices[(i + 1) % n]));
            *directed.entry(edge).or_insert(0) += 1;
        }
    }
    let missing = directed
        .iter()
        .filter(|&(&(a, b), &count)| directed.get(&(b, a)).copied() != Some(count))
        .count();
    let repeated = directed.values().filter(|&&count| count > 1).count();
    let degenerate = directed.keys().filter(|&&(a, b)| a == b).count();
    if std::env::var("RBX_UNION_SURVEY_DUMP").is_ok() {
        for polygon in &polygons {
            let ring: Vec<String> = polygon
                .vertices
                .iter()
                .map(|v| format!("({:.5},{:.5},{:.5})", v.x, v.y, v.z))
                .collect();
            println!(
                "    n=({:.4},{:.4},{:.4}) {}",
                polygon.plane.normal.x,
                polygon.plane.normal.y,
                polygon.plane.normal.z,
                ring.join(" ")
            );
        }
        for (&(a, b), &count) in &directed {
            if count > 1 || a == b || !directed.contains_key(&(b, a)) {
                println!("    bad edge {a:?} -> {b:?} x{count}");
            }
        }
    }
    let volume = Solid {
        polygons: polygons.clone(),
    }
    .volume();
    format!(
        "polygons {} edges {} unbalanced {} repeated {} degenerate {} vertices {} volume {volume:.4} additive bound {:.4}",
        polygons.len(),
        directed.len(),
        missing,
        repeated,
        degenerate,
        vertices.len(),
        super::additive_volume_bound(root)
    )
}
