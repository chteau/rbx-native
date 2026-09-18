//! The left mouse button over the 3D view: clicking to select (plain,
//! `Shift`/`Ctrl`/`Cmd` to add to or remove from the selection), and dragging
//! the whole selection by a transform tool's handles or by any selected
//! part's own body.
//!
//! All of it happens here on the UI thread, against `rbx_viewer`'s own
//! geometry ([`rbx_viewer::pick`], [`rbx_viewer::gizmo`]) and the camera the
//! render thread reports with each frame — the renderer builds the handles the
//! user sees from exactly the same functions, so what is grabbable is what is
//! drawn. Nothing here touches the DOM: `Shell` owns that, and hears about a
//! click or a drag through [`super::ViewportAction`].
//!
//! The camera's own gestures are untouched: look and orbit are the *right*
//! button (see `WorkspaceView::begin_look`), so a left-button drag never
//! competes with them.

use glam::{Mat3, Mat4, Vec2, Vec3};
use gpui_kit::{Modifiers, Pixels, Point};
use rbx_viewer::gizmo::{self, Faces, Handles};
use rbx_viewer::pick::{self, Ray};
use rbx_viewer::snap;

use crate::transform::{Target, Tool};

use super::{readout, ViewportAction, WorkspaceView};
use crate::settle::Settle;

/// `BasePart.Size`'s documented range: "the individual dimensions (length,
/// height, width) can be as low as `0.001` and as high as `2048`"
/// (`creator-docs`, `reference/engine/classes/BasePart.yaml`). A drag that ran
/// past either end would write a value the engine rejects.
const MIN_SIZE: f32 = 0.001;
const MAX_SIZE: f32 = 2048.0;

/// How near a part's surface a free drag's grab point has to pass before it
/// soft-snaps onto it, as a fraction of a dragger arm.
///
/// A fraction of the arm rather than a fixed number of studs because the arm
/// is itself screen-relative (see `gizmo::arm_length`): the pull then reaches
/// the same distance on screen whether the camera is on top of the part or
/// across the map from it, which is how it behaves in Studio. The docs
/// describe soft snapping and illustrate it but publish no threshold, so the
/// fraction is this editor's own.
const SOFT_SNAP_REACH: f32 = 0.35;

/// Whether a click's modifiers mean "add to (or drop from) the selection"
/// rather than "replace it" — `Shift`, `Ctrl`, or `Cmd` (`platform`), per
/// `creator-docs` (`studio/ui-overview.md#object-selection`), which lists all
/// three as equivalent.
fn extends_selection(modifiers: Modifiers) -> bool {
    modifiers.shift || modifiers.control || modifiers.platform
}

/// A left-button drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Drag {
    /// One of the Move tool's arrows is held: the axis line it slides along,
    /// and how far along that line the cursor stood when it was grabbed.
    Axis {
        origin: Vec3,
        axis: Vec3,
        grabbed: f32,
    },
    /// Any selected part's own body is held — Studio's "cursor dragging". The
    /// gizmo's anchor comes to rest on whatever the cursor is over (see
    /// [`crate::settle`]); with nothing under the cursor it travels instead
    /// in the plane that faced the camera through the grab point, keeping the
    /// anchor where it was relative to the cursor regardless of which
    /// selected part was actually grabbed — the offset is always measured
    /// from the anchor, so the whole group tracks the cursor together.
    ///
    /// The plane is fixed in the world at the moment of the grab rather than
    /// recomputed from the live camera: a plane that turned with the view
    /// would slide the part every time the camera did, which is not what
    /// holding it still means.
    ///
    /// With snapping off, the grab point soft-snaps onto the surfaces and
    /// edges of parts it passes near (see [`Landing`]), which is what settles
    /// a part against its neighbours instead of sliding it flat across the
    /// view.
    Plane {
        point: Vec3,
        normal: Vec3,
        offset: Vec3,
    },
    /// One of the Scale tool's balls is held: the part's own axis the grabbed
    /// face sits on (pointing *out* through that face), where the cursor stood
    /// along it, and the placement the drag started from.
    Size {
        origin: Vec3,
        axis: Vec3,
        grabbed: f32,
        size: Vec3,
        /// Which component of `size` this axis is — `Size` is expressed in the
        /// part's own frame, so the axis alone does not say.
        component: usize,
    },
    /// One of the Scale tool's balls is held on a *group's* box (see
    /// `gizmo::scale_box`): the box's centre, the world axis pointing out
    /// through the grabbed face, where the cursor stood along it, how long
    /// the box was along that axis, and the centre of the opposite face —
    /// the point the whole group scales about, so that face holds still the
    /// way a lone part's does.
    Box {
        origin: Vec3,
        axis: Vec3,
        grabbed: f32,
        extent: f32,
        pivot: Vec3,
    },
    /// One of the Rotate tool's rings is held: the ring's own frame, frozen at
    /// the grab (see [`gizmo::ring_crossing`]), the orientation the anchor
    /// part started at, the angle last measured and how far the drag has
    /// turned in total.
    Ring {
        origin: Vec3,
        frame: (Vec3, Vec3, Vec3),
        orientation: Mat3,
        last: f32,
        turned: f32,
    },
}

