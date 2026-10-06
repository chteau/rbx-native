//! The sheet's two popovers: the Workspace picker an insert starts from,
//! and the Fit menu.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::kit::mono;
use super::{Popover, Shell};
use crate::tokens;
use crate::viewport_frame::{self as frame, Insertable};

/// Rows shown before the list scrolls.
const VISIBLE_ROWS: usize = 13;
const ROW: f32 = 26.0;
/// One level of the tree, indented.
const INDENT: f32 = 14.0;
/// Where each popover sits in the sheet, from the boards: just under the
/// tool strip, its left edge by the button that opens it.
const TOP: f32 = 79.0;
const PICKER_LEFT: f32 = 9.0;
const PICKER_WIDTH: f32 = 320.0;
const FIT_WIDTH: f32 = 210.0;
/// The Fit button's left edge, less the board's 9px.
const FIT_LEFT: f32 = 171.0;

fn surface(width: f32) -> Div {
    v_flex()
        .absolute()
        .top(px(TOP))
        .w(px(width))
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(6.))
        // CSS `0 14px 40px`, at GPUI's blur scale.
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.6),
            offset: point(px(0.), px(14.)),
            blur_radius: px(20.),
            spread_radius: px(0.),
            inset: false,
        }])
        .overflow_hidden()
        .occlude()
}

impl Shell {
    pub(super) fn sheet_popover(
        &mut self,
        popover: Popover,
        target: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match popover {
            Popover::Picker => self.picker(target, window, cx),
            Popover::Fit => self.fit_menu(cx),
        }
    }

