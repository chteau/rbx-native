//! The Explorer & Output, Layout and Accessibility pages.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::kit::{icon, secondary_button, segmented, still_toggle, text, toggle, Row, Section};
use super::nav::Page;
use super::SettingsWindow;

/// Default services' grid: the services Studio's Explorer lists, and
/// whether the default view shows each (`rbx_viewer::services`).
const SERVICES: [&str; 12] = [
    "Workspace",
    "Players",
    "Lighting",
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPlayer",
    "SoundService",
    "Teams",
    "Chat",
    "TextChatService",
];

impl SettingsWindow {
    pub(super) fn explorer_output_page(&mut self, cx: &mut Context<Self>) -> Vec<Section> {
        let (all, increment, expand, timestamps) = {
            let shell = self.shell.read(cx);
            (
                shell.show_all_services(),
                shell.increment_names(),
                shell.expand_on_select,
                shell.output_show_timestamps,
            )
        };
        let services = div()
            .grid()
            .grid_cols(4)
            .gap_y(px(6.))
            .gap_x(px(12.))
            .children(SERVICES.map(|name| {
                let shown = rbx_viewer::services::is_default_visible(name);
                h_flex()
                    .h(px(22.))
                    .gap(px(8.))
                    .min_w_0()
                    .items_center()
                    .text_size(px(11.5))
                    .text_color(tokens::text2())
                    .child(
                        h_flex()
                            .flex_none()
                            .size(px(14.))
                            .rounded(px(3.))
                            .items_center()
                            .justify_center()
                            .map(|this| {
                                if shown {
                                    this.bg(tokens::check_on())
                                        .text_color(tokens::black())
                                        .child(icon("check", 10.))
                                } else {
                                    this.border_1().border_color(tokens::border2())
                                }
                            }),
                    )
                    .child(div().truncate().child(name))
            }));
        let explorer = Section::new(
            "Explorer",
            vec![
                Row::new(
                    "Show all services",
                    toggle("all-services", all, self.set(move |shell, cx| shell.set_show_all_services(!all, cx))),
                )
                .describe("Include services Roblox hides by default.")
                .changed(all, |shell, cx| shell.set_show_all_services(false, cx)),
                Row::new(
                    "Increment names",
                    toggle(
                        "increment-names",
                        increment,
                        self.set(move |shell, cx| shell.set_increment_names(!increment, cx)),
                    ),
                )
                .describe("A pasted or new Part next to \u{201c}Part\u{201d} becomes \u{201c}Part2\u{201d}.")
                .changed(!increment, |shell, cx| shell.set_increment_names(true, cx)),
                Row::new(
                    "Expand to selection",
                    toggle(
                        "expand-selection",
                        expand,
                        self.set(move |shell, cx| shell.set_expand_on_select(!expand, cx)),
                    ),
                )
                .describe("Open the tree down to whatever you select in the viewport.")
                .changed(!expand, |shell, cx| shell.set_expand_on_select(true, cx)),
                Row::new("Default services", div())
                    .describe("The services listed when Show all services is off.")
                    .soon()
                    .below(services),
                Row::new(
                    "Folder colours",
                    text(11.5, 16.).text_color(tokens::text3()).child("Per place"),
                )
                .describe("Set per place from a folder\u{2019}s right-click menu, so they travel with the place."),
            ],
        );
        let output = Section::new(
            "Output",
            vec![Row::new(
                "Timestamps",
                toggle(
                    "timestamps",
                    timestamps,
                    self.set(move |shell, cx| shell.set_output_timestamps(!timestamps, cx)),
                ),
            )
            .describe("Show the time before every line. Remembered between launches.")
            .changed(timestamps, |shell, cx| {
                shell.set_output_timestamps(false, cx)
            })],
        );
        vec![explorer, output]
    }