/// What one step of a drag does to the part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Change {
    Position(Vec3),
    /// A new `Size`, and the centre that keeps the face *opposite* the grabbed
    /// one standing still.
    Size {
        size: Vec3,
        position: Vec3,
    },
    Orientation(Mat3),
    /// A group's box pulled to `factor` times its size at the grab, about
    /// `pivot` (see [`Drag::Box`]).
    Scaled {
        pivot: Vec3,
        factor: f32,
    },
}

impl Drag {
    /// What `Shell` needs to rest the part on the scene for this cursor ray:
    /// the grab, as the ray that made it, and where the part stood then. An
    /// axis drag is pinned to its line and never settles.
    pub(super) fn settle(self, cursor: Ray) -> Option<Settle> {
        match self {
            // Only a Move drag's own body-grab (`Plane`) settles onto a
            // surface — Scale and Rotate have no such concept, and an
            // axis-arrow Move drag already slides exactly along its axis.
            Drag::Axis { .. } | Drag::Size { .. } | Drag::Box { .. } | Drag::Ring { .. } => None,
            Drag::Plane {
                point,
                normal,
                offset,
            } => Some(Settle {
                cursor,
                // The plane faces back along the grab ray, so the ray itself
                // is the plane's normal reversed, starting where it hit.
                grab: Ray::new(point, -normal),
                centre: point + offset,
            }),
        }
    }
}

/// What a drag is allowed to land on for this one mouse move: the grid its
/// travel rounds to (`0.0` for no grid) — studs for Move and Scale, which
/// share the toolbar's one increment, radians for Rotate's own — and the
/// parts its grab point can soft-snap onto.
#[derive(Debug, Clone, Copy)]
pub(super) struct Landing<'a> {
    pub(super) grid: f32,
    pub(super) angle: f32,
    pub(super) neighbours: &'a [Mat4],
    pub(super) reach: f32,
}

