//! Where the selection stands for the transform tools: every part it
//! covers, and its pivot, read out of the DOM and carried through a drag.

use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::gizmo;
use rbx_viewer::pick;

use super::frame::{placement, rigid};

/// Where the handles stand: the selected part, and the matrix it is drawn
/// with.
///
/// Carried on the UI thread so a click can be hit-tested against the handles
/// without asking the render thread, which owns the scene and answers only
/// between frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Target {
    pub(crate) referent: Ref,
    pub(crate) model: Mat4,
    /// Whether this resolves to a `Ball` right now — read once, here, rather
    /// than re-resolved on every drag step: a Scale drag holds a `Target`
    /// snapshotted at the grab (see `Drag::Size`), and a part's `Shape`
    /// cannot change mid-gesture anyway. Reuses `rbx_viewer`'s own shape
    /// resolution (`resolved_shape_label`) rather than a second, possibly-
    /// diverging check of the `Shape` property — see
    /// `shell::keys::apply_part_defaults` for why only `Part` itself ever
    /// carries one.
    pub(crate) sphere: bool,
    /// Whether this resolves to a real `Part.Shape == Cylinder` right now —
    /// same reasoning and same read as `sphere`, checked against
    /// `resolved_shape_label`'s `"CylinderX"` rather than the legacy
    /// mesh-child `"CylinderY"` case (`rbx_viewer::scene::shape::part_type`):
    /// `Enum.PartType.Cylinder` always draws with its length along the
    /// part's own local X, round in Y/Z — a fixed convention, not something
    /// read per-instance, which is why nothing here stores *which* axis is
    /// which.
    pub(crate) cylinder: bool,
}

impl Target {
    /// Reads one instance's placement out of the DOM, or `None` for anything
    /// that isn't a part with a transform to drag. A container (a `Folder`, a
    /// service, a `Model`) has no `CFrame` or `Size` of its own to write, so
    /// it never becomes a target itself — [`Targets::read`] resolves one to
    /// the parts beneath it instead.
    pub(crate) fn read(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Option<Ref>,
    ) -> Option<Self> {
        let referent = referent?;
        let shape = rbx_viewer::resolved_shape_label(dom, database, referent);
        Some(Target {
            referent,
            model: rbx_viewer::pick::model_of(dom, referent)?,
            sphere: shape == Some("Ball"),
            cylinder: shape == Some("CylinderX"),
        })
    }

    pub(crate) fn position(&self) -> Vec3 {
        self.model.w_axis.truncate()
    }

    /// The part's own axes, still carrying its `Size` in their lengths —
    /// `rbx_viewer::gizmo::basis` normalizes them.
    pub(crate) fn rotation(&self) -> Mat3 {
        Mat3::from_mat4(self.model)
    }

    /// The part's `Size`, which is exactly what `pick::part_model` folded into
    /// the lengths of those columns.
    pub(crate) fn size(&self) -> Vec3 {
        Vec3::new(
            self.model.x_axis.length(),
            self.model.y_axis.length(),
            self.model.z_axis.length(),
        )
    }

    /// The part's own orientation with its `Size` divided back out — the
    /// rotation a `CFrame` carries.
    pub(crate) fn orientation(&self) -> Mat3 {
        let [x, y, z] = gizmo::basis(Some(self.rotation()));
        Mat3::from_cols(x, y, z)
    }

    /// The same part standing somewhere else — what a drag in progress shows
    /// while `Shell` is still writing the move into the DOM.
    pub(crate) fn moved_to(self, position: Vec3) -> Self {
        Target {
            model: Mat4::from_cols(
                self.model.x_axis,
                self.model.y_axis,
                self.model.z_axis,
                position.extend(1.0),
            ),
            ..self
        }
    }

    /// The same part at a new `Size`, standing where the drag put it.
    pub(crate) fn resized_to(self, size: Vec3, position: Vec3) -> Self {
        self.placed(self.orientation(), size, position)
    }

    /// The same part turned, keeping its size and where it stands.
    pub(crate) fn rotated_to(self, orientation: Mat3) -> Self {
        self.placed(orientation, self.size(), self.position())
    }

