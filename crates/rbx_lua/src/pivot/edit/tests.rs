use rbx_dom::Vector3Data;

use super::*;
use crate::pivot::PRIMARY_PART;

const SIZE: &str = "Size";

fn at(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: IDENTITY.rotation,
    }
}

/// A quarter turn about +Y: +X goes to -Z.
fn turned(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
    }
}

fn close(a: CFrameData, b: CFrameData) -> bool {
    let (p, q) = (a.position, b.position);
    [p.x - q.x, p.y - q.y, p.z - q.z]
        .into_iter()
        .chain(a.rotation.iter().zip(b.rotation).map(|(x, y)| x - y))
        .all(|delta| delta.abs() < 1e-4)
}

fn property(dom: &WeakDom, reference: Ref, name: &str) -> Option<CFrameData> {
    match dom.get(reference)?.properties().get(name) {
        Some(Variant::CFrame(frame)) => Some(*frame),
        _ => None,
    }
}

/// A 2-stud cube at `frame`, under `parent`.
fn part(dom: &mut WeakDom, parent: Option<Ref>, frame: CFrameData) -> Ref {
    let part = dom.new_instance("Part", "Part", parent);
    dom.set_property(part, CFRAME, Variant::CFrame(frame))
        .unwrap();
    let size = Vector3Data {
        x: 2.0,
        y: 2.0,
        z: 2.0,
    };
    dom.set_property(part, SIZE, Variant::Vector3(size))
        .unwrap();
    part
}

/// A model holding two cubes, at x = 0 and x = 10, its pivot on `world`.
fn model(dom: &mut WeakDom, world: CFrameData) -> (Ref, Ref, Ref) {
    let model = dom.new_instance("Model", "Model", None);
    dom.set_property(model, WORLD_PIVOT, Variant::CFrame(world))
        .unwrap();
    let left = part(dom, Some(model), at(0.0, 0.0, 0.0));
    let right = part(dom, Some(model), at(10.0, 0.0, 0.0));
    (model, left, right)
}

#[test]
fn moving_a_parts_pivot_leaves_the_part_where_it_is() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let part = part(&mut dom, None, turned(4.0, 1.0, 0.0));

    let corner = turned(5.0, 2.0, 1.0);
    set_pivot(&mut dom, &db, part, &corner).unwrap().unwrap();

    assert!(close(pivot(&dom, &db, part).unwrap(), corner));
    assert_eq!(property(&dom, part, CFRAME), Some(turned(4.0, 1.0, 0.0)));
    // Stored in the part's own frame: the turn takes world +X to -Z, so a
    // pivot one stud along world +Z is one stud along the part's own -X.
    let offset = property(&dom, part, PIVOT_OFFSET).unwrap();
    assert!(close(offset, at(-1.0, 1.0, 1.0)), "{offset:?}");
}

#[test]
fn a_models_pivot_moves_by_its_world_pivot_and_no_part_moves() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, left, right) = model(&mut dom, at(5.0, 0.0, 0.0));

    set_pivot(&mut dom, &db, model, &turned(0.0, 3.0, 0.0))
        .unwrap()
        .unwrap();

    assert_eq!(pivot(&dom, &db, model), Some(turned(0.0, 3.0, 0.0)));
    assert_eq!(property(&dom, left, CFRAME), Some(at(0.0, 0.0, 0.0)));
    assert_eq!(property(&dom, right, CFRAME), Some(at(10.0, 0.0, 0.0)));
}

/// With a `PrimaryPart`, the model's pivot is that part's, so moving it
/// moves that part's offset — and the model reads the new place back.
#[test]
fn a_model_with_a_primary_part_moves_that_parts_pivot() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, _, right) = model(&mut dom, at(5.0, 0.0, 0.0));
    dom.set_property(model, PRIMARY_PART, Variant::Ref(right))
        .unwrap();

    set_pivot(&mut dom, &db, model, &at(10.0, 1.0, 1.0))
        .unwrap()
        .unwrap();

    assert!(close(pivot(&dom, &db, model).unwrap(), at(10.0, 1.0, 1.0)));
    assert_eq!(property(&dom, right, PIVOT_OFFSET), Some(at(0.0, 1.0, 1.0)));
    assert_eq!(property(&dom, model, WORLD_PIVOT), Some(at(5.0, 0.0, 0.0)));
}

