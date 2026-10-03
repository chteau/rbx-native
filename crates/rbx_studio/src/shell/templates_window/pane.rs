//! The right pane. With nothing selected and no templates of the user's
//! own, it explains what templates are for and offers the two ways to
//! make one.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::ui::{self, icon, text, Weight};
use crate::tokens;

use super::list::open_folder;
use super::TemplatesWindow;

impl TemplatesWindow {
    pub(super) fn pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    }
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
