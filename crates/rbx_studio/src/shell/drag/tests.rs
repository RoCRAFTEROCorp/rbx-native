//! A drag step's DOM writes, against the view's own running answer: what is
//! written has to read back as what was on screen, or the selection jumps
//! when the drag ends and the view re-reads the DOM.

use glam::{Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{apply, vector, CFRAME_PROPERTY, SIZE_PROPERTY};
use crate::transform::{self, Targets};

fn at(x: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y: 0.0, z: 0.0 },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

/// A model of two unit cubes at x = 0 and x = 4, its own `WorldPivot` a
/// stud above the near one.
fn model() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let mut above = at(0.0);
    above.position.y = 1.0;
    dom.set_property(model, "WorldPivot", Variant::CFrame(above))
        .unwrap();
    for x in [0.0, 4.0] {
        let part = dom.new_instance("Part", "Part", Some(model));
        dom.set_property(part, CFRAME_PROPERTY, Variant::CFrame(at(x)))
            .unwrap();
        let unit = Vector3Data {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
        dom.set_property(part, SIZE_PROPERTY, Variant::Vector3(unit))
            .unwrap();
    }
    (dom, model)
}

/// A whole model scaled about the far end of its box, as the view scales it,
/// and written as `Shell::resize_parts` writes it.
fn scaled(pivot: bool) -> (Mat4, Option<Mat4>) {
    let db = ReflectionDatabase::embedded();
    let (mut dom, model) = model();
    let held = Targets::read(&dom, &db, &[model]);
    let mut view = held.clone();
    let parts = view.scale_about(&held, Vec3::new(4.5, 0.0, 0.0), 2.0);
    let writes: Vec<(Ref, &str, String)> = parts
        .iter()
        .flat_map(|&(referent, size, position)| {
            [
                (referent, SIZE_PROPERTY, vector(size)),
                (referent, CFRAME_PROPERTY, vector(position)),
            ]
        })
        .collect();
    let followers = rbx_lua::pivot::followers(&dom, &db, &[model]);
    let to = view.pivot().unwrap();
    let pivot = pivot.then(|| (model, transform::cframe(to)));
    apply(&mut dom, &db, &writes, &followers, pivot).unwrap();
    (to, Targets::read(&dom, &db, &[model]).pivot())
}

fn position(frame: Mat4) -> Vec3 {
    frame.w_axis.truncate()
}

#[test]
fn a_group_scale_writes_the_pivot_the_view_showed() {
    let (shown, written) = scaled(true);
    assert!((position(shown) - position(written.unwrap())).length() < 1e-5);
    // Scaled out from x = 4.5 by two, a stud up doubled too.
    assert!((position(shown) - Vec3::new(-4.5, 2.0, 0.0)).length() < 1e-5);
}

/// The jump this fixes: carried rigidly by its first part, the pivot reads
/// back somewhere the view never drew it.
#[test]
fn carrying_it_with_one_part_alone_would_jump() {
    let (shown, written) = scaled(false);
    assert!((position(shown) - position(written.unwrap())).length() > 0.5);
}
