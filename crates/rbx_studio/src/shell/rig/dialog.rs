//! The Rig Builder dialog: what to build, as four segmented pickers. The
//! state and its rules are plain data so they are tested without a window.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
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
    /// Any player's public avatar, by UserId.
    Player,
}

/// A UserId as typed: digits only, and not zero.
pub(crate) fn parse_user_id(text: &str) -> Result<u64, String> {
    let text = text.trim();
    match text.parse::<u64>() {
        Ok(id) if id > 0 && text.bytes().all(|b| b.is_ascii_digit()) => Ok(id),
        _ => Err(format!(
            "\u{201c}{text}\u{201d} is not a UserId: type the player\u{2019}s number, such as 156"
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RigDialog {
    pub(crate) rig_type: RigType,
    pub(crate) shape: BodyShape,
    pub(crate) scale: BodyScale,
    pub(crate) character: Character,
    /// For My Avatar and Player: the type to build instead of the
    /// avatar's own. `None` keeps the avatar's type.
    pub(crate) avatar_type: Option<RigType>,
}

impl Default for RigDialog {
    fn default() -> Self {
        RigDialog {
            rig_type: RigType::R15,
            shape: BodyShape::Masculine,
            scale: BodyScale::Classic,
            character: Character::Mannequin,
            avatar_type: None,
        }
    }
}

impl RigDialog {
    /// R6 is one fixed body: Studio offers neither Rthro nor a second shape
    /// for it, so those options are off while it is picked.
    pub(crate) fn r6_only(&self) -> bool {
        self.rig_type == RigType::R6
    }

    /// "My Avatar" and "Player" bring their own type, shape and scale.
    pub(crate) fn body_fixed(&self) -> bool {
        self.character != Character::Mannequin
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
        if let Some(rig_type) = self.avatar_type.filter(|_| self.body_fixed()) {
            Some(match (rig_type, self.character) {
                (RigType::R6, Character::Player) => "Built as R6: the player\u{2019}s R15 body parts and scales are converted; no API key needed.",
                (RigType::R15, Character::Player) => "Built as R15: an R6 player gets the standard R15 body; no API key needed.",
                (RigType::R6, _) => "Built as R6: your R15 body parts and scales are converted.",
                (RigType::R15, _) => "Built as R15: an R6 avatar gets the standard R15 body.",
            })
        } else if self.character == Character::Player {
            Some("Type, shape and scale come from that player\u{2019}s public avatar. No API key needed.")
        } else if self.body_fixed() {
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
                                let before = dialog.character;
                                pick(dialog);
                                shell.rig_user_focus = before != Character::Player
                                    && dialog.character == Character::Player;
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

        let types = if d.body_fixed() {
            vec![
                (
                    "Avatar\u{2019}s type",
                    d.avatar_type.is_none(),
                    true,
                    (|d| d.avatar_type = None) as Pick,
                ),
                ("R6", d.avatar_type == Some(RigType::R6), true, |d| {
                    d.avatar_type = Some(RigType::R6)
                }),
                ("R15", d.avatar_type == Some(RigType::R15), true, |d| {
                    d.avatar_type = Some(RigType::R15)
                }),
            ]
        } else {
            vec![
                (
                    "R6",
                    d.rig_type == RigType::R6,
                    true,
                    (|d| d.set_rig_type(RigType::R6)) as Pick,
                ),
                ("R15", d.rig_type == RigType::R15, true, |d| {
                    d.set_rig_type(RigType::R15)
                }),
            ]
        };
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
            ("Player", d.character == Character::Player, true, |d| {
                d.character = Character::Player
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
            .when(d.character == Character::Player, |this| {
                this.child(self.rig_row(
                    "User Id",
                    h_flex().child(Input::new(&self.rig_user).small().w(px(220.))),
                ))
            })
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
        let mut d = RigDialog {
            shape: BodyShape::Feminine,
            scale: BodyScale::RthroSlender,
            ..Default::default()
        };
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
    fn a_user_id_is_digits_and_not_zero() {
        assert_eq!(parse_user_id(" 156 "), Ok(156));
        for bad in ["", "0", "abc", "-5", "1.5", "+7", "12 3"] {
            assert!(parse_user_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn player_fixes_the_body_like_my_avatar() {
        let d = RigDialog {
            character: Character::Player,
            ..RigDialog::default()
        };
        assert!(!d.scale_enabled(BodyScale::Classic));
        assert!(d.explanation().unwrap().contains("No API key"));
    }

    #[test]
    fn the_avatar_type_override_is_explained_in_both_directions() {
        for (character, rig_type, word) in [
            (Character::Player, RigType::R6, "Built as R6"),
            (Character::Player, RigType::R15, "Built as R15"),
            (Character::MyAvatar, RigType::R6, "Built as R6"),
            (Character::MyAvatar, RigType::R15, "Built as R15"),
        ] {
            let d = RigDialog {
                character,
                avatar_type: Some(rig_type),
                ..RigDialog::default()
            };
            assert!(d.explanation().unwrap().contains(word));
        }
        let own = RigDialog {
            character: Character::Player,
            ..RigDialog::default()
        };
        assert!(own.explanation().unwrap().contains("come from"));
    }

    #[test]
    fn a_mannequin_ignores_the_avatar_override() {
        let d = RigDialog {
            avatar_type: Some(RigType::R6),
            ..RigDialog::default()
        };
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
