//! The right pane: the selected template in the editor, with its header,
//! Class row, status line and "Shows up as" strip. With nothing selected and
//! no templates of the user's own, it explains what templates are for and
//! offers the two ways to make one.

use gpui_kit::component::input::Editor;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono, text, Weight};
use crate::script_templates::CLASSES;
use crate::tokens;

use super::actions::Notice;
use super::dialogs::Dialog;
use super::editor::{SaveState, TemplateEditor};
use super::empty::empty_state;
use super::kit::{self, ghost_glyph, segment};
use super::status::{self, Part, Tone};
use super::{Selected, TemplatesWindow};

impl TemplatesWindow {
    pub(super) fn pane(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(editor) = &self.editor {
            return self.editor_pane(editor, window, cx).into_any_element();
        }
        let templates = &self.shell.read(cx).script_templates;
        if let Some(Selected::Skipped { class, file_name }) = &self.selected {
            if let Some(skipped) = templates
                .skipped()
                .iter()
                .find(|s| s.class == *class && s.file_name == *file_name)
                .cloned()
            {
                return self.skipped_pane(&skipped, window, cx).into_any_element();
            }
        }
        let body = if self.selected.is_none() && templates.extras().is_empty() {
            let folder = templates.dir().map(|dir| dir.to_owned());
            Some(empty_state(folder, cx).into_any_element())
        } else {
            None
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .bg(tokens::dock())
            .items_center()
            .justify_center()
            .p(px(24.))
            .children(body)
            .into_any_element()
    }

    fn editor_pane(
        &self,
        editor: &TemplateEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let yours = |class: &str| {
            self.shell
                .read(cx)
                .script_templates
                .default_for(class)
                .is_some()
        };
        let len = editor.len(cx);
        let (title, class, starter): (SharedString, &'static str, bool) = match &editor.row {
            Selected::Starter(class) => ("Default starter".into(), class, true),
            Selected::Template { class, name } => (name.clone().into(), class, false),
            Selected::Skipped { class, .. } => ("".into(), class, false),
        };
        let mine = starter && yours(class);

        let header = h_flex()
            .flex_none()
            .gap(px(16.))
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        text(18., 24.)
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child(title.clone()),
                    )
                    .map(|this| {
                        if starter {
                            this.child(kit::tag(mine))
                        } else {
                            let name = title.to_string();
                            this.child(ghost_glyph("rename", "pencil", "Rename").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.start_rename(class, name.clone(), window, cx)
                                }),
                            ))
                        }
                    }),
            )
            .child(h_flex().flex_none().gap(px(6.)).items_center().map(|this| {
                if starter {
                    this.child(
                        ui::icon_button(
                            "duplicate-new",
                            "copy",
                            "Duplicate as new",
                            Weight::Secondary,
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.duplicate(window, cx))),
                    )
                    .when(mine, |this| {
                        this.child(
                            ui::icon_button(
                                "reset-starter",
                                "rotate-ccw",
                                "Reset to built-in",
                                Weight::Secondary,
                                true,
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.reset_starter(class, cx)),
                            ),
                        )
                    })
                } else {
                    let row = editor.row.clone();
                    this.child(
                        ui::icon_button("duplicate", "copy", "Duplicate", Weight::Secondary, true)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                            ),
                    )
                    .child(
                        ui::icon_button("delete", "trash", "Delete", Weight::Ghost, true)
                            .text_color(ui::red())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dialog = Some(Dialog::Delete(row.clone()));
                                cx.notify();
                            })),
                    )
                }
            }));

        let path = if !starter {
            format!("{class}/{title}.luau")
        } else if mine {
            format!("{class}/Default.luau")
        } else {
            format!("Built in \u{b7} saves as {class}/Default.luau")
        };
        let class_row = h_flex()
            .flex_none()
            .gap(px(10.))
            .items_center()
            .child(text(11.5, 16.).text_color(tokens::text2()).child("Class"))
            .map(|this| {
                if starter {
                    this.child(kit::class_tag(class))
                } else {
                    this.child(self.class_picker(class, cx))
                }
            })
            .child(div().flex_1())
            .child(
                mono(11., 16.)
                    .min_w_0()
                    .truncate()
                    .text_color(tokens::text3())
                    .child(path),
            );

        let notice = self.notice.as_ref().map(|notice| match notice {
            Notice::MoveFailed { to, reason } => status::banner(
                "circle-alert",
                Tone::Danger,
                vec![
                    Part::Lead(format!("Can\u{2019}t move to {to}.").leak()),
                    Part::Plain(format!(" {reason}")),
                ],
                window,
            ),
            Notice::Info(line) => status::banner(
                "info",
                Tone::Neutral,
                vec![Part::Plain(line.clone())],
                window,
            )
            .child(
                h_flex()
                    .id("notice-dismiss")
                    .flex_none()
                    .size(px(18.))
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .text_color(tokens::text3())
                    .cursor_pointer()
                    .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
                    .child(icon("x", 11.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    })),
            ),
        });
        let note = if editor.save == SaveState::TooLarge {
            Some(status::banner(
                "circle-alert",
                Tone::Danger,
                vec![
                    Part::Lead("Over the 256 KiB limit."),
                    Part::Plain(
                        " Your last saved version is still in use. Trim this one to save it, \
                         or keep the data in a ModuleScript instead."
                            .into(),
                    ),
                ],
                window,
            ))
        } else if starter && mine {
            Some(status::banner(
                "info",
                Tone::Neutral,
                vec![
                    Part::Lead("Your version of the starter."),
                    Part::Plain(format!(
                        " Every new {class} starts like this. \u{201c}Reset to built-in\u{201d} deletes "
                    )),
                    Part::File("Default.luau"),
                    Part::Plain(" and brings the built-in one back.".into()),
                ],
                window,
            ))
        } else if starter {
            Some(status::banner(
                "info",
                Tone::Neutral,
                vec![
                    Part::Lead("Built-in starter."),
                    Part::Plain(format!(
                        " Every new {class} starts like this. Edit it and RbxNative saves your version as "
                    )),
                    Part::File("Default.luau"),
                    Part::Plain(", which replaces the built-in one.".into()),
                ],
                window,
            ))
        } else {
            None
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .bg(tokens::dock())
            .gap(px(12.))
            .pt(px(20.))
            .px(px(24.))
            .pb(px(16.))
            .child(header)
            .child(class_row)
            .children(notice)
            .children(note)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .bg(tokens::black())
                    .border_1()
                    .border_color(tokens::border())
                    .rounded(px(8.))
                    // The editor pads its text by 10 px across and 6 down on
                    // its own; placed this way, the gutter starts at the card's
                    // edge and the first line 8 px under it, as designed.
                    .child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .bottom(px(2.))
                            .left(px(-10.))
                            .right_0()
                            .child(
                                Editor::new(&editor.state)
                                    .appearance(false)
                                    .text_size(px(12.5))
                                    .line_height(px(20.))
                                    .h_full()
                                    .w_full(),
                            ),
                    ),
            )
            .child(status::status_line(&editor.save, len))
            .when(!starter, |this| {
                this.child(status::shows_up_as(&title, class))
            })
    }

    /// The Class segmented control: picking another class moves the file
    /// into that class's folder, under that class's name rule.
    fn class_picker(&self, current: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_none()
            .gap(px(2.))
            .p(px(2.))
            .border_1()
            .border_color(tokens::border())
            .rounded(px(6.))
            .bg(ui::panel())
            .children(CLASSES.into_iter().map(|class| {
                let tab = segment(
                    ElementId::Name(format!("class-{class}").into()),
                    class,
                    class == current,
                    24.,
                );
                if class == current {
                    tab
                } else {
                    tab.on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.move_to_class(class, window, cx)
                        }),
                    )
                }
            }))
    }
}
