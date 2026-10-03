//! New template: a name (checked as it's typed), the class, and what the
//! file starts from — empty, the class's built-in starter, or a copy of
//! another template.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, mono, text, Weight};
use crate::script_templates::CLASSES;
use crate::tokens;

use super::dialogs::{card, card_foot, card_head, Dialog};
use super::kit::segment;
use super::{Selected, TemplatesWindow};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Start {
    Empty,
    Starter,
    Copy,
}

pub(super) struct NewDialog {
    name: Entity<InputState>,
    class: &'static str,
    pub(super) start: Start,
    /// The template "A copy of another template" copies.
    pub(super) copy_of: Option<(&'static str, String)>,
    pub(super) picker_open: bool,
    /// Why the typed name can't be used: shown once something is typed, or
    /// after Create was pressed.
    error: Option<String>,
    _subscription: Subscription,
}

/// The first of `class`'s templates, what "A copy of another template"
/// offers first.
fn first_of(
    templates: &crate::script_templates::ScriptTemplates,
    class: &str,
) -> Option<(&'static str, String)> {
    templates
        .extras()
        .iter()
        .find(|t| t.class == class)
        .map(|t| (t.class, t.name.clone()))
}

/// The built-in starter, shortened to its first and last lines.
fn preview(source: &str) -> String {
    let lines: Vec<_> = source.lines().filter(|l| !l.trim().is_empty()).collect();
    match lines.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first, .., last] => format!("{first} \u{2026} {last}"),
    }
}

