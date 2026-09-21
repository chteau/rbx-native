//! Studio's dragger guides over the 3D view: which of them show for what the
//! cursor is doing right now, built from what the view knows — the camera,
//! the handles, the toolbar — plus the face under the cursor, which only
//! `Shell` can find (see `crate::dragger` for the guides themselves).
//!
//! - Nothing held, Select or Move tool: the hover ruler on the face under the
//!   cursor, from `Shell`'s answer to the hover ([`WorkspaceView::set_hover_target`]).
//! - A free (body) drag: the ruler, or the face alignments, the drag landed
//!   on, from `Shell`'s answer to the step ([`WorkspaceView::settle_at`]).
//! - A Move or Scale handle drag: the axis line, the soft-snap dots found at
//!   the press, and the distance label (see [`handles`]).

use glam::{Mat3, Vec3};
use gpui_kit::*;
use rbx_viewer::gizmo::Axis;
use rbx_viewer::pick::Ray;
use rbx_viewer::Segment;

use super::WorkspaceView;
use crate::dragger::surface::SurfaceFrame;
use crate::dragger::sweep::SoftSnap;
use crate::dragger::{free, pixel_size, round, ruler, Guides};
use crate::settings::DraggerSettings;
use crate::settle::Settled;
use crate::transform::Tool;

mod handles;

pub(super) use handles::label_element;

/// The line layer the dragger guides are drawn on, apart from the light
/// guides' so that a guide moving with the mouse never re-uploads them.
const DRAGGER_GUIDES: usize = 1;

/// Everything the guides keep between one event and the next.
#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) settings: DraggerSettings,
    /// The face under the cursor and where the cursor meets it, as of the
    /// last hover `Shell` resolved.
    hover: Option<(SurfaceFrame, Vec3)>,
    /// The modifiers at that hover, and whether a Move arrow was under the
    /// cursor — Studio shows no hover ruler over a handle.
    modifiers: Modifiers,
    over_handle: bool,
    /// Whether the cursor is over the view at all, so a hover asked for
    /// again (see [`WorkspaceView::rehover`]) is never resolved at a cursor
    /// that has left it.
    inside: bool,
    /// The face a free drag last landed on. Over nothing, the drag keeps
    /// landing in that face's plane, on its grid.
    landed_on: Option<SurfaceFrame>,
    /// The quarter turns `R` and `T` have added to the free drag under way.
    pub(super) tilt: Mat3,
    /// A handle drag's soft snaps, found once at the press.
    snaps: Vec<SoftSnap>,
    /// The Move arrow a handle drag holds: its axis, which end (`±1`) and
    /// how far out along it the press landed.
    arrow: Option<(Axis, f32, f32)>,
    /// What is drawn now.
    drawn: Guides,
    /// Studio's distance label: where, in the panel's own logical pixels,
    /// and what it reads.
    pub(super) label: Option<(Point<Pixels>, SharedString)>,
    /// What the render thread was last sent.
    sent: Vec<Segment>,
    /// Where the cursor last stepped the drag in progress: a modifier
    /// pressed or released with the mouse still re-steps it from there.
    pub(super) dragged_at: Option<Point<Pixels>>,
}

impl WorkspaceView {
    pub(crate) fn set_dragger(&mut self, settings: DraggerSettings) {
        self.guides.settings = settings;
        self.refresh_guides();
    }

    /// Rebuilds the hover ruler after something it depends on changed with
    /// nothing held: the tool, or a setting.
    pub(super) fn refresh_guides(&mut self) {
        if self.drag.is_none() {
            self.show_guides(self.hover_guides(false));
        }
    }

    /// `Shell`'s answer to a hover: the face under the cursor, framed on its
    /// corner nearest the cursor, and where the cursor meets it — `None`
    /// over nothing selectable.
    pub(crate) fn set_hover_target(&mut self, target: Option<(SurfaceFrame, Vec3)>) {
        if self.drag.is_some() {
            return;
        }
        self.guides.hover = target;
        self.show_guides(self.hover_guides(false));
    }

    /// Notes what a hover ray is doing that `Shell` does not see: the
    /// modifiers, and whether it points at one of the Move tool's arrows.
    pub(super) fn note_hover(&mut self, ray: Option<Ray>, modifiers: Modifiers) {
        self.guides.modifiers = modifiers;
        self.guides.inside = true;
        self.guides.over_handle = self.transform.tool == Tool::Move
            && ray
                .zip(self.handles())
                .is_some_and(|(ray, handles)| handles.grab(ray).is_some());
    }

    /// Studio's hover ruler, when it shows: the Select or Move tool, nothing
    /// held, the ruler setting and the toolbar's snapping both on, and no
    /// handle under the cursor. `pending` is a press on the selection that
    /// has not moved yet.
    fn hover_guides(&self, pending: bool) -> Guides {
        let state = &self.guides;
        let snap = self.transform.translate;
        let shows = state.settings.show_hover_ruler
            && matches!(self.transform.tool, Tool::Select | Tool::Move)
            && snap.enabled
            && !state.over_handle;
        let (Some((frame, hit)), Some(pose), true) = (state.hover, self.view, shows) else {
            return Guides::default();
        };
        let mut guides = ruler::hover(
            &frame,
            hit,
            snap.increment,
            snap.active(state.modifiers.shift),
            pending,
            pose,
            self.orthographic,
        );
        guides.lines.extend(round::major_lines(&frame));
        guides
    }

