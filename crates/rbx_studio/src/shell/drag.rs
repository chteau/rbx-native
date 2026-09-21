//! What the left mouse button in the 3D view asks of the editor: resolve a
//! click against the DOM, or move, resize or turn the part a drag is carrying.
//!
//! The split is deliberate. `WorkspaceView` has the cursor, the camera and the
//! handles' geometry but no DOM; `Shell` has the DOM, the Explorer and the
//! undo stack but no idea where the cursor is. Everything in between travels
//! as a [`ViewportAction`].

use glam::{Mat3, Mat4, Vec3};
use gpui_kit::*;
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_viewer::gizmo;
use rbx_viewer::pick::{self, Ray, Selected};

use crate::dragger::surface::{surface_frame, SurfaceFrame};
use crate::properties;
use crate::settle::{self, Settle};
use crate::transform;
use crate::workspace_view::ViewportAction;

use super::{selection, Shell};

/// `RBX_STUDIO_DRAG` / `RBX_STUDIO_RESIZE`: one drag step, applied at startup
/// through the same entry points a real gesture ends with.
mod debug;

/// The property a viewport move or rotation writes. A move gives it three
/// numbers and a rotation nine, and `properties::edit::commit` carries the
/// half that was left out through untouched — which is what keeps a dragged
/// part facing the way it was, and a turned one standing where it was.
pub(super) const CFRAME_PROPERTY: &str = "CFrame";
/// Roblox's binary format spells `BasePart.Size` lowercase, which is the name
/// the DOM keeps — see `rbx_viewer::pick::model_of`, which reads the same pair.
const SIZE_PROPERTY: &str = "size";

