//! Home's page head: the title, the owner dropdown, search and the buttons
//! beside it.

use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::*;
use crate::launcher::home_window::{Games, HomeWindow, Page};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

/// The page head's widths that the search has to clear: the title plus the
/// owner dropdown on the left, Add by link and New experience on the right.
const OWNERS_END: f32 = 130. + 16. + 220.;
const ADD_BY_LINK: f32 = 114.;
const NEW_EXPERIENCE: f32 = 148.;
const HEAD_GAP: f32 = 8.;
const SEARCH_WIDTH: f32 = 360.;

/// Where the search goes in a head `content` wide, and how wide it is:
/// centred while it fits, otherwise pushed right of the owner dropdown and
/// narrowed so it never runs under the buttons beside it.
fn search_slot(content: f32, owners: bool, add_by_link: bool, new_experience: bool) -> (f32, f32) {
    let lead = if owners {
        OWNERS_END + HEAD_GAP * 1.5
    } else {
        0.
    };
    let trail = add_by_link as u8 as f32 * (ADD_BY_LINK + HEAD_GAP)
        + new_experience as u8 as f32 * (NEW_EXPERIENCE + HEAD_GAP * 1.5);
    let width = SEARCH_WIDTH.min(content - lead - trail).max(160.);
    let left = ((content - width) / 2.)
        .max(lead)
        .min(content - trail - width);
    (left, width)
}

impl HomeWindow {
    pub(super) fn page_head(
        &self,
        title: &'static str,
        grid: &Grid,
        search: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let add_by_link = self.page == Page::MyGames
            && self.has_key()
            && !self.note_visible()
            && matches!(self.games, Games::Loaded(ref g) if !g.experiences.is_empty());
        let new_experience = self.offer_new && self.has_key();
        let owners = self.page == Page::MyGames && self.has_key() && self.owner_options.len() > 1;
        let (search_left, search_width) =
            search_slot(grid.content, owners, add_by_link, new_experience);
        div()
            .relative()
            .h(px(36.))
            .flex_none()
            .flex()
            .items_center()
            .child(
                ui::text(22., 28.)
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens::text())
                    .child(title),
            )
            .when(owners, |this| {
                // Whose games: the account's own, or one group's.
                this.child(
                    ui::field_frame(Some(220.), ui::panel2(), tokens::border(), "users")
                        .ml(px(16.))
                        .child(
                            ui::select_slot().child(
                                Select::new(&self.owner_select)
                                    .appearance(false)
                                    .text_color(tokens::text())
                                    .w_full()
                                    .py_0()
                                    .px_0()
                                    .text_size(px(12.))
                                    .icon(ui::icon("chevron-down", 12.))
                                    .menu_width(px(260.))
                                    .accessibility_label("Whose games"),
                            ),
                        ),
                )
            })
            .when(search, |this| {
                this.child(
                    ui::field_frame(Some(search_width), ui::panel2(), tokens::border(), "search")
                        .absolute()
                        .left(px(search_left))
                        .top(px(2.))
                        .child(
                            div().flex_1().min_w_0().h_full().child(
                                Input::new(&self.search)
                                    .appearance(false)
                                    .h_full()
                                    .px(px(0.))
                                    .text_size(px(12.))
                                    .text_color(tokens::text()),
                            ),
                        ),
                )
            })
            .when(
                self.page == Page::Recent && !self.recent.is_empty(),
                |this| this.child(div().ml_auto().child(self.recent_view_toggle(cx))),
            )
            .when(new_experience, |this| {
                this.child(if self.session.is_some() {
                    ui::icon_button(
                        "games-new-experience",
                        "plus",
                        "New experience",
                        Weight::Primary,
                        false,
                    )
                    .h(px(32.))
                    .ml_auto()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_new_experience(window, cx)
                    }))
                    .into_any_element()
                } else {
                    ui::disabled_button("games-new-experience", "New experience")
                        .h(px(32.))
                        .ml_auto()
                        .tooltip(|window, cx| {
                            crate::shell::tooltip::text(
                                "Creating an experience needs a Roblox sign-in: Settings \u{203a} Account \u{203a} Roblox sign-in",
                                window,
                                cx,
                            )
                        })
                        .into_any_element()
                })
            })
            .when(add_by_link, |this| {
                this.child(
                    ui::icon_button(
                        "games-add-link",
                        "link",
                        "Add by link",
                        Weight::Secondary,
                        false,
                    )
                    .h(px(32.))
                    .absolute()
                    .left(px(search_left + search_width + HEAD_GAP))
                    .top(px(2.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.link_open = !this.link_open;
                        let focus = this.link.read(cx).focus_handle(cx);
                        if this.link_open {
                            focus.focus(window, cx);
                        }
                        cx.notify();
                    })),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{search_slot, ADD_BY_LINK, HEAD_GAP, NEW_EXPERIENCE, OWNERS_END, SEARCH_WIDTH};

    #[test]
    fn the_search_clears_the_dropdown_and_the_buttons_in_the_picker() {
        let content = 976.;
        let (left, width) = search_slot(content, true, true, true);
        assert!(left >= OWNERS_END);
        assert!(left + width + HEAD_GAP + ADD_BY_LINK + HEAD_GAP + NEW_EXPERIENCE <= content);
        // With room to spare it stays centred at full width.
        assert_eq!(search_slot(1400., false, true, false), (520., SEARCH_WIDTH));
    }
}