impl WorkspaceView {
    /// The world-space ray under a window-space cursor position, or `None`
    /// before the panel has a size or the render thread has reported a camera.
    pub(super) fn cursor_ray(&self, position: Point<Pixels>, scale: f32) -> Option<Ray> {
        let pose = self.view?;
        let viewport = self.viewport.get();
        if viewport.size.0 == 0 || viewport.size.1 == 0 {
            return None;
        }

        // GPUI reports logical pixels against the window; the panel's own rect
        // is kept in physical ones (see `frame::Viewport`), which is also what
        // the frame under the cursor was rendered at.
        let pixel = Vec2::new(
            f32::from(position.x) * scale - viewport.origin.0 as f32,
            f32::from(position.y) * scale - viewport.origin.1 as f32,
        );
        let extent = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32);
        let projection = pose.view_projection(self.orthographic, extent.x / extent.y);
        Some(pick::ray_through(projection, pick::ndc_of(pixel, extent)))
    }

    /// Cursor motion with nothing held: resolves what is under it and asks
    /// `Shell` to outline it, distinctly from the selection outline —
    /// Studio's "about to click" cue. `None` when the panel has no size yet
    /// or the render thread has not reported a camera (see `cursor_ray`),
    /// which clears the hover outline exactly as a ray that hits nothing
    /// does.
    pub(super) fn hover_moved(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        scale: f32,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let ray = self.cursor_ray(position, scale);
        cx.emit(ViewportAction::Hover {
            ray,
            alt: modifiers.alt,
        });
    }

    /// Where the Move and Rotate arms stand this frame, anchored on the same
    /// part `rbx_viewer::renderer::selection::Selection::anchor` draws them
    /// on — the first selected part with a placement, whether or not it is
    /// alone — and built the same way `rbx_viewer::renderer` builds the ones
    /// on screen. Scale's own handles are [`WorkspaceView::faces`].
    fn handles(&self) -> Option<Handles> {
        let anchor = self.targets.anchor()?;
        let pose = self.view?;
        // Move drags the whole selection by one offset and Rotate turns it
        // about one point, so both sit at the middle of it — which for one
        // part is that part's own centre. The renderer picks the same origin
        // the same way (see `renderer::Renderer::handles`), both from
        // `gizmo::centre_of`, so what can be grabbed is what is drawn. The
        // *basis* always comes from the anchor — a selection has no
        // aggregate rotation to take.
        let origin = self.targets.centre()?;
        Some(Handles::new(
            origin,
            gizmo::basis(self.transform.local.then(|| anchor.rotation())),
            gizmo::arm_length(origin, pose, self.orthographic),
        ))
    }

    /// Where the Scale tool's balls stand this frame: on the faces of a lone
    /// part's own box, or of the world-aligned box round a group (see
    /// `gizmo::scale_box`). Built from the same placement and the same camera
    /// the renderer builds the drawn ones from (see
    /// `rbx_viewer::renderer::Renderer::handles`), so what can be grabbed is
    /// what is on screen.
    fn faces(&self) -> Option<Faces> {
        Some(Faces::new(
            self.targets.scale_box()?,
            self.view?,
            self.orthographic,
        ))
    }

    /// The left button going down: grab a handle, grab the selected part, or
    /// — failing both — ask `Shell` to resolve a pick against the DOM.
    pub(super) fn press(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        scale: f32,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.drag = None;
        self.pending_grab = None;
        let Some(ray) = self.cursor_ray(position, scale) else {
            return;
        };

        let cycling = modifiers.alt;
        let extend = extends_selection(modifiers);
        if self.transform.drags() {
            // A handle is the gizmo's own, drawn over everything: grabbing
            // one needs no second opinion.
            if let Some(drag) = self.grab_handle(ray) {
                self.begin(drag, cx);
                return;
            }
            // A selected part's body is only a *candidate*: the view knows
            // the selection's boxes but not what else stands in front of
            // them, so the click goes to `Shell` as a pick that may turn
            // into this drag (see [`WorkspaceView::confirm_grab`]) — and
            // never with `Alt` or an extend modifier, which ask to change
            // the selection, not to move it.
            if !cycling && !extend {
                self.pending_grab = self.grab_body(ray);
            }
        }

        cx.emit(ViewportAction::Pick {
            ray,
            // Studio's selection cycling: `Alt`/`⌥`-click steps to the next
            // object behind the current one instead of selecting a model.
            cycling,
            extend,
            held: self.pending_grab.is_some(),
        });
    }

    /// Starts `drag` this gesture: the part under a body grab, or the gizmo
    /// itself under an axis/face/ring grab, would otherwise still be wearing
    /// a hover box for the whole gesture — nothing moves the cursor off it,
    /// since `render`'s `on_mouse_move` routes every move into `drag_to`
    /// instead of `hover_pending` once `dragging()` is true.
    fn begin(&mut self, drag: Drag, cx: &mut gpui_kit::Context<Self>) {
        self.drag = Some(drag);
        self.held = self.targets.clone();
        self.dragged = false;
        self.drag_readout = None;
        self.hover_pending = None;
        cx.emit(ViewportAction::Hover {
            ray: None,
            alt: false,
        });
    }

    /// `Shell`'s answer to a pick sent with `held`: what the cursor is over
    /// is already selected, so the body grab the press held back goes ahead
    /// as this gesture's drag. A no-op once the button is up again
    /// ([`WorkspaceView::end_drag`] clears the candidate), or when the press
    /// held nothing.
    pub(crate) fn confirm_grab(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if let Some(drag) = self.pending_grab.take() {
            self.begin(drag, cx);
        }
    }

    /// `Shell`'s other answer: something nearer than the selection was under
    /// the cursor and took the click as a pick instead, so nothing is held.
    pub(crate) fn refuse_grab(&mut self) {
        self.pending_grab = None;
    }

    /// What this ray grabs on the gizmo itself, if anything: a Move arrow, a
    /// Scale face, a Rotate ring — never a part's body, which is
    /// [`WorkspaceView::grab_body`]'s own question.
    fn grab_handle(&self, ray: Ray) -> Option<Drag> {
        let handles = self.handles()?;
        let anchor = self.targets.anchor()?;
        match self.transform.tool {
            Tool::Select => None,
            Tool::Move => self.grab_axis(&handles, ray),
            Tool::Scale if self.targets.len() > 1 => grab_box(&self.faces()?, ray),
            Tool::Scale => grab_face(&self.faces()?, anchor, ray),
            Tool::Rotate => {
                let axis = handles.grab_ring(ray)?;
                let frame = handles.ring_frame(axis);
                let (angle, ..) = gizmo::ring_crossing(handles.origin(), frame, ray)?;
                Some(Drag::Ring {
                    origin: handles.origin(),
                    frame,
                    orientation: anchor.orientation(),
                    last: angle,
                    turned: 0.0,
                })
            }
        }
    }

    /// The body grab this ray would open on the current selection — Move's
    /// cursor dragging, which starts a group drag of the whole selection by
    /// any selected part's own body (see [`WorkspaceView::drag_to`]). Only
    /// Move: `creator-docs` documents cursor dragging under Move alone.
    ///
    /// Tested against the selection's own boxes only, which is all the view
    /// holds: whether something *unselected* stands nearer along the ray is
    /// `Shell`'s call, made against the real geometry when the pick this
    /// accompanies is resolved (see `Shell::pick_in_viewport`).
    fn grab_body(&self, ray: Ray) -> Option<Drag> {
        if self.transform.tool != Tool::Move {
            return None;
        }
        let anchor = self.targets.anchor()?;
        let distance = self
            .targets
            .iter()
            .filter_map(|target| pick::ray_hits_box(ray, target.model))
            .min_by(|a, b| a.total_cmp(b))?;
        let point = ray.at(distance);
        Some(Drag::Plane {
            point,
            // Square to the view at the moment of the grab, which is the one
            // orientation every cursor position on screen has an answer in.
            normal: -ray.direction,
            offset: anchor.position() - point,
        })
    }

    fn grab_axis(&self, handles: &Handles, ray: Ray) -> Option<Drag> {
        let axis = handles.direction(handles.grab(ray)?);
        // Which arm was grabbed comes from the handles, where the user is
        // actually pointing; where the drag is measured *from* is the anchor's
        // own position, because `Change::Position` is the anchor's new place
        // and every other selected part follows it by the same offset (see
        // `transform::Targets::translate`). The two differ by a constant once
        // more than one part is selected, and a drag only ever reads the
        // difference between two samples, so the travel is identical either
        // way — but the position built from it has to start where the part is.
        let origin = self.targets.anchor()?.position();
        Some(Drag::Axis {
            origin,
            axis,
            grabbed: gizmo::along_axis(origin, axis, ray)?,
        })
    }

    /// What this move is allowed to land on: the grid in force — with `Shift`
    /// already inverting the toolbar's checkbox for the length of the drag —
    /// and the neighbours a free drag can soft-snap onto.
    fn landing(&self, shift: bool) -> Landing<'_> {
        Landing {
            grid: self.transform.translate.grid(shift),
            angle: self.transform.rotate.grid(shift).to_radians(),
            neighbours: &self.neighbours,
            reach: self.arm() * SOFT_SNAP_REACH,
        }
    }

    /// One dragger arm in studs, the screen-relative length everything the
    /// gizmo measures in the world is scaled by.
    fn arm(&self) -> f32 {
        let (Some(anchor), Some(pose)) = (self.targets.anchor(), self.view) else {
            return 0.0;
        };
        gizmo::arm_length(anchor.position(), pose, self.orthographic)
    }

    /// The cursor moving with a drag held: works out what this step does to
    /// the gizmo's anchor, carries every other selected part by the same
    /// offset when it is a Move (see `transform::Targets::translate`), and
    /// tells `Shell`, which is what writes the whole group into the DOM.
    pub(super) fn drag_to(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        scale: f32,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let (Some(drag), Some(ray)) = (self.drag, self.cursor_ray(position, scale)) else {
            return;
        };
        let Some(anchor) = self.targets.anchor() else {
            return;
        };
        // `position` is reused below as a match binding name for the part's
        // own new world-space placement (`Change::Position`), which shadows
        // this screen-space one for the length of that arm — kept under its
        // own name so the readout can still place itself against it there.
        let cursor = position;
        // Read per move rather than latched at the grab: Studio's Shift is
        // held and released mid-drag, and the part follows it either way.
        let Some((drag, change)) = advance(drag, ray, self.landing(modifiers.shift)) else {
            return;
        };
        // Kept even when nothing below changes: a rotate drag measures each
        // step against the previous one, so a sample that moved the part
        // nowhere still has to be the one the next step is measured from.
        self.drag = Some(drag);

        let referent = anchor.referent;
        // `first` is asked for only once a step is known to change the part
        // (see [`stepped`]): the answer opens the gesture's undo entry, and a
        // sample that leaves the part where it stands must not spend it.
        match change {
            Change::Position(position) => {
                let Some((_, first)) = stepped(anchor, change, &mut self.dragged) else {
                    return;
                };
                // Applied to every selected part below, not just the anchor:
                // this is what keeps the group's relative layout intact while
                // only the anchor's own gizmo drag is ever actually measured
                // against the ray. Kept here as well as written into the DOM:
                // the handles have to follow the cursor within this same
                // gesture, and the DOM's answer only comes back through
                // `set_targets` once `Shell` has applied it.
                let moves = self.targets.translate(position - anchor.position());
                // The distance is measured from where the anchor stood at
                // the grab (`self.held`, frozen since `begin`), not from this
                // step's own previous position — "studs moved so far" means
                // the whole gesture's travel, not one step's worth of it.
                self.drag_readout = self.held.anchor().map(|start| {
                    (
                        readout::position(cursor, self.viewport.get().origin, scale),
                        readout::moved((position - start.position()).length()),
                    )
                });
                cx.emit(ViewportAction::Moved {
                    moves,
                    first,
                    settle: drag.settle(ray),
                });
            }
            // Scale and Rotate have no group meaning yet — see
            // `transform::Targets::set_anchor` — so only the anchor itself
            // moves, exactly as it did before there was more than one part to
            // select.
            // A lone part's Size is its own, so a Scale of one resizes that
            // one axis exactly as before; a group has no one Size and scales
            // as a whole (see `transform::Targets::scale_about`), which is
            // what the box its handles stand on already promised.
            Change::Size { size, position } => {
                let Some((moved, first)) = stepped(anchor, change, &mut self.dragged) else {
                    return;
                };
                self.targets.set_anchor(moved);
                // How far the dragged axis has grown since the grab, on the
                // same one component `Drag::Size` was grabbed on — `drag`
                // still carries it unchanged (`advance` returns `Drag::Size`
                // as-is, see its own match arm).
                if let (Drag::Size { component, .. }, Some(start)) = (drag, self.held.anchor()) {
                    self.drag_readout = Some((
                        readout::position(cursor, self.viewport.get().origin, scale),
                        readout::grown(size[component] - start.size()[component]),
                    ));
                }
                cx.emit(ViewportAction::Resized {
                    parts: vec![(referent, size, position)],
                    first,
                });
            }
            Change::Scaled { pivot, factor } => {
                let factor = self.held.factor_within(factor, MIN_SIZE, MAX_SIZE);
                let change = Change::Scaled { pivot, factor };
                let Some((_, first)) = stepped(anchor, change, &mut self.dragged) else {
                    return;
                };
                // The box's own growth along the dragged axis, in studs —
                // `extent` is `Drag::Box`'s length at the grab, unchanged
                // since (`advance` returns `Drag::Box` as-is, see its own
                // match arm), so `factor` applied to it is the whole
                // gesture's growth, not one step's.
                if let Drag::Box { extent, .. } = drag {
                    self.drag_readout = Some((
                        readout::position(cursor, self.viewport.get().origin, scale),
                        readout::grown((factor - 1.0) * extent),
                    ));
                }
                let held = self.held.clone();
                let parts = self.targets.scale_about(&held, pivot, factor);
                cx.emit(ViewportAction::Resized { parts, first });
            }
            // Rotate turns every selected part about the selection's centre
            // (`creator-docs`, `parts/models.md`: a model "transforms based
            // on the center of its bounding box"), which for one part is its
            // own centre and leaves it exactly where it stands.
            Change::Orientation(orientation) => {
                let Some((_, first)) = stepped(anchor, change, &mut self.dragged) else {
                    return;
                };
                let Drag::Ring {
                    origin,
                    orientation: was,
                    ..
                } = drag
                else {
                    unreachable!("only a ring turns");
                };
                // The turn since the grab, as the anchor's orientation then
                // and now give it: absolute, so the group cannot drift.
                let rotation = orientation * was.transpose();
                let held = self.held.clone();
                let parts = self.targets.rotate_about(&held, origin, rotation);
                cx.emit(ViewportAction::Rotated { parts, first });
            }
        }
    }

    /// The correction `Shell` made to the anchor's own move for this gesture,
    /// when it rested the anchor on a surface the view itself cannot see —
    /// the difference between where the anchor actually landed and the flat
    /// guess `drag_to` already applied. Unlike [`WorkspaceView::set_targets`]
    /// this is taken mid-gesture: it is the drag's own answer, finished with
    /// the DOM, not a round trip that could land a frame late. Applied to
    /// every selected part by the same offset, exactly as `drag_to`'s own
    /// `Targets::translate` call is, so a settle never rearranges the group
    /// relative to itself.
    pub(crate) fn settle_at(&mut self, delta: Vec3) {
        if self.drag.is_some() {
            self.targets.translate(delta);
        }
    }

    /// Whether a drag is under way, which is what keeps a moving cursor from
    /// also being reported to the camera.
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Applies the latest recorded cursor position to the drag in progress
    /// — see `render`'s `on_mouse_move` handler for why the two are apart.
    pub(super) fn step_drag(
        &mut self,
        window: &gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        if let Some((position, modifiers)) = self.drag_pending.take() {
            let scale = window.scale_factor();
            self.drag_to(position, modifiers, scale, cx);
        }
    }

    /// The button coming up: the last cursor position the frame gate has
    /// not applied yet is applied first, so the part lands exactly where it
    /// was let go rather than a frame short of it.
    pub(super) fn end_drag(&mut self, window: &gpui_kit::Window, cx: &mut gpui_kit::Context<Self>) {
        self.step_drag(window, cx);
        self.drag = None;
        self.drag_stepped_at = None;
        // A grab `Shell` has not answered yet is answered by the release:
        // nothing is held any more.
        self.pending_grab = None;
    }

    /// `t`/`r` typed with a part held by its body, reporting whether it was
    /// one of the two and was actually carried out — the caller stops there
    /// only then, so a `t` typed with nothing in hand stays an ordinary key.
    pub(super) fn turn_key(
        &mut self,
        key: &str,
        modifiers: Modifiers,
        cx: &mut gpui_kit::Context<Self>,
    ) -> bool {
        // Unmodified: Ctrl+T and Ctrl+R are creator-docs' *other* quarter
        // turns, the ones that act on the selection with no drag at all, and
        // neither belongs to this gesture.
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return false;
        }
        match key {
            "t" => self.turn(true, cx),
            "r" => self.turn(false, cx),
            _ => false,
        }
    }

    /// `T` or `R` with a part held by its body: a quarter turn about the point
    /// it was picked up by.
    ///
    /// `creator-docs` (`parts/index.md#transform-parts`): "While cursor
    /// dragging, `T` and `R` can be used to quickly rotate the part in 90°
    /// increments around the point you picked it up by. `T` tilts the part 90°
    /// towards the camera, while `R` rotates the part 90° around the normal of
    /// the hovered surface."
    ///
    /// Only for a body drag: an axis dragger is a slide along one line, and
    /// the docs give these two keys to cursor dragging alone. Turns the
    /// gizmo's anchor alone rather than the whole group — the docs describe
    /// this for one part being cursor-dragged, and a multi-part turn about a
    /// point that is not every part's own centre has no agreed meaning yet.
    pub(super) fn turn(&mut self, tilt: bool, cx: &mut gpui_kit::Context<Self>) -> bool {
        let (Some(Drag::Plane { point, normal, .. }), Some(target)) =
            (self.drag, self.targets.anchor())
        else {
            return false;
        };
        let axis = if tilt {
            // The plane's normal was fixed at the grab as `-ray.direction`, so
            // the view direction is its opposite, and `forward × up` is the
            // camera's right — the axis a tilt "towards the camera" turns
            // about. A camera looking straight down has no such axis and the
            // tilt has no meaning, so nothing happens.
            (-normal).cross(Vec3::Y).try_normalize()
        } else {
            // "The hovered surface": the same nearby surface a free drag would
            // soft-snap the grab point onto. With none in reach there is
            // nothing being hovered, and the world's own up stands in for it.
            Some(
                snap::nearest_surface(point, &self.neighbours, self.arm() * SOFT_SNAP_REACH)
                    .map_or(Vec3::Y, |surface| surface.normal),
            )
        };
        let Some(axis) = axis else {
            return false;
        };

        let turn = gizmo::quarter_turn(axis);
        let (linear, position) = gizmo::turned(target.rotation(), target.position(), point, turn);
        self.targets.set_anchor(target.turned_to(linear, position));
        // The part turned about the grab point, so the cursor now holds it by
        // a different part of itself; the offset has to turn with it or the
        // next move would snap it back.
        if let Some(Drag::Plane { offset, .. }) = &mut self.drag {
            *offset = turn * *offset;
        }

        // A quarter turn always changes the part, so it can open the gesture
        // the way a real drag step does.
        let first = !std::mem::replace(&mut self.dragged, true);
        cx.emit(ViewportAction::Turned {
            referent: target.referent,
            pivot: point,
            axis,
            first,
        });
        true
    }

    /// The boxes a free drag can soft-snap onto: every drawn part in the
    /// workspace but the selected one, pushed down from `Shell`, which is the
    /// only side with a DOM to read them out of.
    pub(crate) fn set_neighbours(&mut self, neighbours: Vec<Mat4>) {
        self.neighbours = neighbours;
    }
}

