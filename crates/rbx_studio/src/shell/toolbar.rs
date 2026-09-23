//! The transform tools, as the ribbon's Tools group draws them (§2):
//! Select/Move/Scale/Rotate as tight 32x32 icon buttons, the local-axis
//! toggle beside them, then the chevron that opens the snap increments and
//! the Align popover.
//!
//! Studio's fifth **Transform** button is deliberately absent.
//! `creator-docs` only uses "transform" as the umbrella name for
//! Move+Scale+Rotate together and documents no distinct tool behind it, so
//! there is nothing here to implement against yet.

use gpui_kit::assets::IconName;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::transform::{Action, SnapKind, Tool};

use super::ribbon;
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
                "sun" => self.transform_action(Action::Use(Tool::Sun), cx),
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

    /// Studio shows an `L` beside the tools while local orientation is on.
    /// This is that indicator and its toggle, as one more tile in the row.
    pub(super) fn local_tile(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        let local = self.transform.local;

        ribbon::cluster_icon(&self.ribbon_nav, "tool-local", IconName::Axis3d, cx)
            .when(local, |this| ribbon::selected(this, tokens::tool_local()))
            .tooltip(|window, cx| super::tooltip::text("Local orientation", window, cx))
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.transform_action(Action::ToggleLocal, cx);
            }))
    }

    /// The snap increments, as the stack that sits beside the tools: each
    /// row reads back what a drag will round to, and clicking either opens
    /// the fields that set them.
    ///
    /// Read-out and control in one, which is the point — the increments are
    /// checked far more often than they are changed, and a number you have
    /// to open a popover to *see* is a number nobody trusts.
    pub(super) fn snap_stack(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        // Built inside the content closure, from this handle: the popover
        // rebuilds its body every time it opens, and the fields have to be
        // the live `InputState`s rather than a snapshot taken at render.
        let handle = cx.entity();
        let translate = self.transform.translate;
        let rotate = self.transform.rotate;
        let stack = self.ribbon_nav.claim(
            v_flex()
                .id("snap-stack")
                .flex_none()
                .w(tokens::stack_width())
                .h_full()
                .justify_center()
                .gap(px(4.))
                .cursor_pointer()
                .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
                .tooltip(|window, cx| super::tooltip::text("Snap increments", window, cx)),
            cx,
        );

        Popover::new("snap-popover")
            .appearance(false)
            // The board opens the popover 6px under the ribbon's bottom
            // edge. The trigger ends 12px above that edge (the ribbon's
            // 11px bottom padding and its 1px border), and the toolkit
            // already offsets a popover 4px below its trigger.
            .mt(px(14.))
            .trigger(super::chrome::Trigger::with_open(move |open| {
                stack
                    .child(snap_readout(
                        IconName::Magnet,
                        snap::studs(translate.increment),
                        translate.enabled,
                        open,
                    ))
                    .child(snap_readout(
                        IconName::RotateCw,
                        format!("{}°", rotate.increment),
                        rotate.enabled,
                        open,
                    ))
            }))
            .content(move |_, _, cx| handle.update(cx, |shell, cx| shell.snap_fields_popover(cx)))
    }

    /// Align keeps its own popover of toggles; its trigger is one more tile.
    pub(super) fn align_control(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        self.align_popover(
            super::chrome::Trigger::new(
                ribbon::cluster_icon(
                    &self.ribbon_nav,
                    "tool-align",
                    IconName::AlignStartVertical,
                    cx,
                )
                .tooltip(|window, cx| super::tooltip::text("Align selection", window, cx)),
            )
            .accent(tokens::tool_align()),
            cx,
        )
    }
}

/// One row of the snap stack. A disabled increment is still *shown* — it is
/// what snapping would round to the moment it is switched back on — but it
/// reads as inactive rather than as the value in force. While the popover
/// is open both pills take the accent hairline and full-strength text, so
/// the ribbon shows what the popover is editing.
fn snap_readout(icon: IconName, value: String, enabled: bool, open: bool) -> impl IntoElement {
    h_flex()
        .w_full()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(4.))
        .rounded(tokens::RADIUS)
        .bg(tokens::field_select())
        .border_1()
        .border_color(if open {
            tokens::accent_line()
        } else {
            tokens::border()
        })
        .text_size(tokens::text_sm())
        .line_height(tokens::line_xs())
        .text_color(if open {
            tokens::text()
        } else if enabled {
            tokens::text_label()
        } else {
            tokens::text_disabled()
        })
        .child(Icon::new(icon).size(tokens::text_xs()))
        .child(div().flex_1().truncate().child(SharedString::from(value)))
}
