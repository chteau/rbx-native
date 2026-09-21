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
use rbx_viewer::Segment;

use super::WorkspaceView;
use crate::dragger::surface::SurfaceFrame;
use crate::dragger::sweep::SoftSnap;
use crate::dragger::{free, pixel_size, ruler, Guides};
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
    drawn: Guides,
    /// Studio's distance label: where, in the panel's own logical pixels,
    /// and what it reads.
    pub(super) label: Option<(Point<Pixels>, SharedString)>,
    /// The light guides' segments, drawn alongside these.
    light: Vec<Segment>,
    /// What the render thread was last sent, both kinds together.
    sent: Vec<Segment>,
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

    /// Replaces the light guides' segments (see `Shell::sync_light_guides`).
    pub(super) fn show_light_guides(&mut self, segments: Vec<Segment>) {
        self.guides.light = segments;
        self.send_lines();
    }

    /// The render thread takes one list of segments for everything an editor
    /// draws over the scene, so the light guides and the dragger guides go
    /// together, from here alone — and only when they changed: a hover that
    /// lands on the same grid point sends nothing.
    fn send_lines(&mut self) {
        let mut segments = self.guides.light.clone();
        let height = self.viewport.get().size.1 as f32;
        if let (Some(pose), true) = (self.view, height > 0.0) {
            let orthographic = self.orthographic;
            segments.extend(
                self.guides
                    .drawn
                    .segments(|point| pixel_size(point, pose, orthographic, height)),
            );
        }
        if segments != self.guides.sent {
            self.guides.sent = segments.clone();
            self.pump.lines(segments);
        }
    }
}
