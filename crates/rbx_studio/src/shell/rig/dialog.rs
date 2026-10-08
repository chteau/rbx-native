//! The Rig Builder dialog: what to build, as four segmented pickers. The
//! state and its rules are plain data so they are tested without a window.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use crate::shell::Shell;

use super::{BodyScale, BodyShape, RigType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Character {
    Mannequin,
    MyAvatar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RigDialog {
    pub(crate) rig_type: RigType,
    pub(crate) shape: BodyShape,
    pub(crate) scale: BodyScale,
    pub(crate) character: Character,
}

impl Default for RigDialog {
    fn default() -> Self {
        RigDialog {
            rig_type: RigType::R15,
            shape: BodyShape::Masculine,
            scale: BodyScale::Classic,
            character: Character::Mannequin,
        }
    }
}

impl RigDialog {
    /// R6 is one fixed body: Studio offers neither Rthro nor a second shape
    /// for it, so those options are off while it is picked.
    pub(crate) fn r6_only(&self) -> bool {
        self.rig_type == RigType::R6
    }

    /// "My Avatar" brings its own type, shape and scale.
    pub(crate) fn body_fixed(&self) -> bool {
        self.character == Character::MyAvatar
    }

    pub(crate) fn set_rig_type(&mut self, rig_type: RigType) {
        self.rig_type = rig_type;
        if self.r6_only() {
            self.shape = BodyShape::Masculine;
            self.scale = BodyScale::Classic;
        }
    }

    pub(crate) fn shape_enabled(&self, shape: BodyShape) -> bool {
        !self.body_fixed() && (shape == BodyShape::Masculine || !self.r6_only())
    }

    pub(crate) fn scale_enabled(&self, scale: BodyScale) -> bool {
        !self.body_fixed() && (scale == BodyScale::Classic || !self.r6_only())
    }

    /// Why some options are greyed out, if any are.
    pub(crate) fn explanation(&self) -> Option<&'static str> {
        if self.body_fixed() {
            Some("Type, shape and scale come from your Roblox avatar.")
        } else if self.r6_only() {
            Some("R6 has one body: Feminine and Rthro need R15.")
        } else {
            None
        }
    }
}

type Pick = fn(&mut RigDialog);

impl Shell {
    pub(crate) fn open_rig_dialog(&mut self, cx: &mut Context<Self>) {
        self.rig_dialog = Some(RigDialog::default());
        cx.notify();
    }

    /// Escape's half of the dialog; returns whether it was open.
    pub(in crate::shell) fn cancel_rig_dialog(&mut self) -> bool {
        self.rig_dialog.take().is_some()
    }

    fn segmented(
        &self,
        row: &'static str,
        items: Vec<(&'static str, bool, bool, Pick)>,
        cx: &mut Context<Self>,
    ) -> Div {
        h_flex()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(tokens::border())
            .bg(tokens::dock())
            .children(items.into_iter().map(|(label, selected, enabled, pick)| {
                let segment = h_flex()
                    .id(SharedString::from(format!("rig-{row}-{label}")))
                    .h(px(ui::button_height(true)))
                    .px(px(12.))
                    .items_center()
                    .rounded(px(4.))
                    .text_size(px(12.))
                    .child(label);
                if selected {
                    segment
                        .bg(tokens::accent_soft())
                        .text_color(tokens::check_on())
                        .font_weight(FontWeight::SEMIBOLD)
                } else if enabled {
                    segment
                        .text_color(tokens::text2())
                        .cursor_pointer()
                        .hover(|this| this.bg(ui::wash()))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            if let Some(dialog) = shell.rig_dialog.as_mut() {
                                pick(dialog);
                            }
                            cx.notify();
                        }))
                } else {
                    segment.text_color(tokens::text3()).opacity(0.5)
                }
            }))
    }

    fn rig_row(&self, label: &'static str, control: Div) -> Div {
        v_flex()
            .gap(px(6.))
            .child(
                ui::text(11., 14.)
                    .text_color(tokens::text2())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(control)
    }

    pub(in crate::shell) fn rig_dialog_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let d = self.rig_dialog?;
        let types = vec![
            (
                "R6",
                d.rig_type == RigType::R6,
                true,
                (|d| d.set_rig_type(RigType::R6)) as Pick,
            ),
            ("R15", d.rig_type == RigType::R15, true, |d| {
                d.set_rig_type(RigType::R15)
            }),
        ];
        let shapes = [
            ("Masculine", BodyShape::Masculine),
            ("Feminine", BodyShape::Feminine),
        ]
        .map(|(label, shape)| {
            let pick: Pick = match shape {
                BodyShape::Masculine => |d| d.shape = BodyShape::Masculine,
                BodyShape::Feminine => |d| d.shape = BodyShape::Feminine,
            };
            (label, d.shape == shape, d.shape_enabled(shape), pick)
        });
        let scales = [
            ("Classic", BodyScale::Classic),
            ("Rthro Normal", BodyScale::RthroNormal),
            ("Rthro Slender", BodyScale::RthroSlender),
        ]
        .map(|(label, scale)| {
            let pick: Pick = match scale {
                BodyScale::Classic => |d| d.scale = BodyScale::Classic,
                BodyScale::RthroNormal => |d| d.scale = BodyScale::RthroNormal,
                BodyScale::RthroSlender => |d| d.scale = BodyScale::RthroSlender,
            };
            (label, d.scale == scale, d.scale_enabled(scale), pick)
        });
        let characters = vec![
            (
                "Mannequin",
                d.character == Character::Mannequin,
                true,
                (|d| d.character = Character::Mannequin) as Pick,
            ),
            ("My Avatar", d.character == Character::MyAvatar, true, |d| {
                d.character = Character::MyAvatar
            }),
        ];
        let body = v_flex()
            .px(px(20.))
            .pt(px(14.))
            .pl(px(78.))
            .gap(px(14.))
            .child(self.rig_row("Rig Type", self.segmented("type", types, cx)))
            .child(self.rig_row("Body Shape", self.segmented("shape", shapes.into(), cx)))
            .child(self.rig_row("Body Scale", self.segmented("scale", scales.into(), cx)))
            .child(self.rig_row("Character", self.segmented("character", characters, cx)))
            .when_some(d.explanation(), |this, why| {
                this.child(ui::text(11.5, 16.).text_color(tokens::text2()).child(why))
            })
            .into_any_element();
        Some(
            div()
                .absolute()
                .inset_0()
                .track_focus(&self.rig_focus)
                .child(ui::dialog(
                    520.,
                    ui::dialog_glyph("users", ui::accent(), ui::wash()),
                    "Rig Builder",
                    "Insert a character with a Humanoid, ready for animation.".to_string(),
                    Some(body),
                    vec![
                        ui::button("rig-cancel", "Cancel", Weight::Secondary, false)
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.rig_dialog = None;
                                cx.notify();
                            }))
                            .into_any_element(),
                        ui::button("rig-insert", "Insert", Weight::Primary, false)
                            .on_click(cx.listener(|shell, _, _, cx| shell.confirm_rig_dialog(cx)))
                            .into_any_element(),
                    ],
                ))
                .into_any_element(),
        )
    }
}

