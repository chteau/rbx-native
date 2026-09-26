//! Recent: the page, and the table Home shows the top of.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::*;
use crate::home::RecentPlace;
use crate::launcher::home_window::{file_name, HomeWindow};
use crate::launcher::ui::{self};
use crate::tokens;

impl HomeWindow {
    pub(super) fn recent_page(&mut self, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).value().to_lowercase();
        let rows: Vec<RecentPlace> = self
            .recent
            .iter()
            .filter(|place| query.is_empty() || recent_name(place).to_lowercase().contains(&query))
            .cloned()
            .collect();
        v_flex()
            .gap(px(20.))
            .child(self.page_head("Recent", grid, true, cx))
            .child(if rows.is_empty() {
                ui::text(12.5, 19.)
                    .text_color(tokens::text2())
                    .child(if self.recent.is_empty() {
                        "Places you open show up here."
                    } else {
                        "No recent place matches."
                    })
                    .into_any_element()
            } else {
                self.recent_table(&rows, grid, true, cx).into_any_element()
            })
            .child(ui::text(11.5, 16.).text_color(tokens::text3()).child(
                "RbxNative keeps your last 20 places. Files that were moved or deleted drop off the list.",
            ))
            .into_any_element()
    }

    pub(super) fn recent_table(
        &self,
        rows: &[RecentPlace],
        grid: &Grid,
        header: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fluid = grid.content - 2. - 28. - GAP * 3. - 200. - 140.;
        let name_w = fluid * 1.3 / 2.9;
        let path_w = fluid * 1.6 / 2.9;
        let last = rows.len().saturating_sub(1);
        v_flex()
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .overflow_hidden()
            .when(header, |this| {
                this.child(
                    h_flex()
                        .h(px(32.))
                        .items_center()
                        .gap(px(GAP))
                        .px(px(14.))
                        .border_b_1()
                        .border_color(tokens::border())
                        .text_size(px(10.))
                        .line_height(px(14.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(tokens::text3())
                        .child(div().w(px(name_w)).child("NAME"))
                        .child(div().w(px(path_w)).child("LOCATION"))
                        .child(div().w(px(200.)).child("ROBLOX"))
                        .child(div().w(px(140.)).text_right().child("OPENED")),
                )
            })
            .children(rows.iter().enumerate().map(|(index, place)| {
                let roblox: AnyElement = match (&place.name, place.place_id) {
                    (Some(name), Some(_)) => {
                        ui::pill(name.clone(), Some("link"), true).into_any_element()
                    }
                    (None, Some(id)) => {
                        ui::pill(format!("Place {id}"), Some("link"), true).into_any_element()
                    }
                    _ => ui::text(11.5, 16.)
                        .text_color(tokens::text3())
                        .child("Local only")
                        .into_any_element(),
                };
                let open = place.clone();
                h_flex()
                    .id(SharedString::from(format!("recent-{index}")))
                    .h(px(40.))
                    .items_center()
                    .gap(px(GAP))
                    .px(px(14.))
                    .when(index != last, |this| {
                        this.border_b_1().border_color(tokens::border())
                    })
                    .cursor_pointer()
                    .hover(|this| tokens::hover_fx(this).bg(tokens::secondary_hover()))
                    .child(
                        h_flex()
                            .w(px(name_w))
                            .min_w_0()
                            .items_center()
                            .gap(px(10.))
                            .text_color(tokens::text2())
                            .child(ui::icon("file", 14.).flex_none())
                            .child(
                                ui::text(12.5, 17.)
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(tokens::text())
                                    .child(file_name(&place.path)),
                            ),
                    )
                    .child(
                        ui::mono(11., 15.)
                            .w(px(path_w))
                            .truncate()
                            .text_color(tokens::text3())
                            .child(display_dir(&place.path)),
                    )
                    .child(h_flex().w(px(200.)).min_w_0().child(roblox))
                    .child(
                        ui::text(11.5, 16.)
                            .w(px(140.))
                            .text_right()
                            .text_color(tokens::text2())
                            .child(place.opened.map(opened_label).unwrap_or_default()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_recent(&open, cx)))
            }))
    }
}