    fn picker(&mut self, target: Ref, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(query) = self.ui.sheet.as_ref().map(|sheet| sheet.query.clone()) else {
            return div().into_any_element();
        };
        let handle = query.focus_handle(cx);
        if !handle.is_focused(window) {
            window.focus(&handle, cx);
        }
        let text = query.read(cx).value().to_string();
        let filtering = !text.trim().is_empty();
        let all = frame::insertable(&self.dom, &self.database);
        let total = all.len();
        let rows: Vec<Insertable> = match filtering {
            true => all
                .into_iter()
                .filter(|row| frame::matched(&row.name, &text).is_some())
                .collect(),
            false => all,
        };
        let count = match filtering {
            true => format!("{} of {total}", rows.len()),
            false => total.to_string(),
        };
        let shown = rows.len().clamp(1, VISIBLE_ROWS);
        let list: Vec<AnyElement> = match rows.is_empty() {
            true => vec![div()
                .h(px(ROW))
                .flex()
                .items_center()
                .justify_center()
                .text_size(tokens::text_md())
                .text_color(tokens::text3())
                .child("No match")
                .into_any_element()],
            false => rows
                .iter()
                .enumerate()
                .map(|(index, row)| self.picker_row(index, row, target, &text, cx))
                .collect(),
        };
        let footer = match filtering {
            true => div().child("Click to insert · filtered by name"),
            false => div()
                .child("Click to insert · Models and parts · ")
                .child(div().text_color(tokens::text2()).child("✓"))
                .child(" already inserted")
                .flex(),
        };

        surface(PICKER_WIDTH)
            .left(px(PICKER_LEFT))
            .child(
                div().flex_none().pt(px(8.)).px(px(8.)).pb(px(6.)).child(
                    h_flex()
                        .h(px(26.))
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .border_1()
                        .border_color(tokens::accent_line())
                        .rounded(px(5.))
                        .bg(tokens::field_select())
                        .child(
                            Icon::new(IconName::Search)
                                .size(px(13.))
                                .text_color(tokens::text2()),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&query)
                                    .appearance(false)
                                    .with_size(tokens::field_size())
                                    .px_0(),
                            ),
                        )
                        .child(mono(count, 10.5, tokens::text3())),
                ),
            )
            .child(
                div()
                    .id("frame-sheet-picker-list")
                    .h(px(shown as f32 * ROW + 6.0))
                    .px(px(4.))
                    .overflow_y_scroll()
                    .children(list),
            )
            .child(
                div()
                    .flex_none()
                    .py(px(7.))
                    .px(px(12.))
                    .border_t_1()
                    .border_color(tokens::border())
                    .text_size(tokens::text_badge())
                    .line_height(px(16.))
                    .text_color(tokens::text3())
                    .child(footer),
            )
            .into_any_element()
    }

    fn picker_row(
        &mut self,
        index: usize,
        row: &Insertable,
        target: Ref,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let inserted = frame::already_in(&self.dom, target, row);
        let filtering = !query.trim().is_empty();
        let indent = match filtering {
            true => 0.0,
            false => row.depth as f32 * INDENT,
        };
        let name_color = match inserted {
            true => tokens::text2(),
            false => tokens::text(),
        };
        // The matched run lit in the accent, the rest as it was.
        let name = match frame::matched(&row.name, query) {
            Some(range) if !range.is_empty() => h_flex()
                .child(row.name[..range.start].to_owned())
                .child(
                    div()
                        .text_color(tokens::check_on())
                        .font_weight(tokens::WEIGHT_BOLD)
                        .child(row.name[range.clone()].to_owned()),
                )
                .child(row.name[range.end..].to_owned()),
            _ => h_flex().child(row.name.clone()),
        };
        let source = row.referent;
        h_flex()
            .id(("frame-sheet-pick", index))
            .h(px(ROW))
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pl(px(8. + indent))
            .pr(px(8.))
            .rounded(px(4.))
            .when(!inserted, |this| {
                this.cursor_pointer()
                    .hover(|this| this.bg(tokens::hover()))
                    .on_click(
                        cx.listener(move |shell, _, _, cx| shell.insert_into_frame(source, cx)),
                    )
            })
            .child(
                Icon::new(match row.model {
                    true => IconName::Package,
                    false => IconName::Box,
                })
                .size(px(14.))
                .text_color(tokens::text2()),
            )
            .child(
                name.min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(tokens::text_md())
                    .line_height(px(16.))
                    .text_color(name_color),
            )
            .when(filtering && !row.path.is_empty(), |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(tokens::text_badge())
                        .line_height(px(16.))
                        .text_color(tokens::text3())
                        .child(row.path.join(" › ")),
                )
            })
            .child(div().flex_1())
            .child(match inserted {
                true => Icon::new(IconName::Check)
                    .size(px(14.))
                    .text_color(tokens::check_on())
                    .into_any_element(),
                false => mono(row.class.clone(), 10.5, tokens::text3()).into_any_element(),
            })
            .into_any_element()
    }

    fn fit_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let item = |id: &'static str, label: &'static str, key: &'static str| {
            h_flex()
                .id(id)
                .h(px(28.))
                .flex_none()
                .items_center()
                .gap(px(12.))
                .px(px(10.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(tokens::text_md())
                .text_color(tokens::text())
                .hover(|this| this.bg(tokens::hover()))
                .child(div().flex_1().child(label))
                .child(mono(key, 10.5, tokens::text3()))
        };
        let close = |shell: &mut Shell| {
            if let Some(sheet) = &mut shell.ui.sheet {
                sheet.popover = None;
            }
        };
        surface(FIT_WIDTH)
            .left(px(FIT_LEFT))
            .p(px(4.))
            .child(
                item("frame-sheet-fit-contents", "Fit contents", "F").on_click(cx.listener(
                    move |shell, _, _, cx| {
                        close(shell);
                        shell.fit_frame(cx);
                    },
                )),
            )
            .child(
                item("frame-sheet-fit-reset", "Reset camera", "Home").on_click(cx.listener(
                    move |shell, _, _, cx| {
                        close(shell);
                        shell.reset_frame_camera(cx);
                    },
                )),
            )
            .child(div().h(px(1.)).my(px(4.)).bg(tokens::border()))
            .child(
                item("frame-sheet-fit-top", "Top view", "T").on_click(cx.listener(
                    move |shell, _, _, cx| {
                        close(shell);
                        shell.top_frame_view(cx);
                    },
                )),
            )
            .into_any_element()
    }
}
