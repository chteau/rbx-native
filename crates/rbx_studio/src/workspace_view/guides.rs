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

use glam::Vec3;
use gpui_kit::*;
use rbx_viewer::gizmo::Axis;
use rbx_viewer::pick::Ray;

use super::WorkspaceView;
use crate::dragger::surface::SurfaceFrame;
use crate::dragger::sweep::SoftSnap;
use crate::dragger::{free, ruler, Guides};
use crate::settings::DraggerSettings;
use crate::settle::Settled;
use crate::transform::Tool;

mod handles;

pub(super) use handles::label_element;

/// Everything the guides keep between one event and the next.
#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) settings: DraggerSettings,
    /// The face under the cursor and where the cursor meets it, as of the
    /// last hover `Shell` resolved.
    hover: Option<(SurfaceFrame, Vec3)>,
    /// `Shift` at that hover, and whether a Move arrow was under the cursor —
    /// Studio shows no hover ruler over a handle.
    shift: bool,
    over_handle: bool,
    /// The face a free drag last landed on. Over nothing, the drag keeps
    /// landing in that face's plane, on its grid.
    landed_on: Option<SurfaceFrame>,
    /// A handle drag's soft snaps, found once at the press.
    snaps: Vec<SoftSnap>,
    /// The Move arrow a handle drag holds: its axis, which end (`±1`) and
    /// how far out along it the press landed.
    arrow: Option<(Axis, f32, f32)>,
    /// What is drawn now.
    pub(super) drawn: Guides,
    /// Studio's distance label: where, in the panel's own logical pixels,
    /// and what it reads.
    pub(super) label: Option<(Point<Pixels>, SharedString)>,
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
            self.guides.drawn = self.hover_guides(false);
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
        self.guides.drawn = self.hover_guides(false);
    }

    /// Notes what a hover ray is doing that `Shell` does not see: `Shift`,
    /// and whether it points at one of the Move tool's arrows.
    pub(super) fn note_hover(&mut self, ray: Option<Ray>, shift: bool) {
        self.guides.shift = shift;
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
        ruler::hover(
            &frame,
            hit,
            snap.increment,
            snap.active(state.shift),
            pending,
            pose,
            self.orthographic,
        )
    }

    /// A body grab going ahead: the hover dot turns yellow until the first
    /// step lands the drag.
    pub(super) fn pend_guides(&mut self) {
        self.guides.drawn = self.hover_guides(true);
        self.guides.label = None;
    }

    /// Everything a gesture drew goes when it ends.
    pub(super) fn clear_guides(&mut self) {
        let state = &mut self.guides;
        state.drawn = Guides::default();
        state.label = None;
        state.snaps.clear();
        state.arrow = None;
        state.landed_on = None;
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
        self.guides.drawn = match (settled, self.view) {
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
    }
}