impl Shell {
    /// Avatar's Rig Builder tile.
    pub(in crate::shell) fn rig_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![crate::shell::ribbon::tile(
            &self.ribbon_nav,
            "ribbon-rig-builder",
            gpui_kit::assets::IconName::Users,
            "Rig Builder",
            cx,
        )
        .tooltip(|window, cx| {
            crate::shell::tooltip::text(
                "Rig Builder \u{2014} insert an R6 or R15 character",
                window,
                cx,
            )
        })
        .on_click(cx.listener(|shell, _, _, cx| {
            crate::shell::ribbon::RibbonCommand::RigBuilder.run(shell, cx);
        }))
        .into_any_element()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn r6_turns_off_rthro_and_feminine_and_resets_them() {
        let mut d = RigDialog::default();
        d.shape = BodyShape::Feminine;
        d.scale = BodyScale::RthroSlender;
        d.set_rig_type(RigType::R6);
        assert_eq!(
            (d.shape, d.scale),
            (BodyShape::Masculine, BodyScale::Classic)
        );
        assert!(!d.shape_enabled(BodyShape::Feminine));
        assert!(!d.scale_enabled(BodyScale::RthroNormal));
        assert!(d.scale_enabled(BodyScale::Classic));
        assert!(d.explanation().is_some());
    }

    #[test]
    fn r15_offers_everything() {
        let d = RigDialog::default();
        assert!(d.shape_enabled(BodyShape::Feminine));
        assert!(d.scale_enabled(BodyScale::RthroSlender));
        assert_eq!(d.explanation(), None);
    }

    #[test]
    fn my_avatar_fixes_the_body() {
        let d = RigDialog {
            character: Character::MyAvatar,
            ..RigDialog::default()
        };
        assert!(!d.shape_enabled(BodyShape::Masculine));
        assert!(!d.scale_enabled(BodyScale::Classic));
        assert!(d.explanation().is_some());
    }
}
