//! The transform toolbar: the strip of tool buttons directly under the menu
//! bar and above the dock, where Studio's own is (see `Render for Shell`).
//!
//! Select, Move, Scale and Rotate are all live.
//!
//! Studio's fifth **Transform** button is deliberately absent. `creator-docs`
//! only uses "transform" as the umbrella name for Move+Scale+Rotate together
//! and documents no distinct tool behind it, so there is nothing here to
//! implement against yet.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, ActiveTheme, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::transform::{Action, SnapKind, Tool};

use super::Shell;

pub(crate) mod snap;

/// `RBX_STUDIO_TOOL=rotate` / `scale,local` / `move,nosnap` picks a tool and
/// its snapping at startup — a
/// debugging aid for a screenshot of the draggers, since nothing else can
/// click the toolbar or type its shortcut on the editor's behalf (see
/// `AGENTS.md`'s safety rules), exactly as `RBX_STUDIO_SELECT` and
/// `RBX_STUDIO_EDIT` already stand in for a click and a keystroke elsewhere.
pub(super) const TOOL_VARIABLE: &str = "RBX_STUDIO_TOOL";

impl Shell {
    /// Applies [`TOOL_VARIABLE`], if it is set to anything this understands.
    pub(super) fn apply_debug_tool(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(TOOL_VARIABLE) else {
            return;
        };

        for word in spec.split(',').map(str::trim) {
            match word.to_ascii_lowercase().as_str() {
                "select" => self.transform_action(Action::Use(Tool::Select), cx),
                "move" => self.transform_action(Action::Use(Tool::Move), cx),
                "scale" => self.transform_action(Action::Use(Tool::Scale), cx),
                "rotate" => self.transform_action(Action::Use(Tool::Rotate), cx),
                "local" => self.transform_action(Action::ToggleLocal, cx),
                "nosnap" => self.transform_action(Action::ToggleSnap(SnapKind::Translate), cx),
                other => eprintln!("rbxstudio: {TOOL_VARIABLE}: no tool called {other:?}"),
            }
        }
    }
    /// Applies a toolbar action, whether it came from a control here or from a
    /// shortcut typed over the 3D view.
    ///
    /// `FocusIncrement` is the one action that changes no state: it moves the
    /// caret, which needs a `Window` this path does not have, so the shortcut
    /// is handled where one is (see `Shell::focus_snap_increment`).
    pub(crate) fn transform_action(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Use(tool) => self.transform.tool = tool,
            Action::ToggleLocal => self.transform.local = !self.transform.local,
            Action::ToggleSnap(kind) => {
                let snap = self.snap_mut(kind);
                snap.enabled = !snap.enabled;
            }
            Action::SetIncrement(kind, increment) => self.snap_mut(kind).increment = increment,
            Action::FocusIncrement(_) => {}
        }

        let transform = self.transform;
        self.viewport
            .update(cx, |viewport, _| viewport.set_transform(transform));
        cx.notify();
    }

    fn snap_mut(&mut self, kind: SnapKind) -> &mut crate::transform::Snap {
        match kind {
            SnapKind::Translate => &mut self.transform.translate,
            SnapKind::Rotate => &mut self.transform.rotate,
        }
    }

    /// The strip itself.
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.transform.tool;
        let local = self.transform.local;

        h_flex()
            .w_full()
            .h(px(32.))
            .flex_none()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(Tool::ALL.map(|tool| {
                Button::new(("transform-tool", tool as usize))
                    .label(format!("{} ({})", tool.label(), tool.shortcut()))
                    .xsmall()
                    .selected(active == tool)
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.transform_action(Action::Use(tool), cx);
                    }))
            }))
            .child(
                Button::new("transform-local")
                    .label("L")
                    .ghost()
                    .xsmall()
                    .selected(local)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.transform_action(Action::ToggleLocal, cx);
                    })),
            )
            // Studio shows an `L` beside the tools while local orientation is
            // on; the button above doubles as that indicator, and this spells
            // out what it means rather than leaving a bare letter.
            .when(local, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("local"),
                )
            })
            // Studio's own toolbar puts the snap increments in the same strip,
            // to the right of the transform tools they apply to.
            .child(div().w(px(1.)).h(px(18.)).bg(cx.theme().border).mx_2())
            .child(self.snap_controls(cx))
    }
}