#[test]
fn reset_puts_a_parts_pivot_back_on_its_centre() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let part = part(&mut dom, None, turned(4.0, 1.0, 0.0));
    set_pivot(&mut dom, &db, part, &at(9.0, 9.0, 9.0))
        .unwrap()
        .unwrap();

    reset(&mut dom, &db, part).unwrap().unwrap();

    assert_eq!(pivot(&dom, &db, part), Some(turned(4.0, 1.0, 0.0)));
}

#[test]
fn reset_puts_a_models_pivot_on_its_bounding_boxs_centre_facing_the_same_way() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, ..) = model(&mut dom, at(-20.0, 4.0, 0.0));

    reset(&mut dom, &db, model).unwrap().unwrap();
    assert_eq!(pivot(&dom, &db, model), Some(at(5.0, 0.0, 0.0)));

    // Turned, the box is squared to the pivot's axes — the same box here,
    // the cubes being square too — and the pivot keeps its turn.
    set_pivot(&mut dom, &db, model, &turned(0.0, 9.0, 0.0))
        .unwrap()
        .unwrap();
    reset(&mut dom, &db, model).unwrap().unwrap();
    assert!(close(
        pivot(&dom, &db, model).unwrap(),
        turned(5.0, 0.0, 0.0)
    ));
}

/// An 8-stud bar turned an eighth of a turn with a world-square cube 6 studs
/// out along it: squared to the bar, the cube reaches √2 either side of 6,
/// so the box runs from -4 to 6 + √2 along the bar and is centred on the
/// bar's own axis; squared to the world it is centred somewhere else.
#[test]
fn a_box_squared_to_a_turned_pivot_is_not_the_worlds() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let eighth = std::f32::consts::FRAC_1_SQRT_2;
    let rotation = [eighth, 0.0, eighth, 0.0, 1.0, 0.0, -eighth, 0.0, eighth];
    let bar = part(
        &mut dom,
        Some(model),
        CFrameData {
            position: IDENTITY.position,
            rotation,
        },
    );
    let size = Vector3Data {
        x: 8.0,
        y: 2.0,
        z: 2.0,
    };
    dom.set_property(bar, SIZE, Variant::Vector3(size)).unwrap();
    // The bar's own +X is world (1, 0, -1) / √2.
    part(&mut dom, Some(model), at(6.0 * eighth, 0.0, -6.0 * eighth));

    let own = bounds_centre(&dom, &db, model, &rotation).unwrap();
    assert_eq!(own.rotation, rotation);
    let (centre, out) = (own.position, (2.0 + 2.0f32.sqrt()) / 2.0);
    assert!((centre.x - out * eighth).abs() < 1e-4, "{centre:?}");
    assert!((centre.z + out * eighth).abs() < 1e-4, "{centre:?}");

    let world = bounds_centre(&dom, &db, model, &IDENTITY.rotation).unwrap();
    assert!((world.position.x - centre.x).abs() > 0.1, "{world:?}");
}

/// The docs' own scenario: assigning a `PrimaryPart` takes the pivot to
/// that part's, and deleting the part leaves it there rather than jumping
/// back to wherever the model's own pivot was before.
#[test]
fn deleting_a_primary_part_leaves_the_pivot_where_it_was() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, _, right) = model(&mut dom, at(5.0, 0.0, 0.0));
    dom.set_property(model, PRIMARY_PART, Variant::Ref(right))
        .unwrap();
    assert_eq!(pivot(&dom, &db, model), Some(at(10.0, 0.0, 0.0)));

    keep_pivots(&mut dom, &db, &[right]).unwrap();
    dom.remove(right);

    assert_eq!(pivot(&dom, &db, model), Some(at(10.0, 0.0, 0.0)));
}

