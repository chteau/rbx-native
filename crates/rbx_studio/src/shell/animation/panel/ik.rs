//! The panel's IK section, Roblox's Manage IK. With IK on, dragging a part
//! with the Move tool poses its chain instead of only turning its own
//! joint. Body Part moves the limb the part belongs to; Full Body moves
//! everything from the part to the root. Pinned parts stay where they are
//! in either mode.

use gpui_kit::component::h_flex;
use gpui_kit::*;
use rbx_animation::ik::Mode;

use super::super::widgets::{keyed_button, label, section, wide_button};
use super::Shell;

impl Shell {
    pub(in crate::shell::animation) fn anim_ik_section(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ik = &self.anim.ik;
        let (enabled, mode) = (ik.enabled, ik.mode);
        let selected = self.selected_all().first().copied();
        let pinned_selected = selected.is_some_and(|s| ik.pinned.contains(&s));
        let names: Vec<String> = self
            .anim
            .bound
            .as_ref()
            .map(|b| {
                ik.pinned
                    .iter()
                    .map(|p| b.rig.part_name(*p).to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let pinned = if names.is_empty() {
            "Nothing pinned".to_owned()
        } else {
            format!("Pinned: {}", names.join(", "))
        };
        section("IK")
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        keyed_button(
                            "anim-ik-toggle".into(),
                            if enabled { "IK on" } else { "IK off" },
                            true,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.anim.ik.enabled = !shell.anim.ik.enabled;
                            cx.notify();
                        })),
                    )
                    .child(
                        keyed_button(
                            "anim-ik-mode".into(),
                            match mode {
                                Mode::BodyPart => "Body Part",
                                Mode::FullBody => "Full Body",
                            },
                            enabled,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.anim.ik.mode = match shell.anim.ik.mode {
                                Mode::BodyPart => Mode::FullBody,
                                Mode::FullBody => Mode::BodyPart,
                            };
                            cx.notify();
                        })),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        keyed_button(
                            "anim-ik-pin".into(),
                            if pinned_selected {
                                "Unpin selected"
                            } else {
                                "Pin selected"
                            },
                            selected.is_some(),
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            if let Some(&part) = shell.selected_all().first() {
                                if !shell.anim.ik.pinned.remove(&part) {
                                    shell.anim.ik.pinned.insert(part);
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        wide_button("anim-ik-unpin-all", "Unpin all", !names.is_empty()).on_click(
                            cx.listener(|shell, _, _, cx| {
                                shell.anim.ik.pinned.clear();
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(label(pinned))
            .child(label(if enabled {
                "Drag a part with the Move tool to pose its chain".into()
            } else {
                "Turn a part with the Rotate tool to pose its joint".into()
            }))
    }
}
