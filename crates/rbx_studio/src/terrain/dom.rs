//! Reading the voxels out of `Workspace.Terrain` and writing them back.
//!
//! Written as `Variant::Unknown { type_id: 1 }` whatever the bytes: that is
//! what keeps `SmoothGrid` a `BinaryString` in a `.rbxlx` save (a
//! `Variant::String` would come out as `<string>`), and the binary format
//! frames both the same way.

use rbx_dom::{Ref, Variant, WeakDom};
#[cfg(test)]
use rbx_terrain::VoxelGrid;

/// `Workspace.Terrain`, the only terrain Roblox draws.
pub(crate) fn find_terrain(dom: &WeakDom) -> Option<Ref> {
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.class() == "Workspace"))?;
    dom.get(workspace)?
        .children()
        .iter()
        .copied()
        .find(|&child| dom.get(child).is_some_and(|i| i.class() == "Terrain"))
}

/// What the terrain's `SmoothGrid` held.
#[cfg(test)]
#[derive(Debug)]
pub(crate) enum GridRead {
    Grid(VoxelGrid),
    /// Bytes this cannot read. Editing would throw them away, so the editor
    /// refuses to.
    Unreadable(String),
}

/// The terrain's `SmoothGrid` bytes as the DOM holds them (empty if none).
pub(crate) fn grid_bytes(dom: &WeakDom, terrain: Ref) -> &[u8] {
    match dom
        .get(terrain)
        .and_then(|i| i.properties().get("SmoothGrid"))
    {
        Some(Variant::String(text)) => text.as_bytes(),
        Some(Variant::Unknown { raw, .. }) => raw.as_slice(),
        _ => &[],
    }
}

#[cfg(test)]
pub(crate) fn read_grid(dom: &WeakDom, terrain: Ref) -> GridRead {
    let bytes = match dom
        .get(terrain)
        .and_then(|i| i.properties().get("SmoothGrid"))
    {
        Some(Variant::String(text)) => text.as_bytes(),
        Some(Variant::Unknown { raw, .. }) => raw.as_slice(),
        _ => return GridRead::Grid(VoxelGrid::new()),
    };
    if bytes.is_empty() {
        return GridRead::Grid(VoxelGrid::new());
    }
    match rbx_terrain::smooth_grid::decode(bytes) {
        Ok(grid) => GridRead::Grid(grid),
        Err(error) => GridRead::Unreadable(error.to_string()),
    }
}

fn blob(bytes: Vec<u8>) -> Variant {
    Variant::Unknown {
        type_id: 1,
        raw: bytes,
    }
}

/// Writes already-encoded `SmoothGrid` bytes (an editor keeps encoders
/// that only redo the chunks an edit touched), and the `PhysicsGrid`
/// Roblox saves beside them when given. A drag writes `PhysicsGrid` once at
/// the end rather than every step.
pub(crate) fn write_encoded(
    dom: &mut WeakDom,
    terrain: Ref,
    smooth: Vec<u8>,
    physics: Option<Vec<u8>>,
) -> Result<(), rbx_dom::DomError> {
    dom.set_property(terrain, "SmoothGrid", blob(smooth))?;
    if let Some(physics) = physics {
        dom.set_property(terrain, "PhysicsGrid", blob(physics))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn write_grid(
    dom: &mut WeakDom,
    terrain: Ref,
    grid: &VoxelGrid,
    physics: bool,
) -> Result<(), rbx_dom::DomError> {
    dom.set_property(
        terrain,
        "SmoothGrid",
        blob(rbx_terrain::smooth_grid::encode(grid)),
    )?;
    if physics {
        dom.set_property(
            terrain,
            "PhysicsGrid",
            blob(rbx_terrain::physics_grid::encode(grid)),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Instance;
    use rbx_terrain::{Cell, Material};

    fn place() -> (WeakDom, Ref) {
        let mut dom = WeakDom::new();
        let workspace = Ref::new(1);
        let terrain = Ref::new(2);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        dom.insert(Instance::new(terrain, "Terrain", "Terrain"));
        dom.set_parent(terrain, Some(workspace));
        (dom, terrain)
    }

    #[test]
    fn writes_round_trip_as_binary_strings() {
        let (mut dom, terrain) = place();
        assert_eq!(find_terrain(&dom), Some(terrain));
        let mut grid = VoxelGrid::new();
        grid.set([1, 2, 3], Cell::full(Material::Rock));
        write_grid(&mut dom, terrain, &grid, true).unwrap();
        let props = dom.get(terrain).unwrap().properties();
        assert!(matches!(
            props.get("SmoothGrid"),
            Some(Variant::Unknown { type_id: 1, .. })
        ));
        assert!(matches!(
            props.get("PhysicsGrid"),
            Some(Variant::Unknown { type_id: 1, .. })
        ));
        match read_grid(&dom, terrain) {
            GridRead::Grid(back) => assert_eq!(back, grid),
            GridRead::Unreadable(why) => panic!("{why}"),
        }
    }

    #[test]
    fn unreadable_bytes_are_reported_not_replaced() {
        let (mut dom, terrain) = place();
        dom.set_property(terrain, "SmoothGrid", blob(vec![7, 5]))
            .unwrap();
        assert!(matches!(read_grid(&dom, terrain), GridRead::Unreadable(_)));
        let (empty, terrain) = place();
        assert!(matches!(read_grid(&empty, terrain), GridRead::Grid(g) if g.is_empty()));
    }
}
