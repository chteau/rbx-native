//! The pane with nothing selected and no templates of the user's own: what
//! templates are for, and the ways to make one.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::ui::{self, icon, text, Weight};
use crate::tokens;

use super::kit::open_folder;
use super::TemplatesWindow;

pub(super) fn empty_state(
    folder: Option<std::path::PathBuf>,
    cx: &mut Context<TemplatesWindow>,
) -> impl IntoElement {
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
                        .px(px(12.))
                        .on_click(cx.listener(|this, _, window, cx| this.open_new(window, cx))),
                )
                .child(
                    ui::icon_button(
                        "empty-import",
                        "upload",
                        "Import .luau files\u{2026}",
                        Weight::Secondary,
                        true,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.import(window, cx))),
                ),
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
