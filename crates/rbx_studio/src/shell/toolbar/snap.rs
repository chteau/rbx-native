//! The snapping half of the transform toolbar: one increment field per unit,
//! each with its own enable/disable checkbox beside it.
//!
//! Two pairs, not three. `creator-docs` (`parts/index.md#transform-parts`)
//! gives Move and Scale a single studs increment between them and Rotate its
//! own in degrees — "increments are based on **studs** for moving/scaling or
//! **degrees** for rotating, each adjustable in the toolbar" — and its
//! shortcuts say the same thing twice over: `Shift`+`2` jumps to "the
//! **move/scale** increment input", `Alt`+`R` to "the **rotate** increment
//! input".
//!
//! The rotate pair is drawn disabled, the same convention the Scale and Rotate
//! buttons already follow next to it: there is no Rotate tool for a degree
//! increment to apply to yet, and a live-looking control that does nothing
//! would say less than a visibly disabled one.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInputEvent, StepAction};
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::transform::{self, Action, Snap, SnapKind};

use super::super::{rows, Shell};

/// The live text of both increment fields.
///
/// Persistent entities rather than text rebuilt per render, for the same
/// reason the Properties panel's rows are (see `shell::edit`): an `InputState`
/// rebuilt under a keystroke loses the caret, and this one is typed into while
/// the viewport beside it is redrawing continuously.
pub(crate) struct SnapFields {
    translate: Entity<InputState>,
    rotate: Entity<InputState>,
}

impl SnapFields {
    /// Seeded from the toolbar state the editor starts with, so the fields and
    /// what a drag actually rounds to cannot disagree at startup.
    pub(crate) fn new(
        transform: crate::transform::Transform,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> (Self, [Subscription; 4]) {
        let fields = SnapFields {
            translate: field(transform.translate, window, cx),
            rotate: field(transform.rotate, window, cx),
        };
        let subscriptions = [
            watch(&fields.translate, SnapKind::Translate, cx),
            watch(&fields.rotate, SnapKind::Rotate, cx),
            watch_steps(&fields.translate, SnapKind::Translate, window, cx),
            watch_steps(&fields.rotate, SnapKind::Rotate, window, cx),
        ];
        (fields, subscriptions)
    }

    /// Rewrites a field whose text no longer says its increment, as after a
    /// change from Settings. A field being typed in is left alone: "1." on
    /// the way to "1.5" is not a disagreement to correct. Focus counts only
    /// while the window is active: a field left focused in a window behind
    /// another one is not being typed in.
    pub(crate) fn sync(
        &self,
        transform: crate::transform::Transform,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (input, increment) in [
            (&self.translate, transform.translate.increment),
            (&self.rotate, transform.rotate.increment),
        ] {
            let state = input.read(cx);
            if (window.is_window_active() && state.focus_handle(cx).is_focused(window))
                || transform::parse_increment(&state.value()) == Some(increment)
            {
                continue;
            }
            input.update(cx, |state, cx| {
                state.set_value(format!("{increment}"), window, cx)
            });
        }
    }

    fn of(&self, kind: SnapKind) -> &Entity<InputState> {
        match kind {
            SnapKind::Translate => &self.translate,
            SnapKind::Rotate => &self.rotate,
        }
    }

    /// Puts the caret in one field and selects what is already there, so the
    /// shortcut's next keystroke replaces the increment rather than appending
    /// to it — which is the only reason to jump to a two-character field.
    pub(crate) fn focus(&self, kind: SnapKind, window: &mut Window, cx: &mut App) {
        self.of(kind).update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
    }
}

fn field(snap: Snap, window: &mut Window, cx: &mut Context<Shell>) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(format!("{}", snap.increment)))
}

/// As typed, not on `Enter`: an increment is one number, and waiting for a
/// commit key would leave the field disagreeing with the drag beside it.
/// Text that isn't a number yet ("", "1.") simply leaves the increment alone
/// — see `transform::parse_increment`.
fn watch(input: &Entity<InputState>, kind: SnapKind, cx: &mut Context<Shell>) -> Subscription {
    cx.subscribe(input, move |shell, input, event: &InputEvent, cx| {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let text = input.read(cx).value().to_string();
        if let Some(increment) = transform::parse_increment(&text) {
            shell.transform_action(Action::SetIncrement(kind, increment), cx);
        }
    })
}

/// The spinner buttons §5.1 puts on a numeric field. One press is one
/// increment step: the fields hold a snap size, and doubling or halving it
/// is what a user reaching for the arrows actually wants — 1, 2, 4 studs,
/// not 1, 1.01, 1.02.
fn watch_steps(
    input: &Entity<InputState>,
    kind: SnapKind,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Subscription {
    // `subscribe_in` rather than `subscribe`: writing the stepped value back
    // into the field needs a `Window`, and a plain subscription has none.
    cx.subscribe_in(
        input,
        window,
        move |shell, input, event: &NumberInputEvent, window, cx| {
            let NumberInputEvent::Step(action) = event;
            let current = match kind {
                SnapKind::Translate => shell.transform.translate.increment,
                SnapKind::Rotate => shell.transform.rotate.increment,
            };
            let next = match action {
                StepAction::Increment => current * 2.,
                StepAction::Decrement => current / 2.,
            }
            .clamp(0.001, 360.);

            shell.transform_action(Action::SetIncrement(kind, next), cx);
            input.update(cx, |state, cx| {
                state.set_value(format!("{next}"), window, cx);
            });
        },
    )
}

/// The Move/Scale pill's copy: "1 stud" for exactly one, "N studs" for
/// anything else.
pub(super) fn studs(increment: f32) -> String {
    if increment == 1. {
        "1 stud".to_owned()
    } else {
        format!("{increment} studs")
    }
}

/// The section title, spaced for reading. `SnapKind::label` stays
/// "Move/Scale": it names the element IDs and the shortcut tooltips.
fn title(kind: SnapKind) -> &'static str {
    match kind {
        SnapKind::Translate => "Move / Scale",
        SnapKind::Rotate => "Rotate",
    }
}