/// Which of the part's own faces a Scale ball stands on, and the drag that
/// grabbing it opens.
///
/// No guessing at which axis the user meant: a ball sits *on* one of the
/// part's own faces, and `BasePart.Size` is expressed along exactly those axes
/// — so the handle names the component that stretches outright, whichever way
/// the world/local toggle stands.
fn grab_face(faces: &Faces, target: Target, ray: Ray) -> Option<Drag> {
    let (grabbed, sign) = faces.grab(ray)?;
    // Pointing out through the grabbed face, so dragging away from the part
    // always grows it.
    let axis = faces.direction(grabbed) * sign;

    let origin = target.position();
    Some(Drag::Size {
        origin,
        axis,
        grabbed: gizmo::along_axis(origin, axis, ray)?,
        size: target.size(),
        component: grabbed as usize,
    })
}

/// Which face of a group's box a Scale ball stands on, and the whole-group
/// drag grabbing it opens: pulling the face scales every part by the same
/// factor about the opposite face, which holds still.
///
/// One factor rather than one axis: a group's parts stand at every angle to
/// the box, and stretching the box along one world axis is nothing a rotated
/// part's own `Size` can express. `creator-docs` gives the transform tools no
/// per-axis behaviour for models at all (`parts/models.md`), so this follows
/// `Model:ScaleTo`, the one model-scaling operation Roblox does document.
fn grab_box(faces: &Faces, ray: Ray) -> Option<Drag> {
    let (grabbed, sign) = faces.grab(ray)?;
    let axis = faces.direction(grabbed) * sign;
    let origin = faces.centre();
    let extent = faces.extent(grabbed);
    Some(Drag::Box {
        origin,
        axis,
        grabbed: gizmo::along_axis(origin, axis, ray)?,
        extent,
        pivot: origin - axis * extent * 0.5,
    })
}