    /// Rebuilds the model matrix the way `pick::part_model` does: the
    /// orientation's columns scaled by the size, and the centre in the last.
    pub(crate) fn placed(self, orientation: Mat3, size: Vec3, position: Vec3) -> Self {
        Target {
            model: Mat4::from_cols(
                (orientation.x_axis * size.x).extend(0.0),
                (orientation.y_axis * size.y).extend(0.0),
                (orientation.z_axis * size.z).extend(0.0),
                position.extend(1.0),
            ),
            ..self
        }
    }
}

/// Where every selected part stands, in selection order.
///
/// The transform gizmo stands at the [`centre`](Targets::centre) of the whole
/// selection's bounds, exactly the way
/// `rbx_viewer::renderer::selection::Selection::anchor` places the one it
/// draws — both call `rbx_viewer::gizmo::centre_of`, so the handles the user
/// can grab and the handles they can see cannot disagree about where they
/// are. The [`anchor`](Targets::anchor) is a different question: it is the
/// first entry with a placement, and it is what Scale and Rotate actually
/// transform, and whose own frame the local-orientation toggle takes.
/// [`Targets::translate`] is what a group drag uses to move every other part
/// by the same offset, which is what keeps the whole selection's relative
/// arrangement intact while only the gizmo's own travel is measured.
///
/// The second field is the selection's [`pivot`](Targets::pivot), carried
/// through every drag along with the parts; the third, whether the selection
/// is one `BasePart` standing for itself, which Scale treats apart from
/// everything else (see [`Targets::scale_box`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Targets(
    pub(super) Vec<Target>,
    pub(super) Option<Mat4>,
    pub(super) bool,
);

impl Targets {
    /// Reads every part the selection covers out of the DOM, in the order
    /// `referents` lists them.
    ///
    /// A container resolves to the parts beneath it (`pick::parts_of`, the
    /// same function the renderer's outline is built from) rather than being
    /// dropped: a viewport click selects the outermost `Model` around what it
    /// hit, so dropping them left the commonest selection of all with no
    /// target, no handles, and nothing on screen to explain why. A container
    /// with no drawable geometry under it still yields nothing — there is
    /// genuinely nothing to transform.
    ///
    /// Selecting a `Model` *and* something inside it would otherwise name the
    /// same part twice, which a group drag would then move twice as far as
    /// the gizmo travelled. `pick::selection` drops the covered entry, here
    /// and for the outline `shell::selection::outlined` sends the renderer
    /// alike — the dedup belongs to both of them, so neither holds a copy of
    /// it that could drift from the other's.
    pub(crate) fn read(dom: &WeakDom, database: &ReflectionDatabase, referents: &[Ref]) -> Self {
        let entries = pick::selection(dom, database, referents);
        let pivot = match entries.as_slice() {
            [entry] => rbx_lua::pivot::pivot(dom, database, entry.referent()).map(|f| rigid(&f)),
            _ => None,
        };
        let lone_part = matches!(entries.as_slice(), [entry] if entry.is_part());
        let parts = entries
            .iter()
            .flat_map(|entry| entry.parts())
            .filter_map(|&part| Target::read(dom, database, Some(part)))
            .collect();
        Targets(parts, pivot, lone_part)
    }

    /// The pivot of the one part or model selected, rigid — no `Size` in
    /// its columns — or `None` for a selection of several (or of nothing
    /// with a pivot). Move's and Rotate's handles stand on it, so a turn goes
    /// round it (`studio/pivot-tools.md`: "Once set, rotation and scaling
    /// occur around the pivot point"), and Edit Pivot moves it.
    pub(crate) fn pivot(&self) -> Option<Mat4> {
        self.1
    }

    /// The pivot put somewhere else — an Edit Pivot drag's running answer.
    pub(crate) fn set_pivot(&mut self, pivot: Mat4) {
        if self.1.is_some() {
            self.1 = Some(pivot);
        }
    }

