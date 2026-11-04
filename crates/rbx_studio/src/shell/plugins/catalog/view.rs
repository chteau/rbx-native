//! The tab strip both tabs share and the Popular tab's body: the search
//! field over a scrolling list of 60px rows (icon, name with the "Runs in
//! rbx-native" badge, creator and votes, our blurb, the action).

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Load, Row, State, Tab};
use crate::launcher::ui::{self, Weight};
use crate::shell::Shell;
use crate::tokens;

/// The list's height: six rows and their gaps, so the dialog keeps one
/// size while results load.
const LIST_HEIGHT: f32 = 396.;
/// Both tabs' body: the field, the caption and the list with their gaps,
/// so switching tab doesn't resize the dialog.
pub(in crate::shell::plugins) const BODY_HEIGHT: f32 = 32. + 10. + 16. + 10. + LIST_HEIGHT;
pub(in crate::shell::plugins) const ROW_HEIGHT: f32 = 60.;

/// The card every row in both tabs sits in.
pub(in crate::shell::plugins) fn row_frame(id: impl Into<ElementId>) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(ROW_HEIGHT))
        .flex_none()
        .px(px(10.))
        .gap(px(12.))
        .items_center()
        .rounded(tokens::radius_tile())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
}
const ICON: f32 = 40.;

/// The search box both tabs put over their list, `hint` at its right.
pub(in crate::shell::plugins) fn search_field(
    query: &Entity<InputState>,
    hint: SharedString,
) -> Div {
    h_flex()
        .h(px(32.))
        .flex_none()
        .px(px(10.))
        .gap(px(8.))
        .items_center()
        .rounded(tokens::radius())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .text_color(tokens::text3())
        .child(ui::icon("search", 13.).flex_none())
        .child(
            div().flex_1().min_w_0().h_full().child(
                Input::new(query)
                    .appearance(false)
                    .h_full()
                    .px(px(0.))
                    .text_size(tokens::text_md())
                    .text_color(tokens::text()),
            ),
        )
        .child(
            div()
                .flex_none()
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .child(hint),
        )
}

/// `1.2k` past a thousand, as the store prints votes.
fn votes_label(votes: u64) -> String {
    if votes >= 1000 {
        let tenths = (votes + 50) / 100;
        if tenths.is_multiple_of(10) {
            format!("{}k", tenths / 10)
        } else {
            format!("{}.{}k", tenths / 10, tenths % 10)
        }
    } else {
        votes.to_string()
    }
}

fn badge() -> Div {
    h_flex()
        .h(px(18.))
        .flex_none()
        .items_center()
        .gap(px(4.))
        .px(px(6.))
        .rounded(tokens::radius_badge())
        .bg(tokens::diff_add_soft())
        .text_color(tokens::diff_add())
        .text_size(tokens::text_xxs())
        .line_height(tokens::line_xxs())
        .font_weight(tokens::WEIGHT_SEMIBOLD)
        .child(ui::icon("badge-check", 11.))
        .child("Runs in rbx-native")
}

fn icon_tile(row: &Row) -> Div {
    let tile = div()
        .size(px(ICON))
        .flex_none()
        .rounded(tokens::radius_tile())
        .overflow_hidden()
        .bg(tokens::tile());
    match &row.icon {
        Some(image) => tile.child(img(image.clone()).size(px(ICON))),
        None => tile
            .flex()
            .items_center()
            .justify_center()
            .text_color(tokens::text3())
            .child(ui::icon("plug-2", 18.)),
    }
}

/// A one-line centred message where the rows would be.
fn notice(text: String, error: bool) -> Div {
    h_flex()
        .h(px(LIST_HEIGHT))
        .items_center()
        .justify_center()
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(if error {
            tokens::text_error()
        } else {
            tokens::text2()
        })
        .child(text)
}

