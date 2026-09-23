//! The Home and Installed pages: a header row, then a grid of cards —
//! the featured packages, a search's results, or the place's installed
//! packages. The skeletons and the two centred states are in `states`.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::wally_client::Listing;

use super::super::wally_sync::{package_id, Installed, Remote};
use super::super::Shell;
use super::states::{skeleton, SKELETONS};
use super::{Layout, GRID_GAP};

impl Shell {
    /// Home: "Discover" over the featured cards, or a search's header
    /// over its results.
    pub(super) fn home_page(
        &mut self,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> (Div, AnyElement) {
        if !self.wally.query.is_empty() {
            return self.search_page(layout, cx);
        }
        let header = header("Discover", None);
        let content = match &self.wally.featured {
            Remote::Ready(listings) => {
                let cards: Vec<AnyElement> = listings
                    .iter()
                    .enumerate()
                    .map(|(index, listing)| {
                        let query = format!("{}/{}", listing.scope, listing.name);
                        listing_card(index, listing)
                            .cursor_pointer()
                            .tab_index(self.tab_order.next())
                            .hover(|this| {
                                this.border_color(tokens::border2())
                                    .bg(tokens::secondary_hover())
                            })
                            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.wally_search_for(query.clone(), window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect();
                grid(layout.columns, cards).into_any_element()
            }
            Remote::Failed => self.error_state(cx).into_any_element(),
            Remote::Idle | Remote::Loading => grid(
                layout.columns,
                SKELETONS.iter().map(|&bars| skeleton(bars)).collect(),
            )
            .into_any_element(),
        };
        (header, content)
    }

    /// Installed: "N packages" over the same grid, each card not
    /// clickable and with its realm badge over its description.
    pub(super) fn installed_page(
        &mut self,
        layout: Layout,
        installed: &[Installed],
        _cx: &mut Context<Self>,
    ) -> (Div, AnyElement) {
        let count = match installed.len() {
            1 => "1 package".to_owned(),
            n => format!("{n} packages"),
        };
        let cards: Vec<AnyElement> = installed
            .iter()
            .map(|package| {
                let description = self
                    .wally_listing(&package_id(&package.scope, &package.name))
                    .and_then(|listing| listing.description.clone());
                card_frame()
                    .child(title_row(
                        &package.scope,
                        &package.name,
                        &format!("v{}", package.version),
                    ))
                    .child(
                        v_flex()
                            .h(px(32.))
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .child(
                                h_flex().h(px(16.)).items_center().child(
                                    div()
                                        .flex_none()
                                        .px(px(5.))
                                        .border_1()
                                        .border_color(tokens::border2())
                                        .rounded(tokens::RADIUS_BADGE)
                                        .text_size(tokens::text_xxs())
                                        .line_height(tokens::line_xxs())
                                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                                        .text_color(tokens::text2())
                                        .child(package.realm.label()),
                                ),
                            )
                            .child(
                                h_flex()
                                    .h(px(16.))
                                    .min_w_0()
                                    .child(description_line(description)),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        (
            header("Installed", Some(count)),
            grid(layout.columns, cards).into_any_element(),
        )
    }

    /// A search: "N results for `query`" over a result card each, the
    /// error state when the registry couldn't be reached.
    fn search_page(&mut self, layout: Layout, cx: &mut Context<Self>) -> (Div, AnyElement) {
        let query = self.wally.query.clone();
        let (count, content): (String, AnyElement) = match &self.wally.results {
            Remote::Ready(results) => {
                let count = match results.len() {
                    1 => "1 result".to_owned(),
                    n => format!("{n} results"),
                };
                let results = results.clone();
                let cards: Vec<AnyElement> = results
                    .iter()
                    .enumerate()
                    .map(|(index, result)| self.result_card(index, result, layout, cx))
                    .collect();
                (
                    count,
                    v_flex()
                        .gap(px(GRID_GAP))
                        .children(cards)
                        .into_any_element(),
                )
            }
            Remote::Failed => (
                "0 results".to_owned(),
                self.error_state(cx).into_any_element(),
            ),
            Remote::Idle | Remote::Loading => ("Searching".to_owned(), div().into_any_element()),
        };
        let header = h_flex()
            .h(px(22.))
            .flex_none()
            .items_center()
            .gap(px(6.))
            .text_size(tokens::text_md())
            .line_height(tokens::line_md())
            .text_color(tokens::text2())
            .child(
                div()
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text())
                    .child(count),
            )
            .child("for")
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text())
                    .child(query),
            );
        (header, content)
    }
}

/// 22px: the page's title at 12/700 and, 12px after it, its count.
pub(super) fn header(title: &'static str, subtitle: Option<String>) -> Div {
    h_flex()
        .h(px(22.))
        .flex_none()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .font_weight(tokens::WEIGHT_BOLD)
                .text_color(tokens::text())
                .child(title),
        )
        .children(subtitle.map(|text| {
            div()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text2())
                .child(text)
        }))
}

/// `columns` equal-width cards per row, 8px apart both ways; the last
/// row keeps its card widths with empty slots.
pub(super) fn grid(columns: usize, cards: Vec<AnyElement>) -> Div {
    let columns = columns.max(1);
    let mut rows = v_flex().gap(px(GRID_GAP));
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let mut row = h_flex().gap(px(GRID_GAP)).items_start();
        for _ in 0..columns {
            row = row.child(div().flex_1().min_w_0().children(cards.next()));
        }
        rows = rows.child(row);
    }
    rows
}

/// A card's frame: `panel2` on a `border` hairline, radius 6, 10/12
/// padding, 4px between the title row and the body.
fn card_frame() -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .gap(px(4.))
        .px(px(12.))
        .py(px(10.))
        .rounded(tokens::RADIUS_TILE)
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
}

/// `scope/` in `text2`, the name in `text` 600 (truncated), then the
/// version in mono `text2` at the right.
pub(super) fn title_row(scope: &str, name: &str, version: &str) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(10.))
        .child(name_span(scope, name, tokens::text_md()))
        .child(
            div()
                .flex_none()
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text2())
                .child(version.to_owned()),
        )
}

/// `scope/` then the name, one line; the name takes the ellipsis.
pub(super) fn name_span(scope: &str, name: &str, size: Pixels) -> Div {
    h_flex()
        .flex_1()
        .min_w_0()
        .text_size(size)
        .line_height(tokens::line_md())
        .child(
            div()
                .flex_none()
                .text_color(tokens::text2())
                .child(format!("{scope}/")),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .text_color(tokens::text())
                .child(name.to_owned()),
        )
}

/// One line of description in `text2`, or "No description" in `text3`.
fn description_line(description: Option<String>) -> Div {
    match description {
        Some(text) => div()
            .min_w_0()
            .truncate()
            .text_color(tokens::text2())
            .child(text),
        None => div().text_color(tokens::text3()).child("No description"),
    }
}

/// A featured card: the title row, then up to two lines of description
/// in a 32px body. The caller makes it clickable.
fn listing_card(index: usize, listing: &Listing) -> Stateful<Div> {
    card_frame()
        .id(("wally-featured", index))
        .child(title_row(
            &listing.scope,
            &listing.name,
            &format!("v{}", listing.latest()),
        ))
        .child(
            div()
                .h(px(32.))
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .map(|this| match &listing.description {
                    Some(text) => this
                        .text_color(tokens::text2())
                        .line_clamp(2)
                        .child(text.clone()),
                    None => this.text_color(tokens::text3()).child("No description"),
                }),
        )
}
