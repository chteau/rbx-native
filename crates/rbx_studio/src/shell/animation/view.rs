//! The document's layout, Blender's Animation workspace: the sub-tab strip
//! on top, the 3D view with the side panel to its right, the dope sheet (or
//! the Curve Editor, or the graph canvas) under them, and the playback bar along the bottom.

use gpui_kit::assets::IconName;
use gpui_kit::component::{self, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{split, Gesture, Mode, Shell};
use crate::shell::chrome;
use crate::shell::menu::{self, MenuId};
use crate::tokens;

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
        self.sync_autorig_markers(cx);
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
        let toast = self.animation_toast(cx);

        // The room the seam divides is what the 3D view and the panel took
        // last frame; before the first layout the panel takes its default
        // share of a guess.
        let room = self.anim_split_room().unwrap_or(600.0);
        let height = match self.anim.sheet_height {
            Some(h) => split::sheet_height(h, room),
            None => split::default_sheet_height(room),
        };
        let report_view = self.anim.view_bounds.clone();
        let report_bottom = self.anim.bottom_bounds.clone();
        let seam = chrome::seam_handle(
            "anim-split",
            false,
            Some(tokens::dock()),
            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                let height = this
                    .anim
                    .bottom_bounds
                    .get()
                    .map_or(0.0, |b| f32::from(b.size.height));
                this.anim.gesture = Some(Gesture::Split {
                    origin_y: f32::from(e.position.y),
                    height,
                });
                cx.stop_propagation();
                cx.notify();
            }),
        );
        v_flex()
            .id("animation-editor")
            .size_full()
            .bg(tokens::dock())
            .child(strip)
            .child(
                h_flex()
                    .relative()
                    .flex_1()
                    .min_h(px(40.))
                    .overflow_hidden()
                    .child(
                        canvas(move |b, _, _| report_view.set(Some(b)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.viewport())
                            .children(toast),
                    )
                    .child(side),
            )
            .child(seam)
            .child(
                div()
                    .relative()
                    .h(px(height))
                    .flex_none()
                    .border_t(px(1.))
                    .border_color(tokens::border())
                    .overflow_hidden()
                    .child(
                        canvas(move |b, _, _| report_bottom.set(Some(b)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(bottom),
            )
            .child(transport)
            .into_any_element()
    }

    /// The height the 3D view and the bottom panel share, as last laid out.
    pub(super) fn anim_split_room(&self) -> Option<f32> {
        let view = self.anim.view_bounds.get()?;
        let bottom = self.anim.bottom_bounds.get()?;
        Some(f32::from(view.size.height) + f32::from(bottom.size.height))
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
        let file = self.anim_file_group(cx);
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
                    "anim-skeleton",
                    IconName::Bone,
                    if self.anim.skeleton {
                        "Hide the skeleton"
                    } else {
                        "Show the skeleton"
                    },
                    true,
                )
                .when(self.anim.skeleton, |this| this.bg(tokens::accent_soft()))
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.anim.skeleton = !shell.anim.skeleton;
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
            .child(file)
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

impl Shell {
    /// Import ▾, Export ▾ and Publish: where a clip comes from and goes, in
    /// the editor's own top bar beside the view toggles, so none of it hides
    /// at the bottom of the side panel. Publish is the one filled button.
    fn anim_file_group(&self, cx: &mut Context<Self>) -> AnyElement {
        let saved = crate::anim_editor::place::saved_animations(
            &self.dom,
            self.anim.bound.as_ref().map(|b| b.model),
        );
        let mut imports = vec![
            menu::item("From asset ID\u{2026}")
                .icon(IconName::Download)
                .hint("Download a published animation by its AnimationId and open it here, unsaved")
                .on_click(|shell, cx| shell.request_animation_id_prompt(cx)),
            menu::item("From file\u{2026}")
                .icon(IconName::FileUp)
                .hint("Open an FBX, glTF or .rbxm animation; FBX and glTF are fitted onto the selected rig")
                .on_click(|shell, cx| shell.choose_animation_file(cx)),
            menu::separator(),
        ];
        if saved.is_empty() {
            imports.push(
                menu::item("No saved animations for this rig")
                    .disabled()
                    .hint("Export \u{203a} Save to rig stores the clip in the place; it is listed here after"),
            );
        } else {
            imports.extend(saved.into_iter().map(|(r, n, class)| {
                menu::item(n)
                    .hint(format!("Open this {class} saved in the place"))
                    .on_click(move |shell, cx| {
                        shell.load_animation(r);
                        cx.notify();
                    })
            }));
        }
        let exports = vec![
            menu::item(".rbxm file\u{2026}")
                .icon(IconName::FileDown)
                .hint("Write the clip as a KeyframeSequence (or CurveAnimation from the Curve Editor) model file, like Studio's Save to File")
                .on_click(|shell, cx| shell.export_animation_rbxm(cx)),
            menu::item("Save to rig")
                .hint("Store the clip in the place beside the rig's other saves \u{2014} a CurveAnimation while the Curve Editor has converted it, a KeyframeSequence otherwise (undoable)")
                .on_click(|shell, cx| shell.save_animation(cx)),
        ];
        let trigger = |id: &'static str, text: &'static str, tip: &'static str| {
            chrome::Trigger::new(
                chrome::button(id, text, false)
                    .debug_selector(move || id.into())
                    .gap(px(4.))
                    .child(component::Icon::new(IconName::ChevronDown).size(px(10.))),
            )
            .tooltip(tip)
        };
        let busy = self.anim.publishing;
        h_flex()
            .gap(px(4.))
            .ml(px(6.))
            .pl(px(8.))
            .border_l_1()
            .border_color(tokens::border())
            .items_center()
            .child(menu::dropdown(
                self,
                MenuId::AnimImport,
                trigger(
                    "anim-import",
                    "Import",
                    "Bring a clip in: by asset ID, from a file, or one saved in the place",
                ),
                imports,
                cx,
            ))
            .child(menu::dropdown(
                self,
                MenuId::AnimExport,
                trigger(
                    "anim-export",
                    "Export",
                    "Write the clip out: to an .rbxm file, or into the place",
                ),
                exports,
                cx,
            ))
            .child(
                chrome::button(
                    "anim-publish",
                    if busy {
                        "Publishing\u{2026}"
                    } else {
                        "Publish"
                    },
                    true,
                )
                .debug_selector(|| "anim-publish".into())
                .tooltip(|window, cx| {
                    crate::shell::tooltip::text(
                        "Upload the clip to Roblox as an Animation asset and get its AnimationId",
                        window,
                        cx,
                    )
                })
                .when(busy, |this| this.opacity(0.5).cursor_not_allowed())
                .when(!busy, |this| {
                    this.on_click(
                        cx.listener(|shell, _, window, cx| {
                            shell.open_animation_publish(window, cx)
                        }),
                    )
                }),
            )
            .into_any_element()
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
