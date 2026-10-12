//! The panel's Auto-rig section, shown while a mesh is being rigged: the
//! front's turn and flip (Avatar Setup's "Manually Align Front"), mirrored
//! joint edits, each joint's position, Publish and Done (first, so they
//! show without scrolling). Every edit cuts the
//! body parts again and rebuilds the rig in place; the skeleton view shows
//! the joint being edited.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_autorig::Joint;

use super::super::widgets::{
    keyed_button, label, picker, publish_button, section, wide_button, xyz_row,
};
use super::{number, resync, Shell};
use crate::shell::menu::{self, MenuId};

impl Shell {
    /// Writes one typed coordinate of the selected joint and re-cuts.
    pub(super) fn commit_autorig_axis(&mut self, axis: usize, text: &str, cx: &mut Context<Self>) {
        let Ok(value) = text.trim().parse::<f32>() else {
            return;
        };
        let Some(rig) = self.anim_autorig() else {
            return;
        };
        let joint = rig.joint;
        let mut at = rig.fit.joint(joint);
        if (at[axis] - value).abs() < 1e-4 {
            return;
        }
        at[axis] = value;
        self.autorig_move_joint(joint, at, cx);
    }

    pub(in crate::shell::animation) fn anim_autorig_section(
        &mut self,
        inputs: &[Entity<InputState>; 3],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rig = self.anim_autorig()?;
        let joint = rig.joint;
        let at = rig.fit.joint(joint);
        let (symmetric, busy, smooth) = (rig.symmetric, rig.busy, rig.smooth);
        let file = rig
            .source
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let warnings = rig.warnings.len();
        for (i, input) in inputs.iter().enumerate() {
            resync(input, &number(at[i]), window, cx);
        }
        let joints = Joint::ALL
            .into_iter()
            .map(|j| {
                menu::item(j.name())
                    .checked(j == joint)
                    .on_click(move |shell, cx| {
                        if let Some(rig) = shell.anim_autorig_mut() {
                            rig.joint = j;
                        }
                        shell.anim.editor.joint = Some(j.part().name().to_owned());
                        cx.notify();
                    })
            })
            .collect();
        let status = match (busy, warnings) {
            (true, _) => format!("{file} · cutting\u{2026}"),
            (false, 0) => file,
            (false, n) => format!("{file} · {n} warning(s) in Output"),
        };
        Some(
            section("Auto-rig")
                .child(label(status))
                // Done and Publish first: the panel is often no taller than
                // the stage, and these are what the section is for.
                .child(
                    h_flex()
                        .gap(px(6.))
                        .child(
                            wide_button("autorig-done", "Done", true)
                                .on_click(cx.listener(|shell, _, _, cx| shell.finish_autorig(cx))),
                        )
                        .child(
                            publish_button("autorig-publish", self.anim.publishing)
                                .on_click(cx.listener(|shell, _, _, cx| shell.publish_autorig(cx))),
                        ),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(
                            wide_button("autorig-turn", "Turn 90°", true)
                                .on_click(cx.listener(|shell, _, _, cx| shell.autorig_turn(1, cx))),
                        )
                        .child(
                            wide_button("autorig-flip", "Flip front", true)
                                .on_click(cx.listener(|shell, _, _, cx| shell.autorig_turn(2, cx))),
                        )
                        .child(
                            keyed_button(
                                "autorig-smooth".into(),
                                if smooth { "Smooth bends" } else { "Rigid parts" },
                                true,
                            )
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text(
                                    "Smooth: each part is skinned to its neighbours' joints, as Roblox's own bodies are. Rigid: each part moves on its own.",
                                    window,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|shell, _, _, cx| {
                                if let Some(rig) = shell.anim_autorig_mut() {
                                    rig.smooth = !rig.smooth;
                                }
                                shell.recut_autorig(cx);
                            })),
                        )
                        .child(
                            keyed_button(
                                "autorig-mirror".into(),
                                if symmetric {
                                    "Mirrored"
                                } else {
                                    "Not mirrored"
                                },
                                true,
                            )
                            .on_click(cx.listener(
                                |shell, _, _, cx| {
                                    if let Some(rig) = shell.anim_autorig_mut() {
                                        rig.symmetric = !rig.symmetric;
                                    }
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .child(h_flex().gap(px(6.)).child(picker(
                    self,
                    MenuId::AnimAutorigJoint,
                    "autorig-joint",
                    joint.name(),
                    joints,
                    cx,
                )))
                .child(xyz_row("Joint position (studs)", inputs))
                .into_any_element(),
        )
    }
}