/// What [`keep_pivots`] is for: without it the model falls back on a
/// `WorldPivot` that has not been its pivot since the part was assigned.
#[test]
fn without_keeping_it_the_pivot_would_jump_back() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, _, right) = model(&mut dom, at(5.0, 0.0, 0.0));
    dom.set_property(model, PRIMARY_PART, Variant::Ref(right))
        .unwrap();
    dom.remove(right);
    assert_eq!(pivot(&dom, &db, model), Some(at(5.0, 0.0, 0.0)));
}

/// Unassigning, unlike deleting, resets: the pivot goes from the primary
/// part's (x = 10) to the box's centre (x = 5), not back to the stale
/// `WorldPivot` (x = -20), and keeps the primary part's turn.
#[test]
fn clearing_a_primary_part_resets_the_pivot_to_the_boxs_centre() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, _, right) = model(&mut dom, at(-20.0, 4.0, 0.0));
    dom.set_property(right, CFRAME, Variant::CFrame(turned(10.0, 0.0, 0.0)))
        .unwrap();
    dom.set_property(model, PRIMARY_PART, Variant::Ref(right))
        .unwrap();

    let cleared = clear_primary_part(&mut dom, &db, model).unwrap();

    assert_eq!(cleared, Some(Variant::Ref(right)));
    assert!(dom
        .get(model)
        .unwrap()
        .properties()
        .get(PRIMARY_PART)
        .is_none());
    assert!(close(
        pivot(&dom, &db, model).unwrap(),
        turned(5.0, 0.0, 0.0)
    ));
}

/// Nothing to clear, nothing reset: the stored pivot stays.
#[test]
fn clearing_no_primary_part_leaves_the_pivot_alone() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, ..) = model(&mut dom, at(-20.0, 4.0, 0.0));

    assert_eq!(clear_primary_part(&mut dom, &db, model), Ok(None));
    assert_eq!(pivot(&dom, &db, model), Some(at(-20.0, 4.0, 0.0)));
}

#[test]
fn deleting_some_other_part_pins_nothing() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, left, right) = model(&mut dom, at(5.0, 0.0, 0.0));
    dom.set_property(model, PRIMARY_PART, Variant::Ref(right))
        .unwrap();

    keep_pivots(&mut dom, &db, &[left]).unwrap();

    assert_eq!(property(&dom, model, WORLD_PIVOT), Some(at(5.0, 0.0, 0.0)));
}

#[test]
fn a_models_pivot_follows_its_parts_moved_and_turned_one_by_one() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, left, right) = model(&mut dom, at(5.0, 0.0, 0.0));

    // A move: both parts three studs along +Z.
    let following = followers(&dom, &db, &[model]);
    assert_eq!(following.len(), 1);
    for (part, x) in [(left, 0.0), (right, 10.0)] {
        dom.set_property(part, CFRAME, Variant::CFrame(at(x, 0.0, 3.0)))
            .unwrap();
    }
    follow(&mut dom, &db, &following).unwrap();
    assert_eq!(pivot(&dom, &db, model), Some(at(5.0, 0.0, 3.0)));

    // A quarter turn about the left part's centre: the pivot, five studs
    // along +X from it, swings round to five along -Z, and turns too.
    let following = followers(&dom, &db, &[model]);
    dom.set_property(left, CFRAME, Variant::CFrame(turned(0.0, 0.0, 3.0)))
        .unwrap();
    follow(&mut dom, &db, &following).unwrap();
    assert!(close(
        pivot(&dom, &db, model).unwrap(),
        turned(0.0, 0.0, -2.0)
    ));
}

#[test]
fn a_model_with_a_primary_part_has_nothing_to_follow() {
    let db = ReflectionDatabase::embedded();
    let mut dom = WeakDom::new();
    let (model, left, _) = model(&mut dom, at(5.0, 0.0, 0.0));
    dom.set_property(model, PRIMARY_PART, Variant::Ref(left))
        .unwrap();
    assert!(followers(&dom, &db, &[model]).is_empty());
}
