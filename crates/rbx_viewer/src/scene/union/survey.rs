//! A survey of every union in real places: how many carve, why the rest do
//! not, and whether what carved matches the extent Studio's own bake gave the
//! union (`InitialSize`). Ignored, since it reads places this repository does
//! not ship and downloads the union assets the on-disk cache lacks:
//!
//! `RBX_UNION_SURVEY_FIXTURE=a.rbxl:b.rbxl cargo test -p rbx_viewer --lib
//! union::survey -- --ignored --nocapture`

use std::collections::BTreeMap;

use rbx_assets::{AssetCache, AssetRef};
use rbx_dom::{Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{csg, frame, tree};

/// How far a carved extent may stray from `InitialSize`, relative to it,
/// before it counts as wrong: Studio rounds its own bake a little.
const TOLERANCE: f32 = 0.02;

fn fetch(cache: &AssetCache, asset: &AssetRef) -> Option<Vec<u8>> {
    let AssetRef::Id(id) = asset else {
        return None;
    };
    if let Some(bytes) = cache.get_id(*id) {
        return Some(bytes);
    }
    let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
    let bytes = client.asset(*id).ok()?.bytes;
    let _ = cache.put_id(*id, &bytes);
    Some(bytes)
}

fn survey(path: &str, database: &ReflectionDatabase, cache: &AssetCache) {
    let bytes = std::fs::read(path).expect("fixture must be readable");
    let dom: WeakDom = rbx_binary::deserialize(&bytes).expect("fixture must parse");
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut assets = tree::Assets::new();
    let mut off: Vec<String> = Vec::new();
    for referent in crate::scene::descendants(&dom) {
        let instance = dom.get(referent).expect("walked referent");
        if !database.is_subclass_of(instance.class(), "PartOperation") {
            continue;
        }
        if super::is_empty(&dom, database, referent) {
            *tally.entry("empty (draws nothing)".into()).or_default() += 1;
            continue;
        }
        let Some((asset, inline, ..)) = frame(&dom, database, referent) else {
            *tally.entry("no tree, but a baked mesh".into()).or_default() += 1;
            continue;
        };
        let raw = match inline {
            true => tree::child_data(instance.properties()).map(<[u8]>::to_vec),
            false => fetch(cache, &asset),
        };
        let Some(raw) = raw else {
            *tally.entry("asset unavailable".into()).or_default() += 1;
            continue;
        };
        let parsed = loop {
            let Some(parsed) = tree::parse(&raw, database, &assets) else {
                break None;
            };
            if parsed.missing.is_empty() {
                break Some(parsed);
            }
            for nested in parsed.missing {
                let bytes = fetch(cache, &nested).unwrap_or_default();
                assets.insert(nested, bytes);
            }
        };
        let Some(parsed) = parsed else {
            *tally.entry("tree did not parse".into()).or_default() += 1;
            println!(
                "  unparsed: {} {:?} {} bytes, starts {:?}",
                instance.name(),
                asset,
                raw.len(),
                String::from_utf8_lossy(&raw[..raw.len().min(16)])
            );
            continue;
        };
        let kind = if inline { "inline" } else { "asset" };
        let initial = match instance.properties().get("InitialSize") {
            Some(&Variant::Vector3(v)) => Some(glam::Vec3::new(v.x, v.y, v.z)),
            _ => None,
        };
        match csg::evaluate(&parsed.root, initial) {
            Ok(solid) => {
                *tally.entry(format!("carved ({kind})")).or_default() += 1;
                let mesh = solid.to_mesh();
                if let Some(initial) = initial {
                    // Studio's bake runs from -InitialSize/2 to +InitialSize/2
                    // in the union's own frame.
                    let initial = initial.to_array();
                    let wrong = (0..3).any(|axis| {
                        let slack = TOLERANCE * initial[axis].max(0.2);
                        (mesh.bounds.min[axis] + initial[axis] / 2.0).abs() > slack
                            || (mesh.bounds.max[axis] - initial[axis] / 2.0).abs() > slack
                    });
                    if wrong {
                        *tally
                            .entry("  of which off InitialSize".into())
                            .or_default() += 1;
                        off.push(format!(
                            "{} {:?}: carved {:?}..{:?} vs InitialSize {initial:?}",
                            instance.name(),
                            asset,
                            mesh.bounds.min,
                            mesh.bounds.max
                        ));
                    }
                }
            }
            Err(failure) => {
                *tally
                    .entry(format!("failed {failure:?} ({kind})"))
                    .or_default() += 1;
                println!(
                    "  failed {failure:?}: {} {:?} leaves {} — {}",
                    instance.name(),
                    asset,
                    parsed.root.leaf_count(),
                    csg::leak_report(&parsed.root)
                );
            }
        }
    }
    println!("{path}");
    for (what, count) in tally {
        println!("  {count:5}  {what}");
    }
    off.sort();
    off.dedup();
    for line in off {
        println!("    {line}");
    }
}

/// One union's tree and why its result leaks, polygon by polygon:
/// `RBX_UNION_SURVEY_DUMP=<asset id>`, or `<place.rbxl>#<union name>` for
/// the first union of that name in a place.
#[test]
#[ignore = "needs RBX_UNION_SURVEY_DUMP (an asset id, or place#union name)"]
fn dump_one_union() {
    let wanted = std::env::var("RBX_UNION_SURVEY_DUMP").expect("set RBX_UNION_SURVEY_DUMP");
    let database = ReflectionDatabase::embedded();
    let cache = AssetCache::new(None).expect("an asset cache directory");
    let raw = match wanted.split_once('#') {
        Some((path, name)) => {
            let bytes = std::fs::read(path).expect("place must be readable");
            let dom = rbx_binary::deserialize(&bytes).expect("place must parse");
            let referent = crate::scene::descendants(&dom)
                .find(|&r| {
                    dom.get(r).is_some_and(|i| {
                        i.name() == name && database.is_subclass_of(i.class(), "PartOperation")
                    })
                })
                .expect("a union of that name");
            let (asset, inline, ..) = frame(&dom, &database, referent).expect("a tree");
            match inline {
                true => tree::child_data(dom.get(referent).unwrap().properties())
                    .unwrap()
                    .to_vec(),
                false => fetch(&cache, &asset).expect("asset bytes"),
            }
        }
        None => {
            fetch(&cache, &AssetRef::Id(wanted.parse().expect("an asset id"))).expect("asset bytes")
        }
    };
    let parsed = tree::parse(&raw, &database, &tree::Assets::new()).expect("a tree");
    fn print(node: &tree::Node, depth: usize) {
        match node {
            tree::Node::Leaf(leaf) => println!(
                "{}leaf negate={} {:?} size={:?} cframe={:?}",
                "  ".repeat(depth),
                leaf.negate,
                leaf.geometry.kind,
                leaf.geometry.size,
                leaf.cframe.to_cols_array()
            ),
            tree::Node::Operation { negate, children } => {
                println!("{}op negate={negate}", "  ".repeat(depth));
                for child in children {
                    print(child, depth + 1);
                }
            }
        }
    }
    print(&parsed.root, 0);
    println!("{}", csg::leak_report(&parsed.root));
}

#[test]
#[ignore = "needs RBX_UNION_SURVEY_FIXTURE (colon-separated .rbxl paths) and network for uncached assets"]
fn survey_every_union_in_real_places() {
    let paths = std::env::var("RBX_UNION_SURVEY_FIXTURE")
        .expect("set RBX_UNION_SURVEY_FIXTURE to one or more .rbxl paths, colon-separated");
    let database = ReflectionDatabase::embedded();
    let cache = AssetCache::new(None).expect("an asset cache directory");
    for path in paths.split(':').filter(|path| !path.is_empty()) {
        survey(path, &database, &cache);
    }
}
