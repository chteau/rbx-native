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

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{
    InputEvent, InputState, NumberInput, NumberInputEvent, StepAction,
};
use gpui_kit::component::{v_flex, Sizable as _};
use gpui_kit::*;

use crate::tokens;
use crate::transform::{self, Action, Snap, SnapKind};

use super::super::Shell;
use super::{field_label, snap_container};

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

impl Shell {
    /// §2.4's popover body: one labelled field per snap unit, stacked.
    ///
    /// The enable checkbox rides on the label rather than sitting beside
    /// the field, so the row reads as one thing ("Move/Scale snapping, at
    /// this increment") instead of two controls that happen to be adjacent.
    pub(super) fn snap_fields_popover(&self, cx: &mut Context<Self>) -> AnyElement {
        snap_container(
            SnapKind::ALL
                .map(|kind| self.snap_field(kind, cx).into_any_element())
                .into_iter()
                .collect(),
        )
    }

    fn snap_field(&self, kind: SnapKind, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = match kind {
            SnapKind::Translate => self.transform.translate,
            SnapKind::Rotate => self.transform.rotate,
        };
        let handle = cx.entity();

        v_flex()
            .w_full()
            .child(field_label(
                Checkbox::new(SharedString::from(format!("snap-on-{}", kind.label())))
                    .label(format!("{} ({})", kind.label(), kind.unit()))
                    .checked(snap.enabled)
                    .xsmall()
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.transform_action(Action::ToggleSnap(kind), cx);
                        });
                    }),
            ))
            .child(
                div()
                    .w_full()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .child(NumberInput::new(self.snap_fields.of(kind)).small()),
            )
    }
}
