//! The Explorer & Output, Layout and Accessibility pages.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::kit::{ghost_icon, icon, secondary_button, segmented, text, toggle, Row, Section};
use super::nav::Page;
use super::SettingsWindow;

/// Default services' grid: the services a place usually holds, each ticked
/// when the Explorer's default view lists it (`explorer::is_listed`) and
/// flipped by a click.
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
        let (all, increment, expand, timestamps, overrides) = {
            let shell = self.shell.read(cx);
            (
                shell.show_all_services(),
                shell.increment_names(),
                shell.expand_on_select,
                shell.output_show_timestamps,
                shell.service_overrides().clone(),
            )
        };
        let services = div()
            .grid()
            .grid_cols(4)
            .gap_y(px(6.))
            .gap_x(px(12.))
            .children(SERVICES.map(|name| {
                let shown = crate::explorer::is_listed(name, &overrides);
                h_flex()
                    .id(SharedString::from(format!("service-{name}")))
                    .cursor_pointer()
                    .on_click(self.set(move |shell, cx| shell.toggle_default_service(name, cx)))
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
                    .describe("Click a service to list or hide it while Show all services is off.")
                    .changed(!overrides.is_empty(), |shell, cx| {
                        shell.reset_service_overrides(cx)
                    })
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
        let (collapsed, saved) = {
            let shell = self.shell.read(cx);
            let active = shell.active_named_layout();
            let saved: Vec<(String, bool)> = shell
                .named_layouts()
                .iter()
                .map(|named| (named.name.clone(), Some(named.name.as_str()) == active))
                .collect();
            (shell.output_collapsed, saved)
        };
        let list = v_flex()
            .gap(px(6.))
            .when(saved.is_empty(), |this| {
                this.child(
                    text(11.5, 16.)
                        .text_color(tokens::text3())
                        .child("Nothing saved yet."),
                )
            })
            .children(saved.into_iter().map(|(name, active)| {
                let apply = name.clone();
                let delete = name.clone();
                h_flex()
                    .id(SharedString::from(format!("layout-{name}")))
                    .h(px(40.))
                    .px(px(12.))
                    .gap(px(10.))
                    .items_center()
                    .border_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .map(|this| {
                        if active {
                            this.border_color(tokens::accent_line())
                                .bg(tokens::accent_soft())
                        } else {
                            this.border_color(tokens::border())
                                .bg(tokens::dock())
                                .hover(|this| this.bg(tokens::hover()))
                        }
                    })
                    .on_click(self.set(move |shell, cx| shell.apply_named_layout(&apply, cx)))
                    .child(
                        div()
                            .text_color(tokens::text2())
                            .child(icon("panels-top-left", 14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name.clone()),
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
                    .child(
                        ghost_icon(
                            SharedString::from(format!("delete-layout-{name}")),
                            "trash",
                            "Delete",
                        )
                        // Destructive: 44, not the ghost icon's 24 (WCAG 2.5.5).
                        .size(tokens::primary_target())
                        .on_click({
                            let shell = self.shell.clone();
                            move |_, _, cx| {
                                cx.stop_propagation();
                                shell
                                    .update(cx, |shell, cx| shell.delete_named_layout(&delete, cx));
                            }
                        }),
                    )
            }));
        let docks = Section::new(
            "Docks",
            vec![
                Row::new(
                    "Dock layout",
                    secondary_button("reset-layout", "rotate-ccw", "Reset layout")
                        .h(tokens::primary_target())
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
        let field = self.layout_name.clone();
        let save = h_flex()
            .gap(px(8.))
            .items_center()
            .child(
                h_flex()
                    .w(px(180.))
                    .h(px(30.))
                    .px(px(10.))
                    .items_center()
                    .border_1()
                    .border_color(tokens::border2())
                    .rounded(px(6.))
                    .bg(tokens::dock())
                    .child(Input::new(&field).appearance(false)),
            )
            .child(
                secondary_button("save-layout", "plus", "Save current").on_click(
                    cx.listener(|this, _, window, cx| this.save_named_layout(window, cx)),
                ),
            );
        let named = Section::new(
            "Named layouts",
            vec![Row::new("Saved layouts", save)
                .describe(
                    "Save the current arrangement under a name, then click one to switch to it. Saving under a name you have already used replaces it.",
                )
                .below(list)],
        );
        vec![docks, named]
    }

    /// Saves the arrangement under the typed name and empties the field.
    pub(super) fn save_named_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.layout_name.read(cx).value().to_string();
        if name.trim().is_empty() {
            return;
        }
        self.shell
            .update(cx, |shell, cx| shell.save_named_layout(&name, cx));
        self.layout_name
            .update(cx, |state, cx| state.set_value("", window, cx));
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
                    .describe(
                        "Every clickable control grows to at least 44 px, from 24 px. \
                         Save, Delete, Play and confirm buttons are 44 px either way.",
                    )
                    .changed(large, |shell, cx| shell.toggle_large_targets(cx)),
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
