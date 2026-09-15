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
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, ActiveTheme, Disableable as _, Sizable as _};
use gpui_kit::*;

use crate::transform::{self, Action, Snap, SnapKind};

use super::super::Shell;

/// How wide an increment field is. Enough for "0.001" or "180" without the
/// strip's buttons being pushed off the end of a narrow window.
const FIELD_WIDTH: Pixels = px(52.);

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
    ) -> (Self, [Subscription; 2]) {
        let fields = SnapFields {
            translate: field(transform.translate, window, cx),
            rotate: field(transform.rotate, window, cx),
        };
        let subscriptions = [
            watch(&fields.translate, SnapKind::Translate, cx),
            watch(&fields.rotate, SnapKind::Rotate, cx),
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

impl Shell {
    /// The checkbox-and-field pairs, for the toolbar strip to sit after its
    /// tool buttons.
    pub(super) fn snap_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_2()
            .children(SnapKind::ALL.map(|kind| self.snap_control(kind, cx)))
    }

    fn snap_control(&self, kind: SnapKind, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = match kind {
            SnapKind::Translate => self.transform.translate,
            SnapKind::Rotate => self.transform.rotate,
        };
        // Everything about the rotate pair is live except what it would act
        // on; see this module's own header.
        let unimplemented = kind == SnapKind::Rotate;
        let handle = cx.entity();

        h_flex()
            .items_center()
            .gap_1()
            .child(
                Checkbox::new(SharedString::from(format!("snap-on-{}", kind.label())))
                    .label(kind.label())
                    .checked(snap.enabled)
                    .xsmall()
                    .disabled(unimplemented)
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.transform_action(Action::ToggleSnap(kind), cx);
                        });
                    }),
            )
            .child(
                div().w(FIELD_WIDTH).child(
                    Input::new(self.snap_fields.of(kind))
                        .xsmall()
                        .disabled(unimplemented),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(kind.unit()),
            )
    }
}
