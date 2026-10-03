//! Renaming in the list: the row's label becomes a field with a `.luau`
//! suffix (pencil, double-click or F2), checked against the name rules as
//! it's typed. Enter renames the file, Esc puts the label back. A clash
//! turns the field's border red and explains itself in a popover under the
//! row it clashes with, so that row stays in view.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono};
use crate::script_templates::NameError;
use crate::tokens;

use super::TemplatesWindow;

pub(super) struct Rename {
    pub(super) class: &'static str,
    pub(super) old: String,
    input: Entity<InputState>,
    /// Why the typed name can't be used, checked on every change.
    error: Option<String>,
    /// The existing template's name, when that's why.
    clash: Option<String>,
    _subscription: Subscription,
}

impl TemplatesWindow {
    pub(super) fn start_rename(
        &mut self,
        class: &'static str,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.check_rename(cx),
                InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                _ => {}
            },
        );
        self.rename = Some(Rename {
            class,
            old: name,
            input,
            error: None,
            clash: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn check_rename(&mut self, cx: &mut Context<Self>) {
        let Some(rename) = &self.rename else {
            return;
        };
        let typed = rename.input.read(cx).value().to_string();
        let error = self
            .shell
            .read(cx)
            .script_templates
            .check_name(rename.class, &typed, Some(&rename.old))
            .err();
        if let Some(rename) = &mut self.rename {
            rename.clash = match &error {
                Some(NameError::Taken { name, .. }) => Some(name.clone()),
                _ => None,
            };
            rename.error = error.map(|err| err.to_string());
        }
        cx.notify();
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = &self.rename else {
            return;
        };
        if rename.error.is_some() {
            return;
        }
        let (class, old) = (rename.class, rename.old.clone());
        let typed = rename.input.read(cx).value().to_string();
        if typed.trim() == old {
            self.cancel_rename(window, cx);
            return;
        }
        self.flush_save(cx);
        match self.rename_to(class, &old, &typed, window, cx) {
            Ok(()) => {
                self.rename = None;
                self.focus.focus(window, cx);
            }
            Err(err) => {
                if let Some(rename) = &mut self.rename {
                    rename.error = Some(err.to_string());
                }
            }
        }
        cx.notify();
    }

    pub(super) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// The row being renamed, if `class`/`name` is it: the glyph, then the
    /// field, on the selected wash.
    pub(super) fn rename_row(
        &self,
        class: &str,
        name: &str,
        glyph: &'static str,
    ) -> Option<AnyElement> {
        let rename = self
            .rename
            .as_ref()
            .filter(|r| r.class == class && r.old == name)?;
        let field = h_flex()
            .flex_1()
            .min_w_0()
            .h(px(26.))
            .gap(px(4.))
            .px(px(8.))
            .items_center()
            .border_1()
            .border_color(if rename.error.is_some() {
                ui::red()
            } else {
                tokens::accent_line()
            })
            .rounded(px(5.))
            .bg(ui::panel())
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&rename.input)
                        .appearance(false)
                        .px_0()
                        .text_size(px(12.5))
                        .line_height(px(17.))
                        .text_color(tokens::text()),
                ),
            )
            .child(mono(10.5, 14.).text_color(tokens::text3()).child(".luau"));
        Some(
            h_flex()
                .relative()
                .h(px(34.))
                .flex_none()
                .gap(px(9.))
                .pl(px(10.))
                .pr(px(6.))
                .items_center()
                .rounded(px(6.))
                .bg(tokens::accent_soft())
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens::text2())
                        .child(icon(glyph, 14.)),
                )
                .child(field)
                .when_some(rename.error.clone(), |this, error| {
                    // Under the row after this one: the row it clashes with
                    // is usually a neighbour, and stays readable.
                    // `deferred` paints it over the rows below; the
                    // absolute origin is where it starts.
                    this.child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(34. + 34. + 8.))
                            .child(deferred(
                                anchored().child(popover(error, rename.clash.clone())),
                            )),
                    )
                })
                .into_any_element(),
        )
    }
}

/// The 240 px conflict popover: a red-edged panel2 card with the message,
/// the clashing name in it in text colour.
fn popover(message: String, clash: Option<String>) -> impl IntoElement {
    let highlights: Vec<_> = clash
        .and_then(|name| message.find(&name).map(|at| at..at + name.len()))
        .map(|range| {
            (
                range,
                HighlightStyle {
                    color: Some(tokens::text().into()),
                    ..Default::default()
                },
            )
        })
        .into_iter()
        .collect();
    h_flex()
        .w(px(240.))
        .items_start()
        .gap(px(8.))
        .py(px(9.))
        .px(px(10.))
        .border_1()
        .border_color(Rgba {
            a: 0.35,
            ..ui::red()
        })
        .rounded(px(6.))
        .bg(ui::panel2())
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.5),
            offset: point(px(0.), px(8.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
            inset: false,
        }])
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(tokens::text2())
        .child(
            div()
                .flex_none()
                .pt(px(1.5))
                .text_color(ui::red())
                .child(icon("circle-alert", 13.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(StyledText::new(message).with_highlights(highlights)),
        )
}