impl Shell {
    pub(super) fn handle_viewport_action(
        &mut self,
        action: &ViewportAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ViewportAction::Pick {
                ray,
                cycling,
                extend,
                held,
            } => self.pick_in_viewport(*ray, *cycling, *extend, *held, cx),
            ViewportAction::Hover { ray, alt } => self.hover_in_viewport(*ray, *alt, cx),
            ViewportAction::Scrolled(scroll) => self.scroll_canvas(scroll, cx),
            ViewportAction::Moved {
                moves,
                first,
                settle,
            } => self.move_parts(moves, *first, *settle, cx),
            ViewportAction::Resized { parts, first } => self.resize_parts(parts, *first, cx),
            ViewportAction::Rotated { parts, first } => self.rotate_parts(parts, *first, cx),
            ViewportAction::Turned {
                referent,
                pivot,
                axis,
                first,
            } => self.turn_part(*referent, *pivot, *axis, *first, cx),
            // The one toolbar action that moves the caret instead of changing
            // state, which is why this path carries a `Window` at all.
            ViewportAction::Tool(transform::Action::FocusIncrement(kind)) => {
                self.snap_fields.focus(*kind, window, cx);
            }
            ViewportAction::Tool(action) => self.transform_action(*action, cx),
        }
    }

    /// Cursor motion with nothing held: outlines the nearest `BasePart`
    /// under `ray`, distinctly from the selection outline — Studio's "about
    /// to click" cue (see `ViewportAction::Hover`'s own doc comment for what
    /// `None` means).
    ///
    /// Resolves the outline exactly as a click would (see
    /// `selection::from_click`): plain, the whole enclosing `Model`'s parts,
    /// so the cue previews what a click selects; with `alt`, the single part
    /// under the cursor, what `Alt`-click's cycling lands on. A hover that
    /// only ever showed the raw nearest part told a Studio user the wrong
    /// thing about a plain click on a model, and showed nothing extra when
    /// they held `Alt` to reach a child.
    ///
    /// The parts the current selection already covers are dropped: a hover
    /// box drawn right under the selection's own outline adds nothing (see
    /// `Shell::selection_changed` for the other half of this, the selection
    /// changing out from under a still-hovered part).
    fn hover_in_viewport(&mut self, ray: Option<Ray>, alt: bool, cx: &mut Context<Self>) {
        let meshes = self.viewport.read(cx).meshes().clone();
        let covered = self.covered.clone();
        let mut target = None;
        let hovered: Vec<Selected> = ray
            .and_then(|ray| {
                let hits = pick::parts_along(&self.dom, &self.database, &meshes, ray);
                // The same resolution a click makes, current selection and
                // all: a plain hover previews the enclosing `Model` a plain
                // click would take, and an `Alt` hover previews the *next*
                // part `Alt`-click cycling would land on from the current
                // selection — so the cue tracks the cycle rather than always
                // showing the nearest hit. That last part is what makes the
                // preview work with the camera inside a part: the surrounding
                // part is the nearest hit (distance zero), and only cycling
                // from the current selection reaches the child in front of it,
                // for the hover exactly as for the click.
                let referent =
                    selection::from_click(&self.dom, &self.database, &hits, self.selected(), alt)?;
                // Studio's hover ruler measures the face of whatever the
                // cursor is over — selected or not — once there is something
                // selectable there at all.
                target = hover_target(&self.dom, ray, hits.first().copied());
                selection::outlined(&self.dom, &self.database, &[referent])
                    .into_iter()
                    // A hover entirely inside the current selection adds only a
                    // second box right under the selection's own outline, so it
                    // is dropped (see `Shell::selection_changed` for the other
                    // half of this).
                    .find(|entry| entry.parts().iter().any(|part| !covered.contains(part)))
                    .map(|entry| vec![entry])
            })
            .unwrap_or_default();

        self.viewport
            .update(cx, |viewport, _| viewport.set_hover_target(target));
        if hovered == self.hovered {
            return;
        }
        self.hovered = hovered.clone();
        self.viewport
            .update(cx, |viewport, _| viewport.set_hover(hovered));
    }

    /// A click in the 3D view: select whatever it resolves to, clear the
    /// selection when it resolves to nothing (clicking the sky deselects, the
    /// same as clicking empty space in the Explorer would), or — with
    /// `extend` (`Shift`/`Ctrl`/`Cmd` held) — add it to or remove it from the
    /// selection instead, leaving an empty-space click with the modifier held
    /// alone rather than clearing everything a Studio user did not ask to
    /// drop.
    ///
    /// With `held`, the view has a Move body-drag waiting on this same click
    /// (see `ViewportAction::Pick`): it goes ahead, and the selection stays
    /// as it is, only when the part actually under the cursor is already
    /// selected — itself, or through a selected `Model` — the way Studio
    /// drags a selected object from any point on it. Anything nearer takes
    /// the click as an ordinary pick instead.
    fn pick_in_viewport(
        &mut self,
        ray: Ray,
        cycling: bool,
        extend: bool,
        held: bool,
        cx: &mut Context<Self>,
    ) {
        // The viewport holds a handle onto the render thread's own mesh data
        // (see `WorkspaceView::meshes`): what keeps a `MeshPart`'s pick on the
        // triangles actually drawn rather than the box around them.
        let meshes = self.viewport.read(cx).meshes().clone();
        let hits = pick::parts_along(&self.dom, &self.database, &meshes, ray);
        if held {
            let outlined = selection::outlined(&self.dom, &self.database, self.selected_all());
            let covered = selection::covers(&outlined, hits.first().copied());
            self.viewport.update(cx, |viewport, cx| match covered {
                true => viewport.confirm_grab(cx),
                false => viewport.refuse_grab(),
            });
            if covered {
                return;
            }
        }
        let picked =
            selection::from_click(&self.dom, &self.database, &hits, self.selected(), cycling);

        match (picked, extend) {
            (Some(referent), true) => self.extend_selection(referent, cx),
            (Some(referent), false) => self.select(referent, cx),
            (None, true) => {}
            (None, false) => self.deselect(cx),
        }
    }

    /// One step of a drag, whether it carries one selected part or a whole
    /// group of them.
    ///
    /// A cursor drag asks, through `settle`, to rest the anchor on whatever
    /// the cursor is over. Only the DOM can answer that, so it is answered
    /// here: `delta` corrects the anchor's own move from the view's flat
    /// guess to where it actually landed, and is applied to every part in
    /// `moves` before any of them are written, so the group's relative
    /// layout survives the settle intact. The answer is handed back to the
    /// view too (`WorkspaceView::settle_at`): its draggers are following
    /// their own flat-plane guess until told otherwise.
    ///
    /// History is pushed once, on `first`, for every part the drag carries
    /// together: a snapshot is a whole `WeakDom` clone (see
    /// `crate::history`), so one per mouse move — let alone one per part per
    /// mouse move — would both cost a copy of the place per frame and flush
    /// every earlier undo step out of a fifty-deep stack in under a second.
    /// Each write goes through the same `properties::edit::commit` the
    /// Properties panel uses, so a drag and a typed coordinate cannot
    /// disagree about what moving a part means.
    fn move_parts(
        &mut self,
        moves: &[(Ref, Vec3)],
        first: bool,
        settle: Option<Settle>,
        cx: &mut Context<Self>,
    ) {
        let settled = settle.and_then(|settle| {
            // Same render-thread mesh handle `pick_in_viewport` reads — a
            // settle's own surface search needs to agree with what a click
            // would have hit.
            let meshes = self.viewport.read(cx).meshes().clone();
            let dragged: Vec<Ref> = moves.iter().map(|&(referent, _)| referent).collect();
            settle::settled(&self.dom, &self.database, &meshes, &dragged, settle)
        });
        let delta = settled
            .as_ref()
            .zip(moves.first())
            .map(|(settled, &(_, fallback))| settled.centre - fallback);

        if first {
            self.push_history();
        }

        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        for &(referent, position) in moves {
            let position = position + delta.unwrap_or(Vec3::ZERO);
            let text = vector(position);
            let written = properties::edit::commit(
                &mut dom,
                &self.database,
                referent,
                CFRAME_PROPERTY,
                &text,
            );
            if let Err(err) = written {
                self.output.push_warning(&format!("viewport drag: {err}"));
            }
        }
        self.dom = dom;
        // Overwrites, not appends: this step's log alone — one `CFrame`
        // write per part carried, however many mouse-move frames it took to
        // get there — is what a later undo of the whole gesture reflects
        // (see this method's doc comment for why history is pushed once, on
        // `first`, not per frame). Reflected as one batch, one DOM clone,
        // whatever the group's size.
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        if settle.is_some() {
            let delta = delta.unwrap_or(Vec3::ZERO);
            self.viewport.update(cx, |viewport, _| {
                viewport.settle_at(delta, settled.as_ref())
            });
        }
        cx.notify();
    }

    /// One step of a Scale drag, for every part it carries. Two properties
    /// per part, because Studio's Scale tool holds the face opposite the
    /// grabbed one still: the part's `Size` grows and its `CFrame` shifts by
    /// half of that growth — or, for a group scaled as a whole, by its
    /// offset from the box's far face — and either one written without the
    /// other would show the part jumping.
    fn resize_parts(&mut self, parts: &[(Ref, Vec3, Vec3)], first: bool, cx: &mut Context<Self>) {
        let writes: Vec<(Ref, &str, String)> = parts
            .iter()
            .flat_map(|&(referent, size, position)| {
                [
                    (referent, SIZE_PROPERTY, vector(size)),
                    (referent, CFRAME_PROPERTY, vector(position)),
                ]
            })
            .collect();
        self.write_drag(first, &writes, cx);
    }

    /// One step of a Rotate drag, for every part it carries: the rotation
    /// and the centre together, as the twelve numbers Roblox's own
    /// `CFrame.new(x, y, z, R00 … R22)` takes them in, row by row
    /// (`creator-docs`, `reference/engine/datatypes/CFrame.yaml`) — a lone
    /// part turning about itself keeps its centre, a part of a group swings
    /// round the group's.
    fn rotate_parts(&mut self, parts: &[(Ref, Mat3, Vec3)], first: bool, cx: &mut Context<Self>) {
        let writes: Vec<(Ref, &str, String)> = parts
            .iter()
            .map(|&(referent, orientation, position)| {
                let rows =
                    (0..3).flat_map(|row| (0..3).map(move |column| orientation.col(column)[row]));
                let text = [position.x, position.y, position.z]
                    .into_iter()
                    .chain(rows)
                    .map(|term| term.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                (referent, CFRAME_PROPERTY, text)
            })
            .collect();
        self.write_drag(first, &writes, cx);
    }

    /// Writes one step of a Scale or Rotate drag into the DOM.
    ///
    /// History is pushed once, on `first`: a snapshot is a whole `WeakDom`
    /// clone (see `crate::history`), so one per mouse move would both cost
    /// a copy of the place per frame and flush every earlier undo step out of
    /// a fifty-deep stack in under a second. The writes themselves go through
    /// the same `properties::edit::commit` the Properties panel uses, so a
    /// drag and a typed value cannot disagree about what transforming a part
    /// means.
    fn write_drag(&mut self, first: bool, writes: &[(Ref, &str, String)], cx: &mut Context<Self>) {
        if first {
            self.push_history();
        }

        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let written = writes.iter().try_for_each(|(referent, name, text)| {
            properties::edit::commit(&mut dom, &self.database, *referent, name, text).map(|_| ())
        });
        self.dom = dom;
        // Same reasoning as `move_parts`: overwrites the entry's log with
        // just this step's writes — one `CFrame` per part for a Rotate,
        // Size and CFrame per part for a Scale — one patch of each part
        // either way, here and on undo.
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);

        if let Err(err) = written {
            self.output.push_warning(&format!("viewport drag: {err}"));
            return;
        }
        cx.notify();
    }

    /// A `T`/`R` quarter turn mid-drag.
    ///
    /// Unlike a move, this cannot go through `properties::edit::commit`: a
    /// `CFrame`'s rotation has no text syntax that path accepts (see
    /// `properties::edit::parse`, which deliberately keeps the existing
    /// rotation and reads only a position), so the new frame is written
    /// straight onto the instance instead. It shares the drag's one undo step,
    /// opening it if the turn is the first thing the gesture did.
    fn turn_part(
        &mut self,
        referent: Ref,
        pivot: Vec3,
        axis: Vec3,
        first: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(Variant::CFrame(frame)) = self
            .dom
            .get(referent)
            .and_then(|instance| instance.properties().get(CFRAME_PROPERTY))
        else {
            return;
        };

        let r = frame.rotation;
        // `CFrameData::rotation` is row-major; `Mat3::from_cols_array` is not.
        let rotation = glam::Mat3::from_cols(
            Vec3::new(r[0], r[3], r[6]),
            Vec3::new(r[1], r[4], r[7]),
            Vec3::new(r[2], r[5], r[8]),
        );
        let position = Vec3::new(frame.position.x, frame.position.y, frame.position.z);
        let (rotation, position) =
            gizmo::turned(rotation, position, pivot, gizmo::quarter_turn(axis));

        let turned = Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: position.x,
                y: position.y,
                z: position.z,
            },
            rotation: [
                rotation.x_axis.x,
                rotation.y_axis.x,
                rotation.z_axis.x,
                rotation.x_axis.y,
                rotation.y_axis.y,
                rotation.z_axis.y,
                rotation.x_axis.z,
                rotation.y_axis.z,
                rotation.z_axis.z,
            ],
        });

        if first {
            self.push_history();
        }
        if let Err(err) = self.dom.set_property(referent, CFRAME_PROPERTY, turned) {
            self.output.push_warning(&format!("viewport turn: {err}"));
            return;
        }
        // Same reasoning as `move_parts`: this turn writes exactly the one
        // `CFrame` change, however many `T`/`R` presses the gesture took.
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// Hands the viewport the boxes a handle drag can soft-snap onto: every
    /// drawn part in the workspace except whichever the selection carries — a
    /// group drag carries all of them together, so none should pull the
    /// others.
    ///
    /// Only on a selection change or after an edit moved something other
    /// than the selection (see `Shell::reflect_changes`), never per mouse
    /// move — this walks the whole workspace, and during a drag nothing but
    /// the dragged parts is moving anyway.
    pub(super) fn sync_snap_neighbours(&mut self, cx: &mut Context<Self>) {
        // Everything the selection carries is left out — a selected
        // `Model`'s own parts included, which `covered` holds and the
        // selection itself does not.
        let neighbours: Vec<Mat4> = pick::drawable_parts(&self.dom, &self.database)
            .filter(|referent| !self.covered.contains(referent))
            .filter_map(|referent| pick::model_of(&self.dom, referent))
            .collect();
        self.viewport
            .update(cx, |viewport, _| viewport.set_neighbours(neighbours));
    }
}

/// The face of `part` under `ray`, framed on its corner nearest the cursor,
/// and where the cursor meets it — what Studio's hover ruler measures.
fn hover_target(dom: &WeakDom, ray: Ray, part: Option<Ref>) -> Option<(SurfaceFrame, Vec3)> {
    let model = pick::model_of(dom, part?)?;
    let hit = ray.at(pick::ray_hits_box(ray, model)?);
    Some((surface_frame(model, hit)?, hit))
}

/// The three-number text `properties::edit::parse` reads a `Vector3` — or a
/// `CFrame`'s position — back out of. `pub(super)`: also `shell::align`'s own
/// position writes, which commit through this exact same text-shaped path.
pub(super) fn vector(value: Vec3) -> String {
    format!("{}, {}, {}", value.x, value.y, value.z)
}
