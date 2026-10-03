//! "A copy of another template": the 150 px button and the list it opens.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono};
use crate::tokens;

use super::list::class_glyph;
use super::new_dialog::{NewDialog, Start};
use super::TemplatesWindow;

impl TemplatesWindow {
    /// The 150 px button listing the user's templates to copy from.
    pub(super) fn copy_picker(
        &self,
        dialog: &NewDialog,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = dialog
            .copy_of
            .as_ref()
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "No templates yet".into());
        let entries: Vec<_> = self
            .shell
            .read(cx)
            .script_templates
            .extras()
            .iter()
            .map(|t| (t.class, t.name.clone()))
            .collect();
        h_flex()
            .id("copy-picker")
            .relative()
            .w(px(150.))
            .h(px(30.))
            .flex_none()
            .gap(px(8.))
            .px(px(10.))
            .items_center()
            .justify_between()
            .border_1()
            .border_color(tokens::border2())
            .rounded(px(6.))
            .bg(ui::panel())
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(tokens::text())
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                if let Some(dialog) = this.new_dialog() {
                    dialog.picker_open = !dialog.picker_open && dialog.copy_of.is_some();
                }
                cx.notify();
            }))
            .child(div().min_w_0().truncate().child(label))
            .child(
                div()
                    .flex_none()
                    .text_color(tokens::text2())
                    .child(icon("chevron-down", 12.)),
            )
            .when(dialog.picker_open, |this| {
                this.child(deferred(
                    anchored()
                        .position_mode(AnchoredPositionMode::Local)
                        .position(point(px(0.), px(32.)))
                        .child(
                            v_flex()
                                .id("copy-list")
                                .w(px(220.))
                                .max_h(px(240.))
                                .overflow_y_scroll()
                                .p(px(4.))
                                .border_1()
                                .border_color(tokens::border2())
                                .rounded(px(6.))
                                .bg(ui::panel2())
                                .shadow(tokens::elevation())
                                .occlude()
                                .children(entries.into_iter().map(|(class, name)| {
                                    let pick = (class, name.clone());
                                    h_flex()
                                        .id(ElementId::Name(format!("copy-{class}-{name}").into()))
                                        .h(px(26.))
                                        .gap(px(8.))
                                        .px(px(8.))
                                        .items_center()
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .hover(|this| this.bg(ui::wash()))
                                        .child(
                                            div()
                                                .text_color(tokens::text2())
                                                .child(icon(class_glyph(class), 12.)),
                                        )
                                        .child(div().flex_1().min_w_0().truncate().child(name))
                                        .child(
                                            mono(10.5, 14.)
                                                .text_color(tokens::text3())
                                                .child(class),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            if let Some(dialog) = this.new_dialog() {
                                                dialog.copy_of = Some(pick.clone());
                                                dialog.start = Start::Copy;
                                                dialog.picker_open = false;
                                            }
                                            cx.notify();
                                        }))
                                })),
                        ),
                ))
            })
    }
}