/// What this cursor ray does to the part, and the drag state the next step is
/// measured from. `None` when the gesture has no answer at this angle — an
/// axis sighted end-on, or a drag plane the ray has turned parallel to (or
/// ended up behind).
///
/// Pure, and the whole of what a drag computes: [`Drag`] is a grab's worth of
/// geometry, and this turns it plus a ray into the part's new placement.
///
/// `landing`'s soft-snap surfaces bear on the Move tool's body grab alone;
/// its grids on every tool — the stud increment on a Move or Scale travel
/// (`creator-docs`, `parts/index.md#transform-parts`: increments "are based
/// on studs for moving/scaling"), the degree increment on a Rotate. What is
/// rounded is the *travel* since the handle was grabbed — the studs slid or
/// grown, the angle swept — not the part's world position, size or heading.
/// The docs say only that increments are "based on studs" and never where the
/// grid is anchored; rounding the travel is what keeps a part that already
/// stood off-grid from jumping the moment it is picked up, and it is the one
/// reading that means the same thing for a dragger along a local axis as for
/// one along a world axis.
pub(super) fn advance(drag: Drag, ray: Ray, landing: Landing) -> Option<(Drag, Change)> {
    match drag {
        Drag::Axis {
            origin,
            axis,
            grabbed,
        } => {
            let travel = gizmo::along_axis(origin, axis, ray)? - grabbed;
            let travel = snap::round_to(travel, landing.grid);
            Some((drag, Change::Position(origin + axis * travel)))
        }
        Drag::Plane {
            point,
            normal,
            offset,
        } => {
            let hit = pick::ray_hits_plane(ray, point, normal)?;
            Some((
                drag,
                Change::Position(grabbed_at(hit, point, landing) + offset),
            ))
        }
        Drag::Size {
            origin,
            axis,
            grabbed,
            size,
            component,
        } => {
            let travelled = snap::round_to(
                gizmo::along_axis(origin, axis, ray)? - grabbed,
                landing.grid,
            );
            let mut resized = size;
            resized[component] = (size[component] + travelled).clamp(MIN_SIZE, MAX_SIZE);
            // Half the growth, so the face opposite the grabbed one holds
            // still and the grabbed one follows the cursor. Taken from what
            // the size *actually* changed by rather than from the travel, so
            // a drag that has run into either end of the range stops moving
            // the part as well as stops resizing it.
            let grown = resized[component] - size[component];
            Some((
                drag,
                Change::Size {
                    size: resized,
                    position: origin + axis * grown * 0.5,
                },
            ))
        }
        Drag::Box {
            origin,
            axis,
            grabbed,
            extent,
            pivot,
        } => {
            let travelled = snap::round_to(
                gizmo::along_axis(origin, axis, ray)? - grabbed,
                landing.grid,
            );
            // The box's own length along the pulled axis, held to the same
            // range a part's Size is; what every part scales by is how much
            // that grew or shrank in proportion.
            let pulled = (extent + travelled).clamp(MIN_SIZE, MAX_SIZE);
            Some((
                drag,
                Change::Scaled {
                    pivot,
                    factor: pulled / extent,
                },
            ))
        }
        Drag::Ring {
            origin,
            frame,
            orientation,
            last,
            turned,
        } => {
            let (angle, ..) = gizmo::ring_crossing(origin, frame, ray)?;
            let turned = turned + gizmo::angle_step(last, angle);
            let (turn, ..) = frame;
            // The running total is kept exact and only the angle *applied* is
            // rounded: rounding each step before adding it up would let a
            // slow drag creep past the increments without ever crossing one.
            let applied = snap::round_to(turned, landing.angle);
            Some((
                Drag::Ring {
                    origin,
                    frame,
                    orientation,
                    last: angle,
                    turned,
                },
                Change::Orientation(Mat3::from_axis_angle(turn, applied) * orientation),
            ))
        }
    }
}

