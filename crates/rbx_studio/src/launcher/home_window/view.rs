//! Home's pages, sidebar and dialogs, drawn after boards `Home-Launcher`,
//! `Home-NoKey`, `Home-Recent`, `Home-MyGames*`, `Home-AddPlace`,
//! `Home-LocalCopy`, `Home-Downloading` and `Home-DownloadError`.

use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone};
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rbx_cloud::{Experience, Owner, Visibility};

use super::{file_name, Dialog, Games, HomeWindow, KeyState, LinkState, Page};
use crate::home::RecentPlace;
use crate::launcher::ui::{self, Weight};
use crate::tokens;

const SIDEBAR: f32 = 232.;
const GAP: f32 = 16.;

/// The width the page's content gets, and the My Games grid over it:
/// `clamp(floor((w + 16) / 192), 3, 8)` columns (#0075 §Windows).
struct Grid {
    content: f32,
    columns: usize,
    card: f32,
}

/// `clamp(floor((content + 16) / 192), 3, 8)`.
pub(super) fn columns(content: f32) -> usize {
    (((content + GAP) / 192.).floor() as usize).clamp(3, 8)
}

impl Grid {
    fn new(window: &Window) -> Self {
        let content = (f32::from(window.viewport_size().width) - SIDEBAR - 64.).max(300.);
        let columns = columns(content);
        let card = (content - GAP * (columns as f32 - 1.)) / columns as f32;
        Grid {
            content,
            columns,
            card,
        }
    }
}

