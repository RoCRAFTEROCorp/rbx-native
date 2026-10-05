use super::*;
use rbx_dom::Instance;
use rbx_terrain::Cell;

fn place(grid: Option<&VoxelGrid>) -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    let mut terrain = Instance::new(Ref::new(2), "Terrain", "Terrain");
    if let Some(grid) = grid {
        terrain.properties_mut().insert(
            "SmoothGrid".into(),
            Variant::Unknown {
                type_id: 1,
                raw: rbx_terrain::smooth_grid::encode(grid),
            },
        );
    }
    dom.insert(terrain);
    dom.set_parent(Ref::new(2), Some(Ref::new(1)));
    dom
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn reads_the_voxels_and_claims_a_layer_per_material() {
    let mut grid = VoxelGrid::new();
    grid.set([0, 0, 0], Cell::full(Material::Grass));
    grid.set([1, 0, 0], Cell::full(Material::Rock));
    grid.set([2, 0, 0], Cell::full(Material::Water));
    let dom = place(Some(&grid));
    let database = database();
    let mut catalog = Catalog::new(&dom, &database);
    let terrain = Terrain::plan(&dom, &database, &mut catalog).unwrap();
    assert_eq!(terrain.grid, grid);
    assert!(terrain.error.is_none());
    // Plastic plus Grass and Rock; water takes no pack.
    assert_eq!(catalog.layers(), 3);
    assert_ne!(
        terrain.slot(&catalog, Material::Grass).layer,
        terrain.slot(&catalog, Material::Rock).layer
    );
    assert!(terrain.has_water());
    assert_eq!(terrain.water_slot().kind, Kind::Water);
    let extent = terrain.extent().unwrap();
    assert_eq!(extent.min, Vec3::ZERO);
    assert_eq!(extent.max, Vec3::splat(128.0));
}

#[test]
fn an_unreadable_grid_draws_nothing_and_says_why() {
    let mut dom = place(None);
    dom.get_mut(Ref::new(2)).unwrap().properties_mut().insert(
        "SmoothGrid".into(),
        Variant::Unknown {
            type_id: 1,
            raw: vec![9, 5],
        },
    );
    let database = database();
    let mut catalog = Catalog::new(&dom, &database);
    let terrain = Terrain::plan(&dom, &database, &mut catalog).unwrap();
    assert!(terrain.grid.is_empty());
    assert!(terrain.error.as_ref().unwrap().contains("version 9"));
    assert!(terrain.extent().is_none());
}

#[test]
fn defaults_match_a_fresh_place() {
    let dom = place(None);
    let database = database();
    let mut catalog = Catalog::new(&dom, &database);
    let terrain = Terrain::plan(&dom, &database, &mut catalog).unwrap();
    assert_eq!(terrain.water.transparency, 0.3);
    assert_eq!(terrain.water.wave_speed, 10.0);
    assert_eq!(terrain.colors, MaterialColors::default());
    assert!(!terrain.has_water());
}

#[test]
fn grass_follows_decoration_length_and_wind_without_a_reload() {
    let mut grid = VoxelGrid::new();
    grid.set([0, 0, 0], Cell::full(Material::Grass));
    let mut dom = place(Some(&grid));
    let database = database();
    let mut scene = crate::scene::Scene::from_dom(&dom, &database).unwrap();
    let grass = scene.terrain().unwrap().grass;
    assert!(grass.enabled);
    assert_eq!(grass.length, 0.7);
    assert_eq!(grass.wind, Vec3::ZERO);

    let terrain = dom.get_mut(Ref::new(2)).unwrap().properties_mut();
    terrain.insert("Decoration".into(), Variant::Bool(false));
    terrain.insert("GrassLength".into(), Variant::Float32(0.2));
    dom.get_mut(Ref::new(1)).unwrap().properties_mut().insert(
        "GlobalWind".into(),
        Variant::Vector3(rbx_dom::Vector3Data {
            x: 10.0,
            y: 0.0,
            z: 5.0,
        }),
    );
    let layers = scene.materials().layers();
    let changed = scene.resync_terrain(&dom, &database, layers).unwrap();
    // Nothing to re-mesh: the grass is redrawn from the chunks already there.
    assert!(changed.is_empty());
    let grass = scene.terrain().unwrap().grass;
    assert!(!grass.enabled);
    assert!((grass.length - 0.2).abs() < 1e-6);
    assert_eq!(grass.wind, Vec3::new(10.0, 0.0, 5.0));
}