/// One step of a gesture: the target as `change` leaves it and whether this is
/// the gesture's first step to change anything — the one that opens its undo
/// entry — or `None` for a sample that leaves the part exactly where it is.
///
/// `dragged` is marked only on a real change, never on a sample. The first
/// samples of a snapped Move round their travel to nothing until the cursor
/// has crossed half an increment, and a gesture that spent its "first" on
/// one of those would write every later step without ever pushing history:
/// the whole drag would then sit outside undo.
pub(super) fn stepped(
    target: Target,
    change: Change,
    dragged: &mut bool,
) -> Option<(Target, bool)> {
    let moved = applied(target, change)?;
    Some((moved, !std::mem::replace(dragged, true)))
}

/// The target as this change leaves it, or `None` when it leaves it exactly as
/// it was — which is what keeps a cursor that has not actually moved the part
/// from opening an undo step.
pub(super) fn applied(target: Target, change: Change) -> Option<Target> {
    match change {
        Change::Position(position) => {
            (position != target.position()).then(|| target.moved_to(position))
        }
        Change::Size { size, position } => (size != target.size() || position != target.position())
            .then(|| target.resized_to(size, position)),
        Change::Orientation(orientation) => {
            (orientation != target.orientation()).then(|| target.rotated_to(orientation))
        }
        Change::Scaled { pivot, factor } => (factor != 1.0).then(|| {
            target.resized_to(
                target.size() * factor,
                pivot + (target.position() - pivot) * factor,
            )
        }),
    }
}

/// Where a free drag's grab point actually settles: on the grid, or — with no
/// grid in force — soft-snapped onto whatever surface or edge it is passing.
///
/// The two are alternatives rather than both, which is what `creator-docs`
/// describes for cursor dragging: with snapping enabled a ruler shows the
/// alignment instead, and "if snapping is **disabled**, the part will
/// 'soft&nbsp;snap' to surfaces and edges of nearby parts"
/// (`parts/index.md#transform-parts`). The docs' other soft-snap sentence —
/// dragging a part *by its pivot* under the Move tool — places no condition on
/// the snap setting at all, but this editor has no pivot handle to drag
/// separately from the part's body, so there is nothing yet to treat
/// differently.
fn grabbed_at(hit: Vec3, grabbed: Vec3, landing: Landing) -> Vec3 {
    if landing.grid > 0.0 {
        return grabbed + snap::round_point(hit - grabbed, landing.grid);
    }
    snap::nearest_surface(hit, landing.neighbours, landing.reach)
        .map_or(hit, |surface| surface.point)
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