impl HomeWindow {
    pub(super) fn render_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let grid = Grid::new(window);
        let crumb = match self.page {
            Page::Home => "Home",
            Page::Recent => "Recent",
            Page::MyGames => "My Games",
        };
        let page = match self.page {
            Page::Home => self.home_page(&grid, cx),
            Page::Recent => self.recent_page(&grid, cx),
            Page::MyGames => self.games_page(&grid, cx),
        };
        let dialog = self.dialog_view(cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(ui::panel())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text())
            .text_size(px(13.))
            .child(ui::titlebar(crumb, self.grab.clone(), |_, cx| cx.quit()))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .id("home-main")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .child(
                                v_flex()
                                    .gap(px(20.))
                                    .pt(px(24.))
                                    .px(px(32.))
                                    .pb(px(32.))
                                    .child(page),
                            ),
                    ),
            )
            .children(dialog)
    }

    // ------------------------------------------------------------ sidebar

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = |id: &'static str,
                   glyph: &'static str,
                   label: &'static str,
                   page: Page,
                   cx: &mut Context<Self>| {
            let active = self.page == page;
            h_flex()
                .id(id)
                .h(px(36.))
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .rounded(px(6.))
                .text_size(px(12.5))
                .line_height(px(17.))
                .cursor_pointer()
                .map(|this| {
                    if active {
                        this.bg(tokens::accent_soft())
                            .text_color(ui::accent())
                            .font_weight(FontWeight::SEMIBOLD)
                    } else {
                        this.text_color(tokens::text2()).hover(|this| {
                            this.bg(tokens::hover_subtle()).text_color(tokens::text())
                        })
                    }
                })
                .child(ui::icon(glyph, 16.))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
        };
        v_flex()
            .w(px(SIDEBAR))
            .flex_none()
            .gap(px(4.))
            .pt(px(18.))
            .px(px(12.))
            .pb(px(12.))
            .bg(ui::bg())
            .border_r_1()
            .border_color(tokens::border())
            .child(
                h_flex()
                    .h(px(32.))
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .mb(px(10.))
                    .child(
                        div()
                            .size(px(22.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(ui::accent()),
                    )
                    .child(
                        ui::text(14., 20.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child("RbxNative"),
                    ),
            )
            .child(
                h_flex()
                    .id("home-new-place")
                    .h(px(36.))
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens::text())
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::hover_subtle()))
                    .child(
                        h_flex()
                            .size(px(20.))
                            .flex_none()
                            .rounded_full()
                            .bg(ui::accent())
                            .text_color(ui::bg())
                            .items_center()
                            .justify_center()
                            .child(ui::icon("plus", 13.)),
                    )
                    .child("New place")
                    .on_click(cx.listener(|this, _, _, cx| this.new_place(cx))),
            )
            .child(
                h_flex()
                    .id("home-open-file")
                    .h(px(36.))
                    .mb(px(6.))
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .text_color(tokens::text2())
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::hover_subtle()).text_color(tokens::text()))
                    .child(ui::icon("folder-open", 16.))
                    .child("Open file\u{2026}")
                    .on_click(cx.listener(|this, _, _, cx| this.open_file(cx))),
            )
            .child(nav("nav-home", "house", "Home", Page::Home, cx))
            .child(nav("nav-recent", "clock", "Recent", Page::Recent, cx))
            .child(nav("nav-games", "gamepad-2", "My Games", Page::MyGames, cx))
            .child(div().flex_1())
            .child(self.account_card(cx))
    }

    fn account_card(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.has_key() {
            return v_flex()
                .gap(px(6.))
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(tokens::border())
                .bg(ui::panel())
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("key-round", 14.).text_color(tokens::text2()))
                        .child(
                            ui::text(12., 16.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text())
                                .child("No API key"),
                        ),
                )
                .child(
                    ui::text(11., 15.).text_color(tokens::text2()).child(
                        "Local files work. Opening and publishing Roblox places needs a key.",
                    ),
                )
                .child(
                    ui::text(11.5, 16.)
                        .id("sidebar-set-up")
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::accent())
                        .cursor_pointer()
                        .hover(|this| this.text_color(tokens::text()))
                        .child("Set up a key")
                        .on_click(cx.listener(|this, _, window, cx| this.set_up_key(window, cx))),
                )
                .into_any_element();
        }
        let (name, status, dot) = match &self.key {
            KeyState::Ready { owner, report } if report.ready() => {
                (owner.clone(), "Key ready", ui::green())
            }
            KeyState::Ready { owner, .. } => (owner.clone(), "Key needs attention", ui::red()),
            KeyState::Checking => ("Your key".to_string(), "Checking\u{2026}", tokens::text3()),
            _ => (
                "Your key".to_string(),
                "Couldn\u{2019}t check",
                tokens::text3(),
            ),
        };
        let initial = name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        h_flex()
            .id("sidebar-account")
            .items_center()
            .gap(px(10.))
            .py(px(10.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel())
            .cursor_pointer()
            .hover(|this| this.bg(tokens::hover_subtle()))
            .child(
                h_flex()
                    .size(px(28.))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(0x2C3050))
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(ui::accent())
                    .child(initial),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        ui::text(12., 16.)
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(tokens::text2())
                            .child(div().size(px(6.)).rounded_full().bg(dot))
                            .child(status),
                    ),
            )
            .child(ui::icon("chevron-right", 14.).text_color(tokens::text3()))
            .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx)))
            .into_any_element()
    }

    // -------------------------------------------------------------- pages

    fn page_head(
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

    fn section_title(title: impl Into<SharedString>) -> Div {
        ui::text(15., 20.)
            .font_weight(FontWeight::BOLD)
            .text_color(tokens::text())
            .child(title.into())
    }

    fn section_head(title: &'static str, see_all: Option<Stateful<Div>>) -> impl IntoElement {
        h_flex()
            .h(px(22.))
            .items_center()
            .justify_between()
            .child(Self::section_title(title))
            .children(see_all)
    }

    fn see_all(&self, id: &'static str, page: Page, cx: &mut Context<Self>) -> Stateful<Div> {
        ui::button(id, "See all", Weight::Ghost, true)
            .h(px(26.))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
    }

    fn home_page(&mut self, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
        let recent: Vec<RecentPlace> = self.recent.iter().take(5).cloned().collect();
        let games: Vec<Experience> = match &self.games {
            Games::Loaded(list) => list
                .experiences
                .iter()
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

    fn no_key_banner(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .flex_none()
                    .relative()
                    .border_l_1()
                    .border_color(tokens::border())
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
                                    .child(ui::icon("cloud", 30.)),
                            ),
                    ),
            )
    }

    fn new_tiles(&self, grid: &Grid, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn recent_page(&mut self, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
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

    fn recent_table(
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
                    .hover(|this| this.bg(tokens::secondary_hover()))
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

    fn games_page(&mut self, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
        if !self.has_key() {
            return v_flex()
                .gap(px(20.))
                .child(self.page_head("My Games", grid, false, cx))
                .child(self.no_key_banner(cx))
                .into_any_element();
        }
        let head = self.page_head("My Games", grid, true, cx);
        let body: AnyElement = match &self.games {
            Games::Loading => v_flex()
                .gap(px(12.))
                .child(
                    h_flex()
                        .h(px(22.))
                        .items_center()
                        .gap(px(8.))
                        .child(Self::section_title("Personal"))
                        .child(
                            h_flex()
                                .ml(px(4.))
                                .gap(px(6.))
                                .text_size(px(11.5))
                                .line_height(px(16.))
                                .text_color(tokens::text3())
                                .child(ui::spinner("games-loading", 12.))
                                .child("Loading your experiences\u{2026}"),
                        ),
                )
                .child(self.skeleton_row(grid, grid.columns * 2))
                .into_any_element(),
            Games::Failed(reason) => self
                .empty_state(
                    "Couldn\u{2019}t load your experiences",
                    reason.clone(),
                    false,
                    cx,
                )
                .into_any_element(),
            Games::Loaded(list) if list.experiences.is_empty() => self
                .empty_state(
                    "No experiences to show",
                    "Roblox lists your public experiences here, plus every experience your key is restricted to. Add a private one by its place ID or link.".to_string(),
                    true,
                    cx,
                )
                .into_any_element(),
            Games::Loaded(list) => {
                let query = self.search.read(cx).value().to_lowercase();
                let shown: Vec<Experience> = list
                    .experiences
                    .iter()
                    .filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query))
                    .cloned()
                    .collect();
                let personal: Vec<Experience> =
                    shown.iter().filter(|e| matches!(e.owner, Owner::User(_))).cloned().collect();
                let mut group_ids: Vec<u64> = shown
                    .iter()
                    .filter_map(|e| match e.owner {
                        Owner::Group(id) => Some(id),
                        _ => None,
                    })
                    .collect();
                group_ids.dedup();
                let groups: Vec<(String, Vec<Experience>)> = group_ids
                    .iter()
                    .map(|&id| {
                        let name = list
                            .groups
                            .iter()
                            .find(|g| g.id == id)
                            .map(|g| g.name.clone())
                            .unwrap_or_else(|| format!("Group {id}"));
                        let games = shown
                            .iter()
                            .filter(|e| e.owner == Owner::Group(id))
                            .cloned()
                            .collect();
                        (name, games)
                    })
                    .collect();
                v_flex()
                    .gap(px(20.))
                    .when(self.note_visible(), |this| this.child(self.partial_note(cx)))
                    .when(!self.note_visible() && self.link_open, |this| {
                        this.child(self.link_row(cx))
                    })
                    .when(!personal.is_empty(), |this| {
                        this.child(self.games_section(None, "Personal".into(), &personal, grid, cx))
                    })
                    .when(self.groups_off(), |this| this.child(self.groups_off_note(None, cx)))
                    .children(groups.into_iter().map(|(name, games)| {
                        self.games_section(Some("users"), name.into(), &games, grid, cx)
                    }))
                    .into_any_element()
            }
        };
        v_flex()
            .gap(px(20.))
            .child(head)
            .child(body)
            .into_any_element()
    }

    fn games_section(
        &self,
        glyph: Option<&'static str>,
        title: SharedString,
        games: &[Experience],
        grid: &Grid,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap(px(12.))
            .child(
                h_flex()
                    .h(px(22.))
                    .items_center()
                    .gap(px(8.))
                    .when_some(glyph, |this, glyph| {
                        this.child(ui::icon(glyph, 14.).text_color(tokens::text2()))
                    })
                    .child(Self::section_title(title))
                    .child(
                        ui::mono(11., 15.)
                            .text_color(tokens::text3())
                            .child(games.len().to_string()),
                    ),
            )
            .child(self.card_row(games, grid, cx))
    }

    fn card_row(
        &self,
        games: &[Experience],
        grid: &Grid,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .flex_wrap()
            .gap(px(GAP))
            .children(games.iter().map(|game| self.card(game, grid, cx)))
    }

    fn card(&self, game: &Experience, grid: &Grid, cx: &mut Context<Self>) -> impl IntoElement {
        let thumb = grid.card - 22.;
        let open = game.clone();
        let universe = game.universe_id;
        let (visibility, glyph) = visibility_label(&game.visibility);
        v_flex()
            .id(SharedString::from(format!("game-{}", game.universe_id)))
            .w(px(grid.card))
            .gap(px(8.))
            .p(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .cursor_pointer()
            .hover(|this| this.border_color(tokens::border2()).bg(tokens::secondary_hover()))
            .child(self.icon_box(game, thumb, 6.))
            .child(
                h_flex()
                    .h(px(24.))
                    .items_center()
                    .justify_between()
                    .child(ui::pill(visibility, Some(glyph), false))
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("game-more-{universe}")))
                            .size(px(24.))
                            .rounded(px(5.))
                            .items_center()
                            .justify_center()
                            .text_color(tokens::text3())
                            .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text("Open on the Creator Dashboard", window, cx)
                            })
                            .child(ui::icon("ellipsis", 14.))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                cx.open_url(&format!(
                                    "https://create.roblox.com/dashboard/creations/experiences/{universe}/overview"
                                ))
                            }),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(1.))
                    .min_w_0()
                    .child(
                        ui::text(12.5, 17.)
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child(game.name.clone()),
                    )
                    .child(
                        ui::mono(10.5, 15.)
                            .truncate()
                            .text_color(tokens::text3())
                            .child(format!("Place {}", game.root_place_id)),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_experience(open.clone(), cx)))
    }

    /// The experience's icon, or the "No icon yet" placeholder.
    fn icon_box(&self, game: &Experience, size: f32, radius: f32) -> impl IntoElement {
        let image: Option<Arc<RenderImage>> = self.icons.get(&game.universe_id).cloned();
        div()
            .size(px(size))
            .flex_none()
            .rounded(px(radius))
            .overflow_hidden()
            .bg(ui::bg())
            .map(|this| match image {
                Some(image) => this.child(img(image).size_full().object_fit(ObjectFit::Cover)),
                None => this.child(
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .text_color(tokens::text3())
                        .child(ui::icon("image", if size > 60. { 30. } else { 18. }))
                        .when(size > 60., |this| {
                            this.child(ui::text(10.5, 14.).child("No icon yet"))
                        }),
                ),
            })
    }

    fn skeleton_row(&self, grid: &Grid, count: usize) -> impl IntoElement {
        let thumb = grid.card - 22.;
        h_flex()
            .flex_wrap()
            .gap(px(GAP))
            .children((0..count).map(|_| {
                v_flex()
                    .w(px(grid.card))
                    .gap(px(8.))
                    .p(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel2())
                    .child(div().size(px(thumb)).rounded(px(6.)).bg(ui::wash_faint()))
                    .child(
                        h_flex()
                            .h(px(24.))
                            .items_center()
                            .child(div().w(px(52.)).h(px(20.)).rounded(px(3.)).bg(ui::wash())),
                    )
                    .child(
                        v_flex()
                            .gap(px(7.))
                            .pt(px(3.))
                            .pb(px(2.))
                            .child(
                                div()
                                    .w(relative(0.78))
                                    .h(px(10.))
                                    .rounded(px(3.))
                                    .bg(ui::wash()),
                            )
                            .child(
                                div()
                                    .w(relative(0.54))
                                    .h(px(8.))
                                    .rounded(px(3.))
                                    .bg(ui::wash()),
                            ),
                    )
            }))
    }

    fn partial_note(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_start()
            .gap(px(12.))
            .pt(px(12.))
            .px(px(14.))
            .pb(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(div().pt(px(1.)).text_color(tokens::text2()).child(ui::icon("info", 16.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(10.))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(12.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(1.))
                                    .child(
                                        ui::text(12.5, 17.)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(tokens::text())
                                            .child("Missing a private experience? Add it by its place ID or link"),
                                    )
                                    .child(ui::text(12., 17.).text_color(tokens::text2()).child(
                                        "Roblox only lists your public experiences for this key. Or restrict the key to your games on the Creator Dashboard, and they all show up here.",
                                    )),
                            )
                            .child(
                                ui::button("note-manage", "Manage key", Weight::Secondary, true)
                                    .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                            ),
                    )
                    .child(self.link_row(cx)),
            )
    }

    /// The 420 px link field, Add, and the error line under them.
    fn link_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.link_state.clone();
        let resolving = state == LinkState::Resolving;
        let empty = self.link.read(cx).value().trim().is_empty();
        let border = match state {
            LinkState::Resolving => tokens::accent_line(),
            LinkState::Idle => tokens::border(),
            _ => ui::red_line(),
        };
        let error: Option<(String, bool)> = match state {
            LinkState::NotALink => Some(("That isn\u{2019}t a place ID or a Roblox game link.".into(), false)),
            LinkState::NoPlace(id) => Some((format!("No place with ID {id}."), false)),
            LinkState::NoAccess => Some((
                "Your key can\u{2019}t open this place. It isn\u{2019}t yours, or the key is restricted to other experiences.".into(),
                true,
            )),
            LinkState::Unreachable => Some(("Couldn\u{2019}t reach Roblox. Check your connection and try again.".into(), false)),
            _ => None,
        };
        v_flex()
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        ui::field_frame(Some(420.), ui::panel(), border, "link")
                            .child(
                                div().flex_1().min_w_0().h_full().child(
                                    Input::new(&self.link)
                                        .appearance(false)
                                        .disabled(resolving)
                                        .h_full()
                                        .px(px(0.))
                                        .font_family(tokens::FONT_FAMILY_MONO)
                                        .text_size(px(11.5))
                                        .text_color(tokens::text()),
                                ),
                            )
                            .when(resolving, |this| {
                                this.child(
                                    h_flex()
                                        .flex_none()
                                        .gap(px(6.))
                                        .text_size(px(11.5))
                                        .line_height(px(16.))
                                        .text_color(tokens::text2())
                                        .child(ui::spinner("link-resolving", 12.))
                                        .child("Resolving\u{2026}"),
                                )
                            }),
                    )
                    .child(if empty || resolving {
                        ui::disabled_button("link-add", "Add")
                            .w(px(64.))
                            .h(px(32.))
                            .px(px(0.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .into_any_element()
                    } else {
                        ui::button("link-add", "Add", Weight::Primary, false)
                            .w(px(64.))
                            .h(px(32.))
                            .px(px(0.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .on_click(cx.listener(|this, _, window, cx| this.add_link(window, cx)))
                            .into_any_element()
                    }),
            )
            .children(error.map(|(line, manage)| {
                h_flex()
                    .items_center()
                    .gap(px(6.))
                    .mt(px(6.))
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(ui::red())
                    .child(ui::icon("circle-alert", 12.))
                    .child(line)
                    .when(manage, |this| {
                        this.child(
                            div()
                                .id("link-manage")
                                .ml(px(4.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .child("Manage key")
                                .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                        )
                    })
            }))
    }

    fn groups_off_note(&self, width: Option<f32>, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .when_some(width, |this, w| this.w(px(w)))
            .items_center()
            .gap(px(12.))
            .py(px(12.))
            .px(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(ui::icon("users", 16.).text_color(tokens::text2()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        ui::text(12.5, 17.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child("Group experiences are off"),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(tokens::text2())
                            .child("Add")
                            .child(
                                ui::mono(11., 17.)
                                    .text_color(tokens::text())
                                    .child("legacy-group:manage"),
                            )
                            .child("to your key to list the games of groups you manage."),
                    ),
            )
            .child(
                ui::button("groups-manage", "Manage key", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
            )
    }

    fn empty_state(
        &self,
        title: &'static str,
        text: String,
        with_link: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap(px(20.))
            .child(
                v_flex()
                    .flex_none()
                    .items_center()
                    .gap(px(12.))
                    .pt(px(64.))
                    .pb(px(8.))
                    .child(
                        h_flex()
                            .size(px(56.))
                            .rounded(px(14.))
                            .bg(ui::panel2())
                            .border_1()
                            .border_color(tokens::border())
                            .items_center()
                            .justify_center()
                            .text_color(tokens::text2())
                            .child(ui::icon("folder", 22.)),
                    )
                    .child(
                        ui::text(16., 22.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child(title),
                    )
                    .child(
                        ui::text(12.5, 19.)
                            .max_w(px(480.))
                            .text_center()
                            .text_color(tokens::text2())
                            .child(text),
                    )
                    .when(with_link && self.note_visible(), |this| {
                        this.child(div().mt(px(6.)).child(self.link_row(cx)))
                    })
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .mt(px(4.))
                            .child(
                                ui::icon_button(
                                    "empty-manage",
                                    "key-round",
                                    "Manage key",
                                    Weight::Secondary,
                                    false,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                            )
                            .child(
                                ui::external_button(
                                    "empty-dashboard",
                                    "Open Creator Dashboard",
                                    Weight::Secondary,
                                    false,
                                )
                                .on_click(|_, _, cx| {
                                    cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL)
                                }),
                            ),
                    ),
            )
            .when(self.groups_off(), |this| {
                this.child(
                    h_flex()
                        .justify_center()
                        .child(self.groups_off_note(Some(640.), cx)),
                )
            })
    }

    // ------------------------------------------------------------ dialogs

    fn dialog_view(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        Some(match dialog {
            Dialog::LocalCopy {
                experience,
                path,
                replace,
            } => {
                let replace = *replace;
                let radio = |id: &'static str,
                             glyph: &'static str,
                             title: &'static str,
                             text: &'static str,
                             on: bool,
                             value: bool,
                             cx: &mut Context<Self>| {
                    h_flex()
                        .id(id)
                        .items_center()
                        .gap(px(12.))
                        .py(px(12.))
                        .px(px(14.))
                        .rounded(px(8.))
                        .border_1()
                        .cursor_pointer()
                        .map(|this| {
                            if on {
                                this.border_color(tokens::accent_line())
                                    .bg(tokens::accent_soft())
                            } else {
                                this.border_color(tokens::border())
                                    .bg(ui::panel2())
                                    .hover(|this| this.bg(tokens::secondary_hover()))
                            }
                        })
                        .child(div().size(px(16.)).flex_none().rounded_full().map(|this| {
                            if on {
                                this.border(px(5.)).border_color(ui::accent())
                            } else {
                                this.border_1().border_color(tokens::border2())
                            }
                        }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_size(px(12.5))
                                        .line_height(px(17.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(tokens::text())
                                        .child(ui::icon(glyph, 13.).text_color(tokens::text2()))
                                        .child(title),
                                )
                                .child(ui::text(11.5, 17.).text_color(tokens::text2()).child(text)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(Dialog::LocalCopy { replace, .. }) = &mut this.dialog {
                                *replace = value;
                            }
                            cx.notify();
                        }))
                };
                let saved = std::fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .map(|time| {
                        let secs = time
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        format!(" \u{b7} saved {}", opened_label(secs).to_lowercase())
                    })
                    .unwrap_or_default();
                let body = v_flex()
                    .child(
                        ui::mono(11., 15.)
                            .mt(px(12.))
                            .mx(px(20.))
                            .ml(px(78.))
                            .truncate()
                            .text_color(tokens::text3())
                            .child(format!("{}{saved}", display_path(path))),
                    )
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .pt(px(16.))
                            .px(px(20.))
                            .child(radio("localcopy-keep", "folder-open", "Open my local copy", "Keep working where you left off.", !replace, false, cx))
                            .child(radio(
                                "localcopy-replace",
                                "download",
                                "Download the published version",
                                "Replaces your local copy. Changes you haven\u{2019}t published are lost.",
                                replace,
                                true,
                                cx,
                            )),
                    )
                    .into_any_element();
                let experience = experience.clone();
                let path = path.clone();
                ui::dialog(
                    520.,
                    self.icon_box(&experience, 44., 8.).into_any_element(),
                    format!("{} is already on this computer", experience.name),
                    "You opened it before. Your local copy may have changes you haven\u{2019}t published.",
                    Some(body),
                    vec![
                        ui::button("localcopy-cancel", "Cancel", Weight::Secondary, false)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                            .into_any_element(),
                        ui::button(
                            "localcopy-go",
                            if replace { "Download and replace" } else { "Open local copy" },
                            Weight::Primary,
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if replace {
                                this.download(experience.clone(), true, cx);
                            } else {
                                this.dialog = None;
                                let _ = crate::home::remember(RecentPlace {
                                    path: path.clone(),
                                    universe_id: Some(experience.universe_id),
                                    place_id: Some(experience.root_place_id),
                                    name: Some(experience.name.clone()),
                                    opened: None,
                                });
                                this.open_path_later(path.clone(), cx);
                            }
                        }))
                        .into_any_element(),
                    ],
                )
                .into_any_element()
            }
            Dialog::Downloading { experience } => {
                let body = v_flex()
                    .gap(px(8.))
                    .pt(px(18.))
                    .pr(px(20.))
                    .pl(px(78.))
                    .child(indeterminate_bar())
                    .child(
                        h_flex()
                            .justify_between()
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(tokens::text3())
                            .child("Downloading\u{2026}"),
                    )
                    .into_any_element();
                ui::dialog(
                    520.,
                    self.icon_box(experience, 44., 8.).into_any_element(),
                    format!("Opening {}", experience.name),
                    "Downloading the published place. It opens in the editor when it\u{2019}s done.",
                    Some(body),
                    vec![ui::button("download-cancel", "Cancel", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                        .into_any_element()],
                )
                .into_any_element()
            }
            Dialog::Error {
                experience,
                title,
                status,
                reason,
            } => {
                let status_line = status.map(|status| {
                    format!(
                        "{status} {}",
                        match status {
                            401 => "Unauthorized",
                            403 => "Forbidden",
                            404 => "Not Found",
                            429 => "Too Many Requests",
                            _ => "",
                        }
                    )
                });
                let body = h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(14.))
                    .mr(px(20.))
                    .ml(px(78.))
                    .py(px(10.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::bg())
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(px(11.))
                    .line_height(px(17.))
                    .text_color(tokens::text2())
                    .when_some(status_line, |this, line| {
                        this.child(div().text_color(ui::red()).child(line))
                            .child("\u{b7}")
                    })
                    .child(reason.clone())
                    .into_any_element();
                let auth = matches!(status, Some(401 | 403));
                let retry = experience.clone();
                let mut footer = Vec::new();
                if auth {
                    footer.push(
                        ui::icon_button(
                            "error-manage",
                            "key-round",
                            "Manage key",
                            Weight::Secondary,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx)))
                        .into_any_element(),
                    );
                }
                footer.push(
                    ui::button("error-close", "Close", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                        .into_any_element(),
                );
                if let Some(retry) = retry {
                    footer.push(
                        ui::button("error-retry", "Try again", Weight::Primary, false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.download(retry.clone(), true, cx)
                            }))
                            .into_any_element(),
                    );
                }
                ui::dialog(
                    520.,
                    ui::dialog_glyph("circle-alert", ui::red(), ui::red_soft()),
                    title.clone(),
                    if experience.is_some() {
                        "Roblox refused the download. Your local copy, if you have one, wasn\u{2019}t changed."
                    } else {
                        "Nothing was changed."
                    },
                    Some(body),
                    footer,
                )
                .into_any_element()
            }
        })
    }
}

/// A 30 % accent segment sliding along a 6 px track (still under reduced
/// motion). The download is one blocking request with no running total,
/// so the board's determinate variant has nothing to show.
fn indeterminate_bar() -> impl IntoElement {
    let track = div()
        .relative()
        .h(px(6.))
        .rounded(px(3.))
        .bg(rgba(0xFFFFFF0F))
        .overflow_hidden();
    let segment = div()
        .absolute()
        .top_0()
        .h_full()
        .w(relative(0.3))
        .rounded(px(3.))
        .bg(ui::accent());
    if tokens::reduced_motion() {
        return track.child(segment.left(relative(0.35))).into_any_element();
    }
    track
        .child(segment.with_animation(
            "download-bar",
            Animation::new(std::time::Duration::from_millis(1200)).repeat(),
            |segment, delta| segment.left(relative(-0.3 + 1.3 * delta)),
        ))
        .into_any_element()
}

/// Studio's own Baseplate thumbnail, drawn: sky over a grey chequered
/// floor.
fn baseplate_thumb(size: f32) -> impl IntoElement {
    let horizon = size * 0.42;
    let cell = 11.;
    let floor_rows = ((size - horizon) / cell).ceil() as usize;
    let floor_cols = (size / cell).ceil() as usize;
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .rounded(px(6.))
        .overflow_hidden()
        .bg(rgb(0x8FA6BD))
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(horizon * 0.6))
                .w_full()
                .h(px(horizon * 0.4))
                .bg(rgb(0xCDD7E0)),
        )
        .child(
            v_flex()
                .absolute()
                .left_0()
                .top(px(horizon))
                .w_full()
                .children((0..floor_rows).map(|row| {
                    h_flex().children((0..floor_cols).map(move |col| {
                        div()
                            .size(px(cell))
                            .flex_none()
                            .bg(if (row + col) % 2 == 0 {
                                rgb(0x6D7178)
                            } else {
                                rgb(0x62666D)
                            })
                    }))
                })),
        )
}

fn visibility_label(visibility: &Visibility) -> (&'static str, &'static str) {
    match visibility {
        Visibility::Public => ("Public", "globe"),
        Visibility::Private => ("Private", "lock"),
        Visibility::Other(_) => ("Friends", "users"),
    }
}

fn recent_name(place: &RecentPlace) -> String {
    file_name(&place.path)
}

/// `~/…` for anything under the home directory.
pub(super) fn display_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && full.starts_with(&home) => {
            format!("~{}", &full[home.len()..])
        }
        _ => full,
    }
}

fn display_dir(path: &std::path::Path) -> String {
    display_path(path.parent().unwrap_or(path))
}

/// `Today, 1:05 AM`, `Yesterday, 6:46 PM`, `Sep 17, 1:23 PM`.
fn opened_label(secs: i64) -> String {
    let Some(when) = Local.timestamp_opt(secs, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let day = when.date_naive();
    let time = when.format("%-I:%M %p");
    if day == today {
        format!("Today, {time}")
    } else if Some(day) == today.pred_opt() {
        format!("Yesterday, {time}")
    } else if day.year() == today.year() {
        format!("{}, {time}", when.format("%b %-d"))
    } else {
        format!("{}, {time}", when.format("%b %-d, %Y"))
    }
}
