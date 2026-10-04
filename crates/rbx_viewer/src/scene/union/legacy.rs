//! Parts smaller than the engine could build when a union was baked.
//!
//! A union's tree keeps each part's `size` property as stored, which can be
//! below the smallest part the engine of the day actually built: Roblox's
//! minimum part size was 0.2 studs, then 0.05 from 2017, and only went lower
//! later. Studio carved such a part at the minimum, so a tree read as stored
//! comes out smaller than the bake. Real case: asset 7339267514 (28 unions in
//! FindTheCode.rbxl) holds a 0.0186-stud-wide sphere beside a 0.0547-stud
//! cylinder. As stored, they span 0.0640 studs on X; with the sphere at 0.05
//! they span 0.07973 — the union's `InitialSize.x` (0.0797348) to five
//! digits, while Y and Z already matched.
//!
//! Which minimum a given asset was baked under is not recorded anywhere, so
//! the union's own `InitialSize` (the extent of Studio's bake) decides: the
//! tree is read again at the 0.05 minimum only when one of its parts is
//! smaller, and kept that way only when its parts then span `InitialSize`
//! more closely than as stored. A union baked after the minimum dropped
//! matches as stored and is left alone.

use glam::{Mat4, Vec3};
use rbx_reflection::ReflectionDatabase;

use super::tree::{self, Assets, Node, Parsed};

/// The smallest part Roblox built from 2017 until the limit was lowered.
const LEGACY_MIN_SIZE: f32 = 0.05;

/// The tree in `bytes` as Studio carved it, judged against `bake` (the
/// union's `InitialSize`) where there is one.
pub(super) fn parse_as_baked(
    bytes: &[u8],
    database: &ReflectionDatabase,
    assets: &Assets,
    bake: Option<Vec3>,
) -> Option<Parsed> {
    let stored = tree::parse(bytes, database, assets)?;
    let Some(bake) = bake else {
        return Some(stored);
    };
    if !has_part_below(&stored.root, LEGACY_MIN_SIZE) {
        return Some(stored);
    }
    let Some(raised) = tree::parse_at_least(bytes, database, assets, LEGACY_MIN_SIZE) else {
        return Some(stored);
    };
    let miss = |root: &Node| {
        additive_extent(root).map_or(f32::INFINITY, |extent| (extent - bake).abs().max_element())
    };
    Some(if miss(&raised.root) < miss(&stored.root) {
        raised
    } else {
        stored
    })
}

fn has_part_below(node: &Node, min: f32) -> bool {
    match node {
        Node::Leaf(leaf) => leaf.properties.get("size").is_some_and(|size| match size {
            rbx_dom::Variant::Vector3(v) => v.x < min || v.y < min || v.z < min,
            _ => false,
        }),
        Node::Operation { children, .. } => children.iter().any(|child| has_part_below(child, min)),
    }
}

/// The size of the box around every additive part's own box, in the
/// union's frame: what a bake spans before anything is carved off it.
fn additive_extent(root: &Node) -> Option<Vec3> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    visit(root, &mut |model: Mat4| {
        for corner in 0..8 {
            let unit = Vec3::new(
                if corner & 1 == 0 { -0.5 } else { 0.5 },
                if corner & 2 == 0 { -0.5 } else { 0.5 },
                if corner & 4 == 0 { -0.5 } else { 0.5 },
            );
            let point = model.transform_point3(unit);
            bounds = Some(bounds.map_or((point, point), |(min, max)| {
                (min.min(point), max.max(point))
            }));
        }
    });
    bounds.map(|(min, max)| max - min)
}

fn visit(node: &Node, each: &mut impl FnMut(Mat4)) {
    match node {
        Node::Leaf(leaf) if !leaf.negate => each(leaf.model()),
        Node::Leaf(_) | Node::Operation { negate: true, .. } => {}
        Node::Operation { children, .. } => children.iter().for_each(|child| visit(child, each)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::union::tests_support::{inline_bytes, Leaf};

    /// Asset 7339267514's two parts, as boxes: the narrow one is stored
    /// below the 0.05-stud minimum it was built at.
    fn pair() -> Vec<u8> {
        inline_bytes(&[
            Leaf::additive(Vec3::new(0.012512, 0.0, 0.0), 1.0)
                .stretched(Vec3::new(0.0547249, 0.119742, 0.11924)),
            Leaf::additive(Vec3::new(-0.014854, 0.0, 0.0), 1.0)
                .stretched(Vec3::new(0.01857634, 0.119742, 0.11924)),
        ])
    }

    fn narrow_width(parsed: &Parsed) -> f32 {
        let Node::Operation { children, .. } = &parsed.root else {
            panic!("a root operation");
        };
        let Node::Leaf(leaf) = &children[1] else {
            panic!("a leaf");
        };
        leaf.geometry.size.x
    }

    #[test]
    fn a_part_below_the_old_minimum_is_built_at_it_when_the_bake_says_so() {
        let database = ReflectionDatabase::embedded();
        // The real union's `InitialSize`.
        let bake = Vec3::new(0.0797348, 0.119911, 0.11956);

        let parsed = parse_as_baked(&pair(), &database, &Assets::new(), Some(bake)).unwrap();

        assert_eq!(narrow_width(&parsed), LEGACY_MIN_SIZE);
        let extent = additive_extent(&parsed.root).unwrap();
        assert!((extent.x - bake.x).abs() < 1e-4, "{extent}");
    }

    #[test]
    fn a_union_baked_after_the_minimum_dropped_keeps_its_parts_as_stored() {
        let database = ReflectionDatabase::embedded();
        // What the parts span as stored: a bake that never raised them.
        let bake = Vec3::new(0.0640, 0.119742, 0.11924);

        let parsed = parse_as_baked(&pair(), &database, &Assets::new(), Some(bake)).unwrap();

        assert!((narrow_width(&parsed) - 0.01857634).abs() < 1e-6);
        // And without a bake to judge by, nothing is raised either.
        let unjudged = parse_as_baked(&pair(), &database, &Assets::new(), None).unwrap();
        assert!((narrow_width(&unjudged) - 0.01857634).abs() < 1e-6);
    }
}
