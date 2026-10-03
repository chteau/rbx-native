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

use super::editor::{SaveState, TemplateEditor};
use super::list::{self, open_folder};
use super::status::{self, Part};
use super::{Selected, TemplatesWindow};

impl TemplatesWindow {
    pub(super) fn pane(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(editor) = &self.editor {
            return self.editor_pane(editor, window, cx).into_any_element();
        }
        let templates = &self.shell.read(cx).script_templates;
        let body = if self.selected.is_none() && templates.extras().is_empty() {
            let folder = templates.dir().map(|dir| dir.to_owned());
            Some(empty_state(folder).into_any_element())
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
                            this.child(list::tag(mine))
                        } else {
                            this.child(ghost_glyph("rename", "pencil", "Rename"))
                        }
                    }),
            )
            .child(h_flex().flex_none().gap(px(6.)).items_center().map(|this| {
                if starter {
                    this.child(ui::icon_button(
                        "duplicate-new",
                        "copy",
                        "Duplicate as new",
                        Weight::Secondary,
                        true,
                    ))
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
                    this.child(ui::icon_button(
                        "duplicate",
                        "copy",
                        "Duplicate",
                        Weight::Secondary,
                        true,
                    ))
                    .child(
                        ui::icon_button("delete", "trash", "Delete", Weight::Ghost, true)
                            .text_color(ui::red()),
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
                    this.child(list::class_tag(class))
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

        let note = if editor.save == SaveState::TooLarge {
            Some(status::banner(
                "circle-alert",
                true,
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
                false,
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
                false,
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
                let selected = class == current;
                h_flex()
                    .id(ElementId::Name(format!("class-{class}").into()))
                    .h(px(24.))
                    .px(px(11.))
                    .items_center()
                    .rounded(px(4.))
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .map(|this| {
                        if selected {
                            this.bg(tokens::accent_soft())
                                .text_color(tokens::text())
                                .font_weight(FontWeight::SEMIBOLD)
                        } else {
                            this.text_color(tokens::text2())
                                .cursor_pointer()
                                .hover(|this| this.bg(ui::wash()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.move_to_class(class, window, cx)
                                }))
                        }
                    })
                    .child(class)
            }))
    }
}

/// A 24 px ghost button holding one glyph, in text3.
fn ghost_glyph(id: &'static str, glyph: &'static str, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .size(px(24.))
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(tokens::text3())
        .cursor_pointer()
        .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
        .child(icon(glyph, 13.))
        .tooltip(move |window, cx| super::super::tooltip::text(label, window, cx))
}

fn empty_state(folder: Option<std::path::PathBuf>) -> impl IntoElement {
    v_flex()
        .w(px(420.))
        .items_center()
        .gap(px(14.))
        .text_center()
        .child(
            h_flex()
                .size(px(56.))
                .items_center()
                .justify_center()
                .border_1()
                .border_color(tokens::border2())
                .rounded(px(14.))
                .bg(ui::panel2())
                .text_color(tokens::text2())
                .child(icon("file-plus", 22.)),
        )
        .child(
            text(18., 24.)
                .font_weight(FontWeight::BOLD)
                .text_color(tokens::text())
                .child("No templates of your own yet"),
        )
        .child(text(12.5, 19.).text_color(tokens::text2()).child(
            "Every new script starts from a built-in starter. Add your own to get an \
             Enemy AI module, a tweened door or a camera shake in one click, from the \
             Model menu and the ribbon\u{2019}s Script tile.",
        ))
        .child(
            h_flex()
                .gap(px(8.))
                .pt(px(6.))
                .items_center()
                .child(
                    ui::icon_button("empty-new", "plus", "New template", Weight::Primary, true)
                        .px(px(12.)),
                )
                .child(ui::icon_button(
                    "empty-import",
                    "upload",
                    "Import .luau files\u{2026}",
                    Weight::Secondary,
                    true,
                )),
        )
        .child(
            h_flex()
                .id("empty-folder")
                .pt(px(4.))
                .gap(px(4.))
                .items_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::accent())
                .cursor_pointer()
                .child("Open templates folder")
                .child(icon("external-link", 11.))
                .on_click(move |_, _, cx| open_folder(folder.as_deref(), cx)),
        )
}