    /// The first part the selection covers: what Scale and Rotate transform,
    /// and whose own frame the local-orientation toggle takes. For a selected
    /// `Model` that is its own first descendant part — a `Model` has no
    /// `Size` or `CFrame` to write a resize or a turn into, exactly as a
    /// multi-part selection scales and rotates its anchor alone. Where the gizmo
    /// *sits* is [`Targets::centre`] instead — see this type's own doc
    /// comment for why the two are separate questions.
    pub(crate) fn anchor(&self) -> Option<Target> {
        self.0.first().copied()
    }

    /// Where the gizmo stands: the centre of the world-axis-aligned box
    /// containing every selected part, which for a single part is simply that
    /// part's own centre.
    ///
    /// Shared with the renderer through `rbx_viewer::gizmo::centre_of` rather
    /// than worked out again here, for the same reason the handle geometry
    /// itself is shared — two derivations of "where the gizmo is" are two
    /// things that can drift apart.
    pub(crate) fn centre(&self) -> Option<Vec3> {
        gizmo::centre_of(self.0.iter().map(|target| target.model))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Target> {
        self.0.iter()
    }

    /// The box the Scale handles stand on, `local` or square to the world —
    /// see `rbx_viewer::gizmo::scale_box`, which the renderer draws them
    /// from.
    pub(crate) fn scale_box(&self, local: bool) -> Option<Mat4> {
        let pivot = self.1.filter(|_| !self.2);
        gizmo::scale_box(self.0.iter().map(|target| target.model), pivot, local)
    }

    /// What a Scale drag with `Ctrl` held scales about: a model's pivot, or
    /// the middle of the box — a lone part's, whose `PivotOffset` Studio's
    /// `ScaleDragger` leaves out of it, or several things', on whose middle
    /// it stands their shared basis.
    pub(crate) fn scale_centre(&self, local: bool) -> Option<Vec3> {
        match self.1.filter(|_| !self.2) {
            Some(pivot) => Some(pivot.w_axis.truncate()),
            None => self.scale_box(local).map(|boxed| boxed.w_axis.truncate()),
        }
    }

    /// Whether Scale resizes the anchor's own `Size` — a lone part — rather
    /// than scaling the whole selection by one factor, the way Studio's
    /// `ScaleDragger` takes a model even of one part (`Model:ScaleTo`).
    pub(crate) fn lone_part(&self) -> bool {
        self.2
    }

    /// The same box squared to the pivot, as the renderer draws Edit
    /// Pivot's hotspots on it — see `rbx_viewer::gizmo::box_along`.
    pub(crate) fn pivot_box(&self) -> Option<Mat4> {
        let axes = Mat3::from_mat4(self.1?);
        gizmo::box_along(self.0.iter().map(|target| target.model), axes)
    }

    /// Every part scaled by the same `factor` about `pivot`, from where it
    /// stood in `held` — the selection as it was when the handle was grabbed,
    /// so a whole gesture is one absolute factor rather than a running
    /// product that drifts. A group has no one `Size` to write, so it scales
    /// the way `Model:ScaleTo` does: each part's size by the factor, each
    /// centre by the factor along its offset from the pivot, orientations
    /// untouched. Replaces this value and returns what `Shell` writes: each
    /// referent's new size and position.
    pub(crate) fn scale_about(
        &mut self,
        held: &Targets,
        pivot: Vec3,
        factor: f32,
    ) -> Vec<(Ref, Vec3, Vec3)> {
        self.0 = held
            .0
            .iter()
            .map(|target| {
                let size = target.size() * factor;
                let position = pivot + (target.position() - pivot) * factor;
                target.placed(target.orientation(), size, position)
            })
            .collect();
        self.1 = held.1.map(|own| {
            let position = own.w_axis.truncate();
            Mat4::from_cols(
                own.x_axis,
                own.y_axis,
                own.z_axis,
                (pivot + (position - pivot) * factor).extend(1.0),
            )
        });
        self.0
            .iter()
            .map(|target| (target.referent, target.size(), target.position()))
            .collect()
    }

    /// The largest and smallest factor [`Targets::scale_about`] may take
    /// before some part's size leaves `min..=max` on some axis — the whole
    /// group stops growing when its biggest part hits the ceiling, exactly as
    /// a lone part stops at its own.
    pub(crate) fn factor_within(&self, factor: f32, min: f32, max: f32) -> f32 {
        let mut clamped = factor;
        for target in &self.0 {
            let size = target.size();
            for axis in 0..3 {
                if size[axis] > 0.0 {
                    clamped = clamped.clamp(min / size[axis], max / size[axis]);
                }
            }
        }
        clamped
    }

    /// Every part turned by `rotation` about `centre`, from where it stood in
    /// `held` (see [`Targets::scale_about`] for why the gesture's start is
    /// the reference): its orientation turned, its centre swung round the
    /// pivot with it, so the group keeps its own arrangement. Returns what
    /// `Shell` writes: each referent's new orientation and position.
    pub(crate) fn rotate_about(
        &mut self,
        held: &Targets,
        centre: Vec3,
        rotation: Mat3,
    ) -> Vec<(Ref, Mat3, Vec3)> {
        self.0 = held
            .0
            .iter()
            .map(|target| {
                let orientation = rotation * target.orientation();
                let position = centre + rotation * (target.position() - centre);
                target.placed(orientation, target.size(), position)
            })
            .collect();
        let turn = Mat4::from_translation(centre)
            * Mat4::from_mat3(rotation)
            * Mat4::from_translation(-centre);
        self.1 = held.1.map(|own| turn * own);
        self.0
            .iter()
            .map(|target| (target.referent, target.orientation(), target.position()))
            .collect()
    }

    /// Moves every target by the same offset, which is what keeps a group
    /// drag from rearranging the selection relative to itself — every part
    /// travels exactly as far as the anchor's own gizmo drag did, no more and
    /// no less. Returns each referent's new absolute position (what `Shell`
    /// writes into the DOM) and updates this value in place so the next call
    /// in the same gesture measures from where the parts stand now.
    pub(crate) fn translate(&mut self, delta: glam::Vec3) -> Vec<(Ref, glam::Vec3)> {
        let moves: Vec<(Ref, glam::Vec3)> = self
            .0
            .iter()
            .map(|target| (target.referent, target.position() + delta))
            .collect();
        for target in &mut self.0 {
            *target = target.moved_to(target.position() + delta);
        }
        if let Some(pivot) = &mut self.1 {
            pivot.w_axis += delta.extend(0.0);
        }
        moves
    }

    /// Every target carried rigidly by `carry` (turned and moved as one)
    /// from where it stands here.
    pub(crate) fn carried(&self, carry: Mat4) -> Targets {
        Targets(
            self.0
                .iter()
                .map(|target| Target {
                    model: carry * target.model,
                    ..*target
                })
                .collect(),
            self.1.map(|pivot| carry * pivot),
            self.2,
        )
    }

    /// Replaces the anchor's own placement — what a Scale or Rotate drag
    /// updates as it goes, since only the anchor ever carries their gizmo
    /// (a multi-part selection's Size and Orientation have no group meaning
    /// the way a Move's position offset does).
    pub(crate) fn set_anchor(&mut self, target: Target) {
        if let Some(anchor) = self.0.first_mut() {
            // A lone part's pivot is an offset in its own frame, so it rides
            // along with whatever the resize did to the part's placement —
            // and, as Studio's `ScaleDragger` fixes a resized part's
            // `PivotOffset` up, its position stretches with the part's size,
            // axis by axis, so the pivot moves as a point of the part would.
            if let Some(pivot) = &mut self.1 {
                let frame = |target: &Target| placement(target.orientation(), target.position());
                let mut offset = frame(anchor).inverse() * *pivot;
                let was = anchor.size();
                let stretch = Vec3::select(was.cmpeq(Vec3::ZERO), Vec3::ONE, target.size() / was);
                offset.w_axis = (offset.w_axis.truncate() * stretch).extend(1.0);
                *pivot = frame(&target) * offset;
            }
            *anchor = target;
        }
    }
}
