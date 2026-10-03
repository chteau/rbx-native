//! The pane for a file the loader refused: what it is, why it isn't used,
//! and the three things to do about it. It never opens in the editor — the
//! reason it was refused is the reason it can't be shown there.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono, Weight};
use crate::script_templates::{SkipReason, Skipped};
use crate::tokens;

use super::dialogs::Dialog;
use super::status::{self, Part, Tone};
use super::{Selected, TemplatesWindow};

/// Why the file isn't a template, after the bold "Not used as a template."
fn reason(skipped: &Skipped) -> String {
    match skipped.reason {
        SkipReason::TooLarge => format!(
            " It is {}; a template can be at most 256 KiB, so a stray generated file \
             never lands in a script\u{2019}s Source.",
            super::status::size_label(skipped.len)
        ),
        SkipReason::NotUtf8 => " It isn\u{2019}t UTF-8 text, so it can\u{2019}t be read as \
             Luau source. Re-save it as UTF-8 to use it."
            .to_owned(),
        SkipReason::Unreadable => " RbxNative couldn\u{2019}t read it. Check that the file \
             is yours and readable."
            .to_owned(),
    }
}

impl TemplatesWindow {
    pub(super) fn skipped_pane(
        &self,
        skipped: &Skipped,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = self
            .shell
            .read(cx)
            .script_templates
            .dir()
            .map(|dir| dir.join(skipped.class).join(&skipped.file_name));
        let row = Selected::Skipped {
            class: skipped.class,
            file_name: skipped.file_name.clone(),
        };
        let facts = |label: &'static str, value: Div| {
            h_flex()
                .gap(px(8.))
                .child(div().w(px(56.)).text_color(tokens::text3()).child(label))
                .child(value)
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .bg(tokens::dock())
            .items_center()
            .justify_center()
            .p(px(24.))
            .child(
                v_flex()
                    .w(px(440.))
                    .gap(px(14.))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(tokens::warning())
                                    .child(icon("triangle-alert", 18.)),
                            )
                            .child(
                                mono(15., 22.)
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(tokens::text())
                                    .child(skipped.file_name.clone()),
                            ),
                    )
                    .child(status::banner(
                        "triangle-alert",
                        Tone::Warning,
                        vec![
                            Part::Lead("Not used as a template."),
                            Part::Plain(reason(skipped)),
                        ],
                        window,
                    ))
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(tokens::text2())
                            .child(facts("Class", div().child(skipped.class)))
                            .child(facts(
                                "Where",
                                mono(11.5, 17.)
                                    .child(format!("{}/{}", skipped.class, skipped.file_name)),
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .pt(px(4.))
                            .items_center()
                            .child({
                                let path = path.clone();
                                ui::icon_button(
                                    "show-in-folder",
                                    "folder",
                                    "Show in folder",
                                    Weight::Secondary,
                                    true,
                                )
                                .on_click(move |_, _, cx| {
                                    if let Some(path) = &path {
                                        cx.reveal_path(path);
                                    }
                                })
                            })
                            .child(
                                ui::icon_button(
                                    "open-elsewhere",
                                    "external-link",
                                    "Open in another editor",
                                    Weight::Secondary,
                                    true,
                                )
                                .on_click(move |_, _, cx| {
                                    if let Some(path) = &path {
                                        cx.open_with_system(path);
                                    }
                                }),
                            )
                            .child(div().flex_1())
                            .child(
                                ui::icon_button(
                                    "delete-file",
                                    "trash",
                                    "Delete file",
                                    Weight::Ghost,
                                    true,
                                )
                                .text_color(ui::red())
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.dialog = Some(Dialog::Delete(row.clone()));
                                        cx.notify();
                                    },
                                )),
                            ),
                    ),
            )
    }
}
