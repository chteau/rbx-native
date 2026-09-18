//! The transform tools, as the ribbon's Tools group draws them (§2):
//! Select/Move/Scale/Rotate as tight 32x32 icon buttons, the local-axis
//! toggle beside them, then the chevron that opens the snap increments and
//! the Align popover.
//!
//! Studio's fifth **Transform** button is deliberately absent.
//! `creator-docs` only uses "transform" as the umbrella name for
//! Move+Scale+Rotate together and documents no distinct tool behind it, so
//! there is nothing here to implement against yet.

use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::transform::{Action, SnapKind, Tool};
use crate::ui_icons;

use super::ribbon::{TOOL_BUTTON, TOOL_ICON};
use super::Shell;

pub(crate) mod snap;

/// `RBX_STUDIO_TOOL=rotate` / `scale,local` / `move,nosnap` picks a tool and
/// its snapping at startup — a
/// debugging aid for a screenshot of the draggers, since nothing else can
/// click the toolbar or type its shortcut on the editor's behalf (see
/// `AGENTS.md`'s safety rules), exactly as `RBX_STUDIO_SELECT` and
/// `RBX_STUDIO_EDIT` already stand in for a click and a keystroke elsewhere.
pub(super) const TOOL_VARIABLE: &str = "RBX_STUDIO_TOOL";

/// The UI-kit icon standing in for each tool. The name itself still shows
/// up — as the button's tooltip (§6), since a 32x32 button has no room for
/// a label and a tool you use constantly is learned by shape anyway.
fn tool_icon(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::Move => "move",
        Tool::Scale => "scale",
        Tool::Rotate => "rotate",
    }
}

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

    /// The four tools plus the local-axis toggle (§2.1, §2.2).
    pub(super) fn tool_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active = self.transform.tool;
        let local = self.transform.local;

        let mut buttons: Vec<AnyElement> = Tool::ALL
            .map(|tool| {
                tool_button(
                    ("tool", tool as usize),
                    tool_icon(tool),
                    active == tool,
                    format!("{} ({})", tool.label(), tool.shortcut()),
                    cx.listener(move |shell, _, _, cx| {
                        shell.transform_action(Action::Use(tool), cx);
                    }),
                )
                .into_any_element()
            })
            .into_iter()
            .collect();

        // Studio shows an `L` beside the tools while local orientation is on;
        // this is that indicator and its toggle in one.
        buttons.push(
            div()
                .id("tool-local")
                .size(TOOL_BUTTON)
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS_SM)
                .cursor_pointer()
                .text_size(tokens::UI_LABEL_SIZE)
                .font_weight(if local {
                    tokens::UI_LABEL_ACTIVE_WEIGHT
                } else {
                    tokens::UI_LABEL_WEIGHT
                })
                .map(|this| {
                    if local {
                        this.bg(tokens::accent_soft_bg())
                            .text_color(tokens::accent())
                            .hover(|this| this.bg(tokens::accent_soft_bg_hover()))
                    } else {
                        this.text_color(tokens::text_secondary()).hover(|this| {
                            this.bg(tokens::bg_2()).text_color(tokens::text_primary())
                        })
                    }
                })
                .active(|this| this.bg(tokens::bg_3()))
                .tooltip(|window, cx| super::tooltip::text("Local orientation", window, cx))
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.transform_action(Action::ToggleLocal, cx);
                }))
                .child("L")
                .into_any_element(),
        );

        buttons
    }

    /// §2.3/§2.4 — the chevron beside the tools, and the numeric snap
    /// increments it opens. The fields moved off the strip and into here so
    /// the Tools group stays a row of tools rather than a row of tools and
    /// a spreadsheet.
    pub(super) fn snap_popover(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        // Built inside the content closure, from this handle: the popover
        // rebuilds its body every time it opens, and the fields have to be
        // the live `InputState`s rather than a snapshot taken at render.
        let handle = cx.entity();

        Popover::new("snap-popover")
            .appearance(false)
            .trigger(super::chrome::Trigger::new(
                div()
                    .id("snap-chevron")
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::RADIUS_XS)
                    .cursor_pointer()
                    .text_color(tokens::text_secondary())
                    .hover(|this| this.text_color(tokens::text_primary()))
                    .child(ui_icons::icon("chevron-down").size(px(10.))),
            ))
            .content(move |_, _, cx| handle.update(cx, |shell, cx| shell.snap_fields_popover(cx)))
    }

    /// Align keeps its own popover of toggles; only its trigger changes, to
    /// the same 32x32 icon button the tools use.
    pub(super) fn align_control(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        self.align_popover(
            super::chrome::Trigger::new(tool_button(
                "tool-align",
                "align",
                false,
                "Align".to_string(),
                |_, _, _| {},
            )),
            cx,
        )
    }
}

/// One tool button, in every state §2.2 asks for. The pressed state paints
/// `bg-3` instead of scaling the button: GPUI's style system has no
/// transform (see `UX_GUIDELINES.md` §10).
fn tool_button(
    id: impl Into<ElementId>,
    icon: &str,
    active: bool,
    tooltip: String,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let tooltip = SharedString::from(tooltip);

    div()
        .id(id.into())
        .size(TOOL_BUTTON)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS_SM)
        .cursor_pointer()
        .tab_index(super::chrome::RIBBON_CONTROL_INDEX)
        .focus(|this| this.shadow(tokens::focus_ring(tokens::bg_1())))
        .map(|this| {
            if active {
                this.bg(tokens::accent_soft_bg())
                    .text_color(tokens::accent())
                    .hover(|this| this.bg(tokens::accent_soft_bg_hover()))
            } else {
                this.text_color(tokens::text_secondary())
                    .hover(|this| this.bg(tokens::bg_2()).text_color(tokens::text_primary()))
            }
        })
        .active(|this| this.bg(tokens::bg_3()))
        .tooltip(move |window, cx| super::tooltip::text(tooltip.clone(), window, cx))
        .on_click(on_click)
        .child(ui_icons::icon(icon).size(TOOL_ICON))
}

/// §2.4's popover body: the two snap fields stacked, in a container sized
/// and elevated to spec.
pub(super) fn snap_container(fields: Vec<AnyElement>) -> AnyElement {
    v_flex()
        .w(px(160.))
        .p(tokens::SPACE_3)
        .gap(tokens::SPACE_2)
        .bg(tokens::bg_2())
        .rounded(tokens::RADIUS_MD)
        .shadow(tokens::elevation_2())
        .children(fields)
        .into_any_element()
}

/// A field's own label row, above its input (§2.4).
pub(super) fn field_label(label: impl IntoElement) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(tokens::SPACE_1)
        .mb(tokens::SPACE_1)
        .text_size(tokens::UI_LABEL_SIZE)
        .line_height(tokens::UI_LABEL_LINE_HEIGHT)
        .text_color(tokens::text_secondary())
        .child(label)
}