impl Shell {
    /// "Installed N" and "Popular", the wally dock's segmented control.
    pub(in crate::shell::plugins) fn catalog_tabs(
        &mut self,
        installed: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let current = self.plugins.catalog.tab;
        let segments = [
            (Tab::Installed, format!("Installed \u{b7} {installed}")),
            (Tab::Popular, "Popular plugins".to_owned()),
        ]
        .map(|(tab, label)| {
            let selected = tab == current;
            h_flex()
                .id(SharedString::from(format!("plugin-tab-{}", tab as u8)))
                .flex_1()
                .h(px(24.))
                .items_center()
                .justify_center()
                .rounded(tokens::radius_segment())
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .map(|this| {
                    if selected {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                    } else {
                        this.cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.catalog_show(tab, window, cx);
                            }))
                    }
                })
                .child(label)
        });
        h_flex()
            .h(px(28.))
            .p(px(2.))
            .gap(px(2.))
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .children(segments)
    }

    pub(in crate::shell::plugins) fn catalog_body(&mut self, cx: &mut Context<Self>) -> Div {
        let catalog = &self.plugins.catalog;
        let field = catalog
            .query
            .as_ref()
            .map(|query| search_field(query, "Enter to search".into()));
        let search = &catalog.search;
        // Always one line, so the list doesn't jump when a search starts.
        let (caption, warn) = match (&search.load, &catalog.listing) {
            (Load::Idle, Load::Failed(err)) => (
                format!("Couldn't reach the Creator Store ({err}); showing saved names."),
                true,
            ),
            (Load::Idle, _) => (
                format!(
                    "{} plugins checked to run in rbx-native, most voted first",
                    super::list::VERIFIED.len()
                ),
                false,
            ),
            (Load::Ready, _) => (
                match search.ids.len() {
                    1 => format!("1 free plugin for \u{201c}{}\u{201d}", search.text),
                    n => format!("{n} free plugins for \u{201c}{}\u{201d}", search.text),
                },
                false,
            ),
            (Load::Loading, _) => ("Searching the Creator Store".to_owned(), false),
            (Load::Failed(_), _) => ("Search failed".to_owned(), true),
        };
        let list = match &search.load {
            Load::Loading => h_flex()
                .h(px(LIST_HEIGHT))
                .items_center()
                .justify_center()
                .gap(px(8.))
                .text_color(tokens::text2())
                .text_size(tokens::text_sm())
                .child(ui::spinner("plugin-catalog-searching", 14.))
                .child(format!("Searching for \u{201c}{}\u{201d}", search.text)),
            Load::Failed(err) => notice(format!("Search failed: {err}"), true),
            Load::Ready if search.ids.is_empty() => notice(
                format!("No free plugins match \u{201c}{}\u{201d}.", search.text),
                false,
            ),
            _ => {
                let rows: Vec<AnyElement> = self
                    .catalog_rows()
                    .into_iter()
                    .map(|row| self.catalog_row(row, cx))
                    .collect();
                div().h(px(LIST_HEIGHT)).child(
                    v_flex()
                        .id("plugin-catalog-list")
                        .size_full()
                        .gap(px(6.))
                        .overflow_y_scroll()
                        .children(rows),
                )
            }
        };
        v_flex()
            .h(px(BODY_HEIGHT))
            .gap(px(10.))
            .children(field)
            .child(
                div()
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(if warn {
                        tokens::warning()
                    } else {
                        tokens::text2()
                    })
                    .child(caption),
            )
            .child(list)
    }

    fn catalog_row(&mut self, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let action: AnyElement = match row.state {
            State::Available => {
                ui::button(("plugin-install", id), "Install", Weight::Primary, true)
                    .on_click(cx.listener(move |shell, _, _, cx| shell.catalog_install(id, cx)))
                    .into_any_element()
            }
            State::Installing => h_flex()
                .h(px(ui::button_height(true)))
                .gap(px(6.))
                .px(px(10.))
                .items_center()
                .text_size(tokens::text_sm())
                .text_color(tokens::text2())
                .child(ui::spinner(("plugin-installing", id), 12.))
                .child("Installing")
                .into_any_element(),
            State::Installed | State::UpdateAvailable => {
                let uninstall =
                    ui::button(("plugin-uninstall", id), "Uninstall", Weight::Ghost, true)
                        .on_click(
                            cx.listener(move |shell, _, _, cx| shell.catalog_uninstall(id, cx)),
                        );
                let lead = if row.state == State::UpdateAvailable {
                    ui::icon_button(
                        ("plugin-update", id),
                        "download",
                        "Update",
                        Weight::Primary,
                        true,
                    )
                    .on_click(cx.listener(move |shell, _, _, cx| shell.catalog_install(id, cx)))
                    .into_any_element()
                } else {
                    h_flex()
                        .h(px(ui::button_height(true)))
                        .gap(px(5.))
                        .px(px(6.))
                        .items_center()
                        .text_size(tokens::text_sm())
                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                        .text_color(tokens::check_on())
                        .child(ui::icon("check", 13.))
                        .child("Installed")
                        .into_any_element()
                };
                h_flex()
                    .gap(px(4.))
                    .items_center()
                    .child(lead)
                    .child(uninstall)
                    .into_any_element()
            }
        };
        let meta = h_flex()
            .gap(px(6.))
            .items_center()
            .min_w_0()
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text2())
            .child(div().truncate().child(format!("by {}", row.creator)))
            .when_some(row.votes, |this, votes| {
                this.child(div().text_color(tokens::text3()).child("\u{b7}"))
                    .child(
                        h_flex()
                            .flex_none()
                            .gap(px(3.))
                            .items_center()
                            .child(ui::icon("thumbs-up", 11.))
                            .child(votes_label(votes)),
                    )
            });
        let tile = icon_tile(&row);
        let detail = match row.failure {
            Some(failure) => div()
                .truncate()
                .text_color(tokens::text_error())
                .child(failure),
            None => div()
                .truncate()
                .text_color(tokens::text2())
                .child(row.blurb),
        };
        row_frame(("plugin-catalog-row", id))
            .child(tile)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(tokens::text_md())
                                    .line_height(tokens::line_md())
                                    .font_weight(tokens::WEIGHT_SEMIBOLD)
                                    .text_color(tokens::text())
                                    .child(row.name),
                            )
                            .when(row.verified, |this| this.child(badge())),
                    )
                    .child(meta)
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .min_w_0()
                            .child(detail),
                    ),
            )
            .child(div().flex_none().child(action))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::votes_label;

    #[test]
    fn votes_read_like_the_store() {
        assert_eq!(votes_label(950), "950");
        assert_eq!(votes_label(9600), "9.6k");
        assert_eq!(votes_label(57000), "57k");
        assert_eq!(votes_label(6720), "6.7k");
    }
}
