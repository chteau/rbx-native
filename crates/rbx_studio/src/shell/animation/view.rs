//! The document's layout, Blender's Animation workspace: the sub-tab strip
//! on top, the 3D view with the side panel to its right, the dope sheet (or
//! the Curve Editor, or the graph canvas) under them, and the playback bar along the bottom.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Mode, Shell};
use crate::shell::chrome;
use crate::tokens;

/// The bottom panel's share of the document's height, and its bounds: room
/// for an R15 rig's fifteen joints when there is room at all.
const SHEET_SHARE: f32 = 0.46;
const SHEET_RANGE: (f32, f32) = (170.0, 380.0);

impl Shell {
    /// The Animation document. Each render also advances playback and poses
    /// the rig, so what the 3D view shows is always the playhead's pose.
    pub(in crate::shell) fn animation_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.anim.bound.is_none() {
            self.bind_selected_rig(cx);
        }
        self.stage_rig(cx);
        self.sync_joint_from_selection();
        if !self.anim.fitted && self.anim.sheet.get().is_some() {
            self.fit_timeline();
        }
        self.animation_frame(window, cx);

        let strip = self.animation_strip(cx);
        let side = match self.anim.mode {
            Mode::DopeSheet | Mode::Curves => self.animation_panel(window, cx),
            Mode::Graph => self.anim_graph_panel(window, cx),
        };
        let bottom = match self.anim.mode {
            Mode::DopeSheet => self.dope_sheet(cx),
            Mode::Curves => self.curve_editor(cx),
            Mode::Graph => self.anim_graph_canvas(cx),
        };
        let transport = self.transport(cx);
        v_flex()
            .id("animation-editor")
            .size_full()
            .bg(tokens::dock())
            .child(strip)
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(120.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.viewport()),
                    )
                    .child(side),
            )
            .child(
                div()
                    .h(relative(SHEET_SHARE))
                    .min_h(px(SHEET_RANGE.0))
                    .max_h(px(SHEET_RANGE.1))
                    .flex_none()
                    .border_t(px(1.))
                    .border_color(tokens::border())
                    .overflow_hidden()
                    .child(bottom),
            )
            .child(transport)
            .into_any_element()
    }

    /// The channel the selection points at: a rig part picked in the 3D view
    /// or the Explorer selects its joint's row.
    fn sync_joint_from_selection(&mut self) {
        let Some(&selected) = self.selected_all().first() else {
            return;
        };
        let Some(bound) = &self.anim.bound else {
            return;
        };
        if let Some(joint) = bound.rig.joint_driving(selected) {
            let name = bound.rig.part_name(joint.part1).to_owned();
            if self.anim.editor.joint.as_deref() != Some(&name) {
                self.anim.editor.joint = Some(name);
            }
        }
    }

    fn animation_strip(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.anim.mode;
        let tabs = Mode::ALL
            .into_iter()
            .map(|mode| {
                chrome::tab_pill(mode.key(), mode.label().into(), mode == active, false)
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.anim.mode = mode;
                        shell.anim.graph.playing = false;
                        shell.anim.graph.pose = None;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let e = &self.anim.editor;
        let title = format!("{}{}", e.clip.name, if e.dirty { " •" } else { "" });
        let (can_undo, can_redo) = (e.can_undo(), e.can_redo());
        let tools = h_flex()
            .gap(px(2.))
            .items_center()
            .child(
                div()
                    .px(px(8.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_label())
                    .child(title),
            )
            .child(
                tool(
                    "anim-undo",
                    IconName::Undo2,
                    "Undo clip edit (Ctrl+Z)",
                    can_undo,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.undo();
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "anim-redo",
                    IconName::Redo2,
                    "Redo clip edit (Ctrl+Y)",
                    can_redo,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.redo();
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "anim-fit",
                    IconName::Maximize2,
                    "Fit the clip to the timeline (F)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.fit_timeline();
                    cx.notify();
                })),
            )
            .into_any_element();
        div()
            .w_full()
            .flex_none()
            .bg(tokens::dock())
            .child(chrome::dock_strip(tabs, Some(tools), true))
            .into_any_element()
    }

    /// Blender's playback bar: jump to start, previous key, play/pause,
    /// next key, jump to end, the current frame, and the clip's range.
    fn transport(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let e = &self.anim.editor;
        let playing = e.playing || self.anim.graph.playing;
        let end = (e.clip.length() * e.fps as f32).round() as i64;
        let frame = e.frame();
        h_flex()
            .id("anim-transport")
            .w_full()
            .h(tokens::dock_tabs_height())
            .flex_none()
            .items_center()
            .gap(px(2.))
            .px(px(8.))
            .bg(tokens::chrome())
            .border_t(px(1.))
            .border_color(tokens::border())
            .child(caption("Playback"))
            .child(div().flex_1())
            .child(
                tool(
                    "anim-start",
                    IconName::ChevronFirst,
                    "Jump to start (Home)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.playing = false;
                    shell.anim.editor.set_time(0.0);
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "anim-prev-key",
                    IconName::SkipBack,
                    "Previous key (Down)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.jump_key(false);
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "anim-prev-frame",
                    IconName::ChevronLeft,
                    "Previous frame (Left)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.step_frames(-1);
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "anim-play",
                    if playing {
                        IconName::Pause
                    } else {
                        IconName::Play
                    },
                    "Play / pause (Space)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| shell.toggle_animation_playback(cx))),
            )
            .child(
                tool(
                    "anim-next-frame",
                    IconName::ChevronRight,
                    "Next frame (Right)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.step_frames(1);
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "anim-next-key",
                    IconName::SkipForward,
                    "Next key (Up)",
                    true,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.editor.jump_key(true);
                    cx.notify();
                })),
            )
            .child(
                tool("anim-end", IconName::ChevronLast, "Jump to end (End)", true).on_click(
                    cx.listener(|shell, _, _, cx| {
                        let length = shell.anim.editor.clip.length();
                        shell.anim.editor.playing = false;
                        shell.anim.editor.set_time(length);
                        cx.notify();
                    }),
                ),
            )
            .child(div().flex_1())
            .child(readout("Frame", frame.to_string()))
            .child(readout("Start", "0".into()))
            .child(readout("End", end.to_string()))
            .into_any_element()
    }

    pub(super) fn toggle_animation_playback(&mut self, cx: &mut Context<Self>) {
        if self.anim.mode == Mode::Graph {
            self.anim_toggle_graph_playback();
        } else {
            self.anim.editor.toggle_play();
        }
        self.anim.last_frame = None;
        cx.notify();
    }
}

fn tool(id: &'static str, icon: IconName, label: &'static str, enabled: bool) -> Stateful<Div> {
    chrome::icon_button(id, icon, label)
        .debug_selector(move || id.into())
        .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
}

fn caption(text: &'static str) -> impl IntoElement {
    div()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_label())
        .child(text)
}

fn readout(name: &'static str, value: String) -> impl IntoElement {
    h_flex()
        .gap(px(6.))
        .ml(px(10.))
        .items_center()
        .child(
            div()
                .text_size(tokens::text_xs())
                .text_color(tokens::text_muted())
                .child(name),
        )
        .child(
            div()
                .min_w(px(40.))
                .h(px(22.))
                .px(px(8.))
                .rounded(px(4.))
                .bg(tokens::field_select())
                .flex()
                .items_center()
                .justify_end()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_strong())
                .child(value),
        )
}