    pub(super) fn layout_page(&mut self, cx: &mut Context<Self>) -> Vec<Section> {
        let collapsed = self.shell.read(cx).output_collapsed;
        let layouts = [
            (
                "Build",
                "Explorer left, Properties right, Output bottom",
                true,
            ),
            ("Scripting", "Script Editor wide, Explorer left", false),
            ("Review", "Argon and Output stacked right", false),
        ];
        let list = v_flex()
            .gap(px(6.))
            .children(layouts.map(|(name, what, active)| {
                h_flex()
                    .h(px(40.))
                    .px(px(12.))
                    .gap(px(10.))
                    .items_center()
                    .border_1()
                    .rounded(px(6.))
                    .map(|this| {
                        if active {
                            this.border_color(tokens::accent_line())
                                .bg(tokens::accent_soft())
                        } else {
                            this.border_color(tokens::border()).bg(tokens::dock())
                        }
                    })
                    .child(
                        div()
                            .text_color(tokens::text2())
                            .child(icon("panels-top-left", 14.)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .text_color(tokens::text2())
                            .child(what),
                    )
                    .when(active, |this| {
                        this.child(
                            text(10.5, 14.)
                                .px(px(6.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::check_on())
                                .child("Active"),
                        )
                    })
            }));
        let docks = Section::new(
            "Docks",
            vec![
                Row::new(
                    "Dock layout",
                    secondary_button("reset-layout", "rotate-ccw", "Reset layout")
                        .on_click(self.set(|shell, cx| shell.reset_layout(cx))),
                )
                .describe("Which panel sits on which edge, and how big. Saved automatically when you close."),
                Row::new(
                    "Output panel",
                    toggle(
                        "output-collapsed",
                        collapsed,
                        self.set(move |shell, cx| shell.set_output_collapsed(!collapsed, cx)),
                    ),
                )
                .describe("Start with the Output panel collapsed.")
                .changed(collapsed, |shell, cx| shell.set_output_collapsed(false, cx)),
            ],
        );
        let named = Section::new(
            "Named layouts",
            vec![Row::new("Saved layouts", secondary_button("save-layout", "plus", "Save current\u{2026}"))
                .describe(
                    "Save the current arrangement under a name and switch between them, like Blender workspaces.",
                )
                .soon()
                .below(list)],
        );
        vec![docks, named]
    }

    pub(super) fn accessibility_page(&mut self, cx: &mut Context<Self>) -> Vec<Section> {
        let choice = self.shell.read(cx).reduce_motion;
        let large = tokens::large_targets();
        let motion = segmented(
            "reduce-motion",
            26.,
            [
                ("Follow system", None),
                ("On", Some(true)),
                ("Off", Some(false)),
            ]
            .into_iter()
            .map(|(label, value)| {
                (
                    label,
                    choice == value,
                    self.shell_fn(move |shell, cx| shell.set_reduce_motion(value, cx)),
                )
            })
            .collect(),
        );
        let appearance = h_flex()
            .id("to-appearance")
            .gap(px(4.))
            .items_center()
            .cursor_pointer()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tokens::check_on())
            .child("Appearance")
            .child(icon("chevron-right", 11.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.page = Page::Appearance;
                cx.notify();
            }));
        vec![
            Section::new(
                "Motion",
                vec![Row::new("Reduce motion", motion)
                    .describe(
                        "Cuts panel slides, hover fades and camera easing. Follow system reads your OS setting.",
                    )
                    .changed(choice.is_some(), |shell, cx| shell.set_reduce_motion(None, cx))],
            ),
            Section::new(
                "Targets",
                vec![
                    Row::new(
                        "Large click targets",
                        toggle("large-targets", large, self.set(|shell, cx| shell.toggle_large_targets(cx))),
                    )
                    .describe("Every clickable control grows to at least 44 px, from 24 px.")
                    .changed(large, |shell, cx| shell.toggle_large_targets(cx)),
                    Row::new("Large primary and destructive buttons", still_toggle(false))
                        .describe("Publish, Delete and the like are 44 px tall even with the option above off.")
                        .soon_faded(),
                ],
            ),
            Section::new(
                "Also here",
                vec![Row::new("UI scale", appearance)
                    .describe("Lives in Appearance. It scales text and controls together.")],
            ),
        ]
    }
}
