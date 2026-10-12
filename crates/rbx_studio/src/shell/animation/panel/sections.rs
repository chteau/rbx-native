//! The panel's lower sections: the selected joint's pose and keys' easing,
//! animation events, and the import buttons.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::{Direction, Frame, Style};

use super::super::widgets::{field_row, label, picker, section, wide_button, xyz_row};
use super::Shell;
use crate::shell::menu::{self, MenuId};

impl Shell {
    pub(in crate::shell::animation) fn anim_key_section(
        &mut self,
        joint: Option<String>,
        rotation: &[Entity<InputState>; 3],
        position: &[Entity<InputState>; 3],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let easing = self.anim.editor.easing();
        let any = !self.anim.editor.selected.is_empty();
        let styles = Style::PICKABLE
            .into_iter()
            .map(|s| {
                menu::item(s.label())
                    .checked(easing.is_some_and(|e| e.0 == s))
                    .on_click(move |shell, cx| {
                        shell.anim.editor.set_easing(Some(s), None);
                        cx.notify();
                    })
            })
            .collect();
        let directions = Direction::ALL
            .into_iter()
            .map(|d| {
                menu::item(d.label())
                    .checked(easing.is_some_and(|e| e.1 == d))
                    .on_click(move |shell, cx| {
                        shell.anim.editor.set_easing(None, Some(d));
                        cx.notify();
                    })
            })
            .collect();
        let style_label = easing.map_or(if any { "Mixed" } else { "Easing" }, |e| e.0.label());
        let direction_label = easing.map_or("Direction", |e| e.1.label());
        let has_joint = joint.is_some();
        let joint_label = joint.unwrap_or_else(|| "No joint selected".into());
        section("Keyframe")
            .child(label(format!(
                "Joint: {joint_label} · frame {}",
                self.anim.editor.frame()
            )))
            .child(xyz_row("Rotation °", rotation))
            .child(xyz_row("Position", position))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("anim-key-joint", "Key joint (I)", has_joint).on_click(
                            cx.listener(|shell, _, _, cx| {
                                let joint = shell.anim.editor.joint.clone();
                                if joint.is_some() {
                                    shell.key_joints(joint.as_deref(), cx);
                                }
                            }),
                        ),
                    )
                    .child(
                        wide_button("anim-key-all", "Key all", true)
                            .on_click(cx.listener(|shell, _, _, cx| shell.key_joints(None, cx))),
                    )
                    .child(
                        wide_button("anim-reset-joint", "Reset", has_joint).on_click(cx.listener(
                            |shell, _, _, cx| {
                                if let Some(j) = shell.anim.editor.joint.clone() {
                                    shell.anim.editor.key(&j, Frame::IDENTITY);
                                    cx.notify();
                                }
                            },
                        )),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .when(any, |this| {
                        this.child(picker(
                            self,
                            MenuId::AnimEasingStyle,
                            "anim-ease-style",
                            style_label,
                            styles,
                            cx,
                        ))
                        .child(picker(
                            self,
                            MenuId::AnimEasingDirection,
                            "anim-ease-direction",
                            direction_label,
                            directions,
                            cx,
                        ))
                    })
                    .when(!any, |this| {
                        this.child(label("Select keys to set their easing".into()))
                    }),
            )
    }

    pub(in crate::shell::animation) fn anim_events_section(
        &mut self,
        marker: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let time = self.anim.editor.snap(self.anim.editor.time);
        let here: Vec<String> = self
            .anim
            .editor
            .clip
            .keyframes
            .iter()
            .filter(|k| (k.time - time).abs() < 1e-4)
            .flat_map(|k| k.markers.iter().map(|m| m.name.clone()))
            .collect();
        let listed = if here.is_empty() {
            "No event at the playhead".to_owned()
        } else {
            here.join(", ")
        };
        section("Events")
            .child(label(listed))
            .child(field_row("Add", marker))
            .child(
                wide_button("anim-remove-events", "Remove events here", !here.is_empty()).on_click(
                    cx.listener(|shell, _, _, cx| {
                        shell.anim.editor.remove_markers_here();
                        cx.notify();
                    }),
                ),
            )
    }

    pub(in crate::shell::animation) fn anim_import_section(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Asset ID, file and saved-clip imports are in the top bar's
        // Import ▾; the Blender addon's clipboard round trip stays here.
        section("Blender").child(
            h_flex()
                .gap(px(6.))
                .child(
                    wide_button("anim-paste-blender", "Paste from Blender", true)
                        .on_click(cx.listener(|shell, _, _, cx| shell.paste_blender_animation(cx))),
                )
                .child(
                    wide_button("anim-copy-blender", "Copy for Blender", true)
                        .on_click(cx.listener(|shell, _, _, cx| shell.copy_blender_animation(cx))),
                ),
        )
    }
}
