//! The view's half of the Terrain Editor (see `crate::terrain`): every press,
//! drag step, release and hover goes to `Shell` as a ray, and `Shell`'s
//! answer — the brush or region outline — comes back to be drawn on a line
//! layer of its own.
//!
//! `B` held turns the wheel into the brush's size, `Ctrl`+`B` its height and
//! `Shift`+`B` its strength (`terrain-editor.md`'s brush shortcuts).

use gpui_kit::{Context, Modifiers, Pixels, Point, ScrollDelta};
use rbx_viewer::pick::Ray;
use rbx_viewer::{Pose, Segment};

use super::input::wheel_notches;
use super::{ViewportAction, WorkspaceView};

/// The line layer the brush and region outlines draw on (0 is the light
/// guides', 1 the dragger guides').
const TERRAIN_LINES: usize = 2;
/// Pixels of sideways motion with `B` held per stud (or strength step).
const PIXELS_PER_STEP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TerrainPhase {
    Hover,
    Press,
    Drag,
    Release,
    /// `Delete`/`Backspace` over the view: the Select tool's region delete.
    Delete,
    /// `Alt`-click: the material picker, at this point in the window.
    Picker {
        x: f32,
        y: f32,
    },
    /// The wheel with `B` held: notches toward the user are negative.
    Adjust {
        dial: Dial,
        notches: f32,
    },
}

/// What `B` and the wheel adjust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dial {
    Size,
    Height,
    Strength,
}

/// One input to the Terrain Editor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TerrainInput {
    /// `None` only for a release, which needs no aim.
    pub(crate) ray: Option<Ray>,
    pub(crate) phase: TerrainPhase,
    pub(crate) ctrl: bool,
    pub(crate) shift: bool,
    /// The camera the ray was cast from, for handle sizes and Auto plane
    /// lock's facing.
    pub(crate) pose: Option<Pose>,
    pub(crate) orthographic: bool,
}

impl WorkspaceView {
    pub(super) fn terrain_input(
        &mut self,
        ray: Ray,
        phase: TerrainPhase,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        self.terrain_modifiers = modifiers;
        cx.emit(ViewportAction::Terrain(TerrainInput {
            ray: Some(ray),
            phase,
            ctrl: modifiers.control || modifiers.platform,
            shift: modifiers.shift,
            pose: self.view,
            orthographic: self.orthographic,
        }));
    }

    /// The cursor moving with nothing held: the brush follows it, unless
    /// `B` is down, when sideways motion turns the brush's dial instead
    /// (and the brush stays put, the way a wheel turn leaves it).
    pub(super) fn terrain_hover(
        &mut self,
        ray: Ray,
        position: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(position.x);
        let last = self.terrain_dial_x.replace(x);
        if self.terrain_b {
            if let Some(last) = last {
                self.terrain_adjust(modifiers, (x - last) / PIXELS_PER_STEP, cx);
            }
            return;
        }
        self.terrain_input(ray, TerrainPhase::Hover, modifiers, cx);
    }

    fn terrain_adjust(&mut self, modifiers: Modifiers, notches: f32, cx: &mut Context<Self>) {
        let dial = if modifiers.shift {
            Dial::Strength
        } else if modifiers.control || modifiers.platform {
            Dial::Height
        } else {
            Dial::Size
        };
        cx.emit(ViewportAction::Terrain(TerrainInput {
            ray: None,
            phase: TerrainPhase::Adjust { dial, notches },
            ctrl: modifiers.control,
            shift: modifiers.shift,
            pose: self.view,
            orthographic: self.orthographic,
        }));
    }

    pub(super) fn terrain_release(&mut self, cx: &mut Context<Self>) {
        let modifiers = self.terrain_modifiers;
        cx.emit(ViewportAction::Terrain(TerrainInput {
            ray: None,
            phase: TerrainPhase::Release,
            ctrl: modifiers.control || modifiers.platform,
            shift: modifiers.shift,
            pose: self.view,
            orthographic: self.orthographic,
        }));
    }

    /// `B`'s state, from every key event over the view, and `Delete`
    /// while the Terrain Editor has the view. Whether the key was taken.
    pub(super) fn terrain_key(&mut self, key: &str, pressed: bool, cx: &mut Context<Self>) -> bool {
        use crate::shortcuts::{self, Command};
        let keystroke = gpui_kit::Keystroke {
            key: key.into(),
            modifiers: Modifiers::none(),
            key_char: None,
        };
        if shortcuts::pressed(Command::TerrainBrush, &keystroke) {
            self.terrain_b = pressed;
        }
        let terrain = self.transform.tool == crate::transform::Tool::Terrain;
        if terrain && pressed && shortcuts::pressed(Command::TerrainDelete, &keystroke) {
            cx.emit(ViewportAction::Terrain(TerrainInput {
                ray: None,
                phase: TerrainPhase::Delete,
                ctrl: false,
                shift: false,
                pose: self.view,
                orthographic: self.orthographic,
            }));
            return true;
        }
        false
    }

    /// The wheel while the Terrain Editor is up with `B` held: the brush's
    /// dial, never the camera. Whether it was taken.
    pub(super) fn terrain_wheel(
        &mut self,
        delta: ScrollDelta,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.transform.tool != crate::transform::Tool::Terrain || !self.terrain_b {
            return false;
        }
        self.terrain_adjust(modifiers, wheel_notches(delta), cx);
        true
    }

    /// A terrain edit in progress, straight to the renderer — see
    /// `Headless::preview_terrain`.
    pub(crate) fn preview_terrain(&self, chunks: super::pump::TerrainChunks) {
        if !chunks.is_empty() {
            self.pump.terrain_preview(chunks);
        }
    }

    /// `Shell`'s answer: the outline to draw, or nothing to clear it.
    pub(crate) fn show_terrain(&mut self, segments: Vec<Segment>) {
        self.pump.lines(TERRAIN_LINES, segments);
    }
}