impl TemplatesWindow {
    pub(super) fn open_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let class = super::actions::class_of(self.selected.as_ref());
        let name = cx.new(|cx| InputState::new(window, cx));
        name.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe_in(
            &name,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.check_new(false, cx),
                InputEvent::PressEnter { .. } => this.confirm_new(window, cx),
                _ => {}
            },
        );
        let templates = &self.shell.read(cx).script_templates;
        let copy_of = first_of(templates, class).or_else(|| {
            templates
                .extras()
                .first()
                .map(|t| (t.class, t.name.clone()))
        });
        self.dialog = Some(Dialog::New(NewDialog {
            name,
            class,
            start: Start::Starter,
            copy_of,
            picker_open: false,
            error: None,
            _subscription: subscription,
        }));
        cx.notify();
    }

    pub(super) fn new_dialog(&mut self) -> Option<&mut NewDialog> {
        match &mut self.dialog {
            Some(Dialog::New(dialog)) => Some(dialog),
            _ => None,
        }
    }

    /// `pressed`: Create was pressed, so an empty name is an error too.
    fn check_new(&mut self, pressed: bool, cx: &mut Context<Self>) {
        let Some(Dialog::New(dialog)) = &self.dialog else {
            return;
        };
        let typed = dialog.name.read(cx).value().to_string();
        let error = (pressed || !typed.trim().is_empty())
            .then(|| {
                self.shell
                    .read(cx)
                    .script_templates
                    .check_name(dialog.class, &typed, None)
                    .err()
            })
            .flatten()
            .map(|err| err.to_string());
        if let Some(dialog) = self.new_dialog() {
            dialog.error = error;
        }
        cx.notify();
    }

    fn confirm_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.check_new(true, cx);
        let Some(Dialog::New(dialog)) = &self.dialog else {
            return;
        };
        if dialog.error.is_some() {
            return;
        }
        let (class, typed) = (dialog.class, dialog.name.read(cx).value().to_string());
        let source = match dialog.start {
            Start::Empty => Some(String::new()),
            Start::Starter => {
                let shell = self.shell.read(cx);
                super::super::keys::default_template(&shell.database, class).map(str::to_owned)
            }
            Start::Copy => dialog
                .copy_of
                .clone()
                .and_then(|(class, name)| self.source_of(&Selected::Template { class, name }, cx)),
        }
        .unwrap_or_default();
        match self.create(class, &typed, &source, window, cx) {
            Ok(()) => self.dialog = None,
            Err(err) => {
                if let Some(dialog) = self.new_dialog() {
                    dialog.error = Some(err.to_string());
                }
            }
        }
        cx.notify();
    }

    pub(super) fn new_card(&self, dialog: &NewDialog, cx: &mut Context<Self>) -> impl IntoElement {
        let class = dialog.class;
        let typed = dialog.name.read(cx).value().to_string();
        let shown = if typed.trim().is_empty() {
            "Name"
        } else {
            typed.trim()
        };
        let starter = {
            let shell = self.shell.read(cx);
            super::super::keys::default_template(&shell.database, class).unwrap_or_default()
        };
        let options = [
            (
                Start::Empty,
                "Empty".to_owned(),
                "A blank file".to_owned(),
                false,
            ),
            (
                Start::Starter,
                format!("The built-in {class} starter"),
                preview(starter),
                true,
            ),
            (
                Start::Copy,
                "A copy of another template".to_owned(),
                "Pick one from the list".to_owned(),
                false,
            ),
        ];
        let field = h_flex()
            .h(px(32.))
            .gap(px(6.))
            .px(px(10.))
            .items_center()
            .border_1()
            .border_color(if dialog.error.is_some() {
                ui::red()
            } else {
                tokens::accent_line()
            })
            .rounded(px(6.))
            .bg(ui::panel())
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&dialog.name)
                        .appearance(false)
                        .px_0()
                        .text_size(px(12.5))
                        .line_height(px(17.))
                        .text_color(tokens::text()),
                ),
            )
            .child(mono(11., 14.).text_color(tokens::text3()).child(".luau"));
        let hint = match &dialog.error {
            Some(error) => text(11.5, 16.).text_color(ui::red()).child(error.clone()),
            None => mono(11.5, 16.)
                .text_color(tokens::text3())
                .child(format!("{class}/{shown}.luau")),
        };
        card(480.)
            .child(card_head("New template", cx))
            .child(
                v_flex()
                    .gap(px(14.))
                    .pt(px(12.))
                    .px(px(18.))
                    .pb(px(16.))
                    .child(labelled(
                        "Name",
                        v_flex().gap(px(6.)).child(field).child(hint),
                    ))
                    .child(labelled("Class", self.new_class_picker(class, cx)))
                    .child(labelled(
                        "Start from",
                        v_flex().gap(px(6.)).children(options.into_iter().map(
                            |(start, title, sub, mono_sub)| {
                                self.start_option(dialog, start, title, sub, mono_sub, cx)
                            },
                        )),
                    )),
            )
            .child(
                card_foot()
                    .child(
                        text(11.5, 16.)
                            .text_color(tokens::text3())
                            .child("Opens in the editor, ready to type."),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::button("new-cancel", "Cancel", Weight::Secondary, true).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.dialog = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        ui::icon_button("new-create", "plus", "Create", Weight::Primary, true)
                            .px(px(12.))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_new(window, cx)),
                            ),
                    ),
            )
    }

    fn new_class_picker(&self, current: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap(px(2.))
            .p(px(2.))
            .border_1()
            .border_color(tokens::border())
            .rounded(px(6.))
            .bg(ui::panel())
            .children(CLASSES.into_iter().map(|class| {
                segment(
                    ElementId::Name(format!("new-class-{class}").into()),
                    class,
                    class == current,
                    26.,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    // The copy follows the class, unless one was picked there.
                    let first = first_of(&this.shell.read(cx).script_templates, class);
                    if let Some(dialog) = this.new_dialog() {
                        dialog.class = class;
                        if dialog.copy_of.as_ref().is_none_or(|(c, _)| *c != class) {
                            dialog.copy_of = first.or(dialog.copy_of.take());
                        }
                    }
                    this.check_new(false, cx);
                }))
            }))
    }

    fn start_option(
        &self,
        dialog: &NewDialog,
        start: Start,
        title: String,
        sub: String,
        mono_sub: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let on = dialog.start == start;
        let usable = start != Start::Copy || dialog.copy_of.is_some();
        h_flex()
            .id(("start", start as usize))
            .min_h(px(40.))
            .gap(px(10.))
            .py(px(6.))
            .px(px(12.))
            .items_center()
            .rounded(px(6.))
            .border_1()
            .map(|this| {
                if on {
                    this.border_color(tokens::accent_line())
                        .bg(tokens::accent_soft())
                } else {
                    this.border_color(tokens::border()).bg(ui::panel())
                }
            })
            .when(usable, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = this.new_dialog() {
                            dialog.start = start;
                        }
                        cx.notify();
                    }))
            })
            .child(radio(on))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        text(12.5, 17.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if usable {
                                tokens::text()
                            } else {
                                tokens::text3()
                            })
                            .child(title),
                    )
                    .child(
                        text(11.5, 15.)
                            .when(mono_sub, |this| this.font_family(tokens::FONT_FAMILY_MONO))
                            .text_color(tokens::text3())
                            .truncate()
                            .child(sub),
                    ),
            )
            .when(start == Start::Copy, |this| {
                this.child(self.copy_picker(dialog, cx))
            })
    }
}

fn labelled(label: &'static str, body: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap(px(6.))
        .child(
            text(11.5, 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens::text2())
                .child(label),
        )
        .child(body)
}

/// A 14 px radio: a 1.5 px border2 ring, or a 4 px accent ring on `bg`.
fn radio(on: bool) -> impl IntoElement {
    div().flex_none().size(px(14.)).rounded_full().map(|this| {
        if on {
            this.border(px(4.)).border_color(ui::accent()).bg(ui::bg())
        } else {
            this.border(px(1.5)).border_color(tokens::border2())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::preview;

    #[test]
    fn a_starter_previews_as_its_first_and_last_lines() {
        assert_eq!(
            preview("local module = {}\n\nreturn module\n"),
            "local module = {} \u{2026} return module"
        );
        assert_eq!(
            preview("print(\"Hello, world!\")\n"),
            "print(\"Hello, world!\")"
        );
    }
}
