//! Home's page heads and its Home and Recent pages.

use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rbx_cloud::Experience;

use super::*;
use crate::home::RecentPlace;
use crate::launcher::home_window::{Games, HomeWindow, Page};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

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
        let search_left = (grid.content - 360.) / 2.;
        let owners = self.page == Page::MyGames && self.has_key() && self.owner_options.len() > 1;
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
                            div().flex_1().min_w_0().h_full().child(
                                Select::new(&self.owner_select)
                                    .appearance(false)
                                    .text_color(tokens::text())
                                    .h_full()
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
                    ui::field_frame(Some(360.), ui::panel2(), tokens::border(), "search")
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
                    .left(px(search_left + 368.))
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

    pub(super) fn section_title(title: impl Into<SharedString>) -> Div {
        ui::text(15., 20.)
            .font_weight(FontWeight::BOLD)
            .text_color(tokens::text())
            .child(title.into())
    }

    pub(super) fn section_head(
        title: &'static str,
        see_all: Option<Stateful<Div>>,
    ) -> impl IntoElement {
        h_flex()
            .h(px(22.))
            .items_center()
            .justify_between()
            .child(Self::section_title(title))
            .children(see_all)
    }

    pub(super) fn see_all(
        &self,
        id: &'static str,
        page: Page,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        ui::button(id, "See all", Weight::Ghost, true)
            .h(px(26.))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
    }

    pub(super) fn home_page(&mut self, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
        let recent: Vec<RecentPlace> = self.recent.iter().take(5).cloned().collect();
        let games: Vec<Experience> = match &self.games {
            Games::Loaded(list) => list
                .experiences
                .iter()
                .filter(|e| matches!(e.owner, rbx_cloud::Owner::User(_)))
                .take(grid.columns)
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        v_flex()
            .gap(px(20.))
            .child(self.page_head("Home", grid, false, cx))
            .when(!self.has_key(), |this| this.child(self.no_key_banner(cx)))
            .child(
                v_flex()
                    .gap(px(12.))
                    .child(Self::section_head("New", None))
                    .child(self.new_tiles(grid, cx)),
            )
            .when(!recent.is_empty(), |this| {
                let see_all = self.see_all("recent-see-all", Page::Recent, cx);
                this.child(
                    v_flex()
                        .gap(px(12.))
                        .child(Self::section_head("Recent", Some(see_all)))
                        .child(self.recent_table(&recent, grid, false, cx)),
                )
            })
            .when(self.has_key(), |this| {
                let see_all = self.see_all("games-see-all", Page::MyGames, cx);
                let body: AnyElement = match &self.games {
                    Games::Loading => self.skeleton_row(grid, grid.columns).into_any_element(),
                    _ => self.card_row(&games, grid, cx).into_any_element(),
                };
                this.child(
                    v_flex()
                        .gap(px(12.))
                        .child(Self::section_head("My Games", Some(see_all)))
                        .child(body),
                )
            })
            .into_any_element()
    }

    pub(super) fn no_key_banner(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(172.))
            .flex_none()
            .rounded(px(10.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .overflow_hidden()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .gap(px(8.))
                    .px(px(28.))
                    .child(
                        ui::text(10., 14.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(ui::accent())
                            .child("NO API KEY YET"),
                    )
                    .child(
                        ui::text(18., 24.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child("Open and publish your Roblox places"),
                    )
                    .child(ui::text(12.5, 19.).max_w(px(490.)).text_color(tokens::text2()).child(
                        "Add an Open Cloud API key once and My Games fills in with your experiences. Local .rbxl and .rbxlx files work without it.",
                    ))
                    .child(
                        h_flex().gap(px(8.)).mt(px(6.)).child(
                            ui::icon_button("banner-set-up", "key-round", "Set up a key", Weight::Primary, false)
                                .on_click(cx.listener(|this, _, window, cx| this.set_up_key(window, cx))),
                        ),
                    ),
            )
            .child(
                div()
                    .w(px(380.))
                    .h_full()
                    .flex_none()
                    .relative()
                    .border_l_1()
                    .border_color(tokens::border())
                    .overflow_hidden()
                    .child(dot_grid(380., 172.))
                    .child(
                        h_flex()
                            .absolute()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap(px(18.))
                            .child(div().size(px(56.)).rounded(px(12.)).bg(ui::accent()))
                            .child(
                                h_flex().w(px(64.)).gap(px(5.)).children(
                                    (0..6).map(|_| div().w(px(6.)).h(px(2.)).bg(tokens::accent_line())),
                                ),
                            )
                            .child(
                                h_flex()
                                    .size(px(72.))
                                    .rounded(px(16.))
                                    .bg(tokens::accent_soft())
                                    .border_1()
                                    .border_color(tokens::accent_line())
                                    .text_color(ui::accent())
                                    .items_center()
                                    .justify_center()
                                    .child(ui::icon("cloud-upload", 30.)),
                            ),
                    ),
            )
    }

    pub(super) fn new_tiles(&self, grid: &Grid, cx: &mut Context<Self>) -> impl IntoElement {
        let width = (grid.content - GAP * 3.) / 4.;
        let tile = |id: &'static str, dashed: bool| {
            h_flex()
                .id(id)
                .w(px(width))
                .h(px(72.))
                .items_center()
                .gap(px(12.))
                .pl(px(10.))
                .pr(px(12.))
                .rounded(px(8.))
                .border_1()
                .when(dashed, |this| {
                    this.border_dashed().border_color(tokens::border2())
                })
                .when(!dashed, |this| {
                    this.border_color(tokens::border()).bg(ui::panel2())
                })
                .cursor_pointer()
                .hover(|this| {
                    this.border_color(tokens::border2())
                        .bg(tokens::secondary_hover())
                })
        };
        let label = |title: &'static str, sub: &'static str| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    ui::text(12.5, 17.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(title),
                )
                .child(
                    ui::text(11., 15.)
                        .truncate()
                        .text_color(tokens::text3())
                        .child(sub),
                )
        };
        h_flex()
            .gap(px(GAP))
            .child(
                tile("new-baseplate", false)
                    .child(baseplate_thumb(52.))
                    .child(label("Baseplate", "Empty place with a spawn"))
                    .on_click(cx.listener(|this, _, _, cx| this.new_place(cx))),
            )
            .child(
                tile("new-open-file", true)
                    .child(
                        h_flex()
                            .size(px(52.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(ui::bg())
                            .items_center()
                            .justify_center()
                            .text_color(tokens::text2())
                            .child(ui::icon("folder-open", 20.)),
                    )
                    .child(label(
                        "Open a file\u{2026}",
                        ".rbxl or .rbxlx from your computer",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| this.open_file(cx))),
            )
    }
}
