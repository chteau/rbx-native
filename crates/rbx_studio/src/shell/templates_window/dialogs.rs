//! The two dialogs, drawn over the window on a 50% black veil: New
//! template (name, class, what it starts from) and the Delete
//! confirmation, which a skipped file's `Delete file` shares.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono, text, Weight};
use crate::tokens;

use super::list::class_glyph;
use super::new_dialog::NewDialog;
use super::{Selected, TemplatesWindow};

pub(super) enum Dialog {
    New(NewDialog),
    Delete(Selected),
}

impl TemplatesWindow {
    /// The veil and whichever dialog is open.
    pub(super) fn dialog_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (top, card) = match self.dialog.as_ref()? {
            Dialog::New(dialog) => (36., self.new_card(dialog, cx).into_any_element()),
            Dialog::Delete(row) => (156., self.delete_card(row, cx).into_any_element()),
        };
        Some(
            div()
                .id("dialog-veil")
                .absolute()
                .left_0()
                .right_0()
                .top(tokens::topbar_height())
                .bottom_0()
                .occlude()
                .bg(hsla(0., 0., 0., 0.5))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.notify();
                }))
                .child(h_flex().pt(px(top)).justify_center().child(card))
                .into_any_element(),
        )
    }

    fn delete_card(&self, row: &Selected, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, body, path, glyph) = match row {
            Selected::Template { class, name } => (
                format!("Delete \u{201c}{name}\u{201d}?"),
                "It leaves the Model menu and the ribbon\u{2019}s Script menu straight away. \
                 Scripts you already made from it, in any place, are not changed.",
                format!("{class}/{name}.luau"),
                class_glyph(class),
            ),
            Selected::Skipped { class, file_name } => (
                format!("Delete \u{201c}{file_name}\u{201d}?"),
                "RbxNative doesn\u{2019}t use this file as a template, so no menu changes.",
                format!("{class}/{file_name}"),
                "triangle-alert",
            ),
            Selected::Starter(_) => return div().into_any_element(),
        };
        let row = row.clone();
        card(440.)
            .child(card_head(title, cx))
            .child(
                v_flex()
                    .gap(px(14.))
                    .pt(px(12.))
                    .px(px(18.))
                    .pb(px(16.))
                    .child(text(12.5, 19.).text_color(tokens::text2()).child(body))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .py(px(8.))
                            .px(px(10.))
                            .items_center()
                            .border_1()
                            .border_color(tokens::border())
                            .rounded(px(6.))
                            .bg(ui::panel())
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(tokens::text3())
                                    .child(icon(glyph, 12.)),
                            )
                            .child(mono(11.5, 16.).text_color(tokens::text2()).child(path)),
                    )
                    .child(
                        text(12., 16.)
                            .text_color(tokens::text2())
                            .child("This can\u{2019}t be undone."),
                    ),
            )
            .child(
                card_foot()
                    .child(div().flex_1())
                    .child(
                        ui::button("delete-cancel", "Cancel", Weight::Secondary, true).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.dialog = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        ui::icon_button("delete-confirm", "trash", "Delete", Weight::Danger, true)
                            .px(px(12.))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dialog = None;
                                this.delete(&row, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

/// A dialog card: panel2, border2, radius 10, a deep shadow. Clicks inside
/// it don't reach the veil.
pub(super) fn card(width: f32) -> Stateful<Div> {
    v_flex()
        .id("dialog")
        .w(px(width))
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(10.))
        .bg(ui::panel2())
        // GPUI's blur reads about twice CSS's.
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.55),
            offset: point(px(0.), px(18.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
        .on_click(|_, _, cx| cx.stop_propagation())
}

pub(super) fn card_head(
    title: impl Into<SharedString>,
    cx: &mut Context<TemplatesWindow>,
) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap(px(12.))
        .pt(px(16.))
        .px(px(18.))
        .pb(px(4.))
        .child(
            text(15., 22.)
                .flex_1()
                .min_w_0()
                .font_weight(FontWeight::BOLD)
                .text_color(tokens::text())
                .child(title.into()),
        )
        .child(
            h_flex()
                .id("dialog-close")
                .flex_none()
                .size(px(24.))
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .text_color(tokens::text3())
                .cursor_pointer()
                .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
                .child(icon("x", 12.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.notify();
                })),
        )
}

pub(super) fn card_foot() -> Div {
    h_flex()
        .gap(px(8.))
        .py(px(12.))
        .px(px(18.))
        .items_center()
        .border_t_1()
        .border_color(tokens::border())
}