impl Shell {
    /// The popover body, to `Snap-Popover` / `Snap-Popover-RotateOff`: 248
    /// wide with the hairline inside that width, one section per snap
    /// unit, a 1px divider with 10px above and below between them.
    pub(super) fn snap_fields_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        let [translate, rotate] = SnapKind::ALL;
        v_flex()
            .w(px(248.))
            .p(px(12.))
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border2())
            .rounded(tokens::radius_container())
            .shadow(vec![tokens::floating_shadow()])
            .child(self.snap_section(translate, cx))
            .child(div().h(px(1.)).my(px(10.)).bg(tokens::border()))
            .child(self.snap_section(rotate, cx))
            .into_any_element()
    }

    /// One section: a 28px row (title, unit, the switch pushed right), 6px,
    /// then the 28px stepper. Switched off, the title dims to `text2` and
    /// the stepper goes flat, `text3`, disabled and out of the Tab order.
    fn snap_section(&self, kind: SnapKind, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = match kind {
            SnapKind::Translate => self.transform.translate,
            SnapKind::Rotate => self.transform.rotate,
        };
        let enabled = snap.enabled;
        let handle = cx.entity();

        // The field takes its place in the window's own Tab order
        // (`shell::roving::TabOrder`) here, the same one-line fix
        // `Shell::quality_control` gets and for the same reason: `InputState`
        // is `Focusable`, and the handle it hands out is the one its own
        // `.focus` already uses, so recording that handle *is* the fix —
        // there is nothing to forward focus to once Tab lands on it.
        //
        // Registered while building the popover's body rather than once at
        // startup, because the popover's body is the only place these fields
        // exist on screen. A stop for a control that is not visible is worse
        // than no stop at all, and the order is rebuilt every frame anyway
        // (`TabOrder::restart`), so closing the popover takes them back out
        // on its own. A disabled stepper is not a stop at all.
        if enabled {
            self.tab_order
                .register(&self.snap_fields.of(kind).read(cx).focus_handle(cx));
        }

        v_flex()
            .w_full()
            .gap(px(6.))
            .child(
                h_flex()
                    .h(px(28.))
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(if enabled {
                                tokens::text()
                            } else {
                                tokens::text2()
                            })
                            .child(title(kind)),
                    )
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text2())
                            .child(kind.unit()),
                    )
                    .child(div().flex_1())
                    .child(rows::checkbox(
                        SharedString::from(format!("snap-on-{}", kind.label())),
                        enabled,
                        move |_, _, cx| {
                            handle.update(cx, |shell, cx| {
                                shell.transform_action(Action::ToggleSnap(kind), cx);
                            });
                        },
                    )),
            )
            .child(self.snap_stepper(kind, enabled))
    }

    /// The stepper: one 28px field, `panel` on a `border2` hairline, with a
    /// 30px − and + at either end split off by a `border` hairline and the
    /// value centred between them in mono. Built on the toolkit's base
    /// number input so the arrow keys and the step subscriptions
    /// (`watch_steps`) keep working exactly as before.
    fn snap_stepper(&self, kind: SnapKind, enabled: bool) -> impl IntoElement {
        let ink = if enabled {
            tokens::text2()
        } else {
            tokens::text3()
        };
        let step = move |button: gpui_kit::base::Button, minus: bool| {
            button
                .w(px(30.))
                .h_full()
                .flex_none()
                .text_color(ink)
                .border_color(tokens::border())
                .map(|this| {
                    if minus {
                        this.border_r_1().rounded_l(px(4.))
                    } else {
                        this.border_l_1().rounded_r(px(4.))
                    }
                })
                .when(enabled, |this| {
                    this.hover(|this| {
                        tokens::hover_fx(this)
                            .bg(tokens::hover())
                            .text_color(tokens::text())
                    })
                })
                .child(
                    Icon::new(if minus {
                        IconName::Minus
                    } else {
                        IconName::Plus
                    })
                    .size(px(12.)),
                )
        };

        gpui_kit::base::NumberInput::new(self.snap_fields.of(kind))
            .disabled(!enabled)
            .w_full()
            .h(px(28.))
            .rounded(tokens::radius())
            .border_1()
            .border_color(if enabled {
                tokens::border2()
            } else {
                tokens::border()
            })
            .when(enabled, |this| this.bg(tokens::dock()))
            .decrement_button(move |button| step(button, true))
            .increment_button(move |button| step(button, false))
            .input(
                Input::new(self.snap_fields.of(kind))
                    .appearance(false)
                    .h_full()
                    .disabled(!enabled)
                    .text_center()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_md())
                    .text_color(if enabled {
                        tokens::text()
                    } else {
                        tokens::text3()
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::studs;

    #[test]
    fn the_pill_says_stud_for_exactly_one_and_studs_otherwise() {
        assert_eq!(studs(1.), "1 stud");
        assert_eq!(studs(2.), "2 studs");
        assert_eq!(studs(0.5), "0.5 studs");
    }
}