    /// The grid a hover's target frame is snapped in, which only a ball's
    /// and a cylinder's side are: the grid in force at the last hover
    /// (Studio's `shouldGridSnap`, off while `Shift` is held).
    pub(crate) fn hover_grid(&self) -> f32 {
        self.transform.translate.grid(self.guides.modifiers.shift)
    }

    /// The frame the hover under the cursor stands on, which a body grab
    /// snaps the grabbed point in, and where the cursor meets it.
    pub(super) fn hovered(&self) -> Option<(SurfaceFrame, Vec3)> {
        self.guides.hover
    }

    /// Resolves the hover again where the cursor stands, next frame: after an
    /// edit, an undo or a delete moved or removed the face the ruler was
    /// measuring, after a drag, and after a tool switch — none of which moves
    /// the mouse, and a ruler left on a face that has gone is a lie.
    pub(super) fn rehover(&mut self) {
        if self.drag.is_some() || self.looking || !self.guides.inside {
            return;
        }
        if let Some(at) = self.cursor {
            self.hover_pending = Some((at, self.guides.modifiers));
        }
    }

    /// The cursor left the view: nothing is hovered until it comes back.
    pub(super) fn left_view(&mut self) {
        self.guides.inside = false;
    }

    /// A body grab going ahead: the hover dot turns yellow until the first
    /// step lands the drag.
    pub(super) fn pend_guides(&mut self) {
        self.show_guides(self.hover_guides(true));
        self.guides.label = None;
    }

    /// Everything a gesture drew goes when it ends.
    pub(super) fn clear_guides(&mut self) {
        self.show_guides(Guides::default());
        let state = &mut self.guides;
        state.label = None;
        state.snaps.clear();
        state.arrow = None;
        state.landed_on = None;
        state.tilt = Mat3::IDENTITY;
        state.dragged_at = None;
    }

    /// `Shift` or `Alt` pressed or released mid-drag, or a quarter turn:
    /// Studio re-lands the drag on the change — onto the grid or off it,
    /// onto the soft snaps or off them, turned onto the face or held as
    /// grabbed — without waiting for the mouse, so the drag is stepped again
    /// where the cursor stands (once per frame, like any step: see
    /// `step_drag`).
    pub(super) fn modifiers_changed(&mut self, modifiers: Modifiers) {
        if let Some(position) = self
            .drag_pending
            .map(|(at, _)| at)
            .or(self.guides.dragged_at)
        {
            self.drag_pending = Some((position, modifiers));
        }
    }

    /// Whether a drag snaps to parts this move: the setting on and `Shift` up
    /// (Studio's `shouldPartSnap`).
    fn part_snap(&self, shift: bool) -> bool {
        self.guides.settings.snap_to_parts && !shift
    }

    /// The soft snaps a handle drag can take this move.
    pub(super) fn soft_snaps(&self, shift: bool) -> &[SoftSnap] {
        if self.part_snap(shift) {
            &self.guides.snaps
        } else {
            &[]
        }
    }

    /// The face the last free-drag step landed on, for the next step to fall
    /// back to over nothing.
    pub(super) fn landed_on(&self) -> Option<SurfaceFrame> {
        self.guides.landed_on
    }

    /// `Shell`'s answer to a free-drag step: the face it landed on, drawn the
    /// way Studio draws it, or nothing over empty space.
    pub(super) fn landed(&mut self, settled: Option<&Settled>) {
        let settings = self.guides.settings;
        let drawn = match (settled, self.view) {
            (Some(settled), Some(pose)) => {
                self.guides.landed_on = Some(settled.frame);
                free::guides(
                    &settled.frame,
                    settled.hit,
                    &settled.landing,
                    settled.grid,
                    settings.show_target_snap,
                    settings.show_dragged_point,
                    pose,
                    self.orthographic,
                )
            }
            _ => Guides::default(),
        };
        self.show_guides(drawn);
    }

    /// Replaces what the dragger guides draw.
    fn show_guides(&mut self, guides: Guides) {
        self.guides.drawn = guides;
        self.send_lines();
    }

    /// Sends the render thread the segments the guides draw now, on their
    /// own line layer — only when they changed: a hover that lands on the
    /// same grid point sends nothing. Also what follows a resized view, whose
    /// screen-constant widths are pixels of the old height.
    pub(super) fn send_lines(&mut self) {
        let height = self.viewport.get().size.1 as f32;
        let segments = match (self.view, height > 0.0) {
            (Some(pose), true) => {
                let orthographic = self.orthographic;
                self.guides
                    .drawn
                    .segments(|point| pixel_size(point, pose, orthographic, height))
            }
            _ => Vec::new(),
        };
        if segments != self.guides.sent {
            self.guides.sent = segments.clone();
            self.pump.lines(DRAGGER_GUIDES, segments);
        }
    }
}
