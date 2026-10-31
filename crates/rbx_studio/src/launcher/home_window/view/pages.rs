//! Home's page heads and its Home and Recent pages.

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
    pub(super) fn section_title(title: impl Into<SharedString>) -> Div {
        ui::text(15., 20.)
            .font_weight(FontWeight::BOLD)
            .text_color(tokens::text())
            .child(title.into())
    }

    pub(super) fn section_head(
        title: &'static str,
        see_all: Option<AnyElement>,
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
        // The icons fill one row, as My Games' cards do.
        let shown = if self.recent_icons { grid.columns } else { 5 };
        let recent: Vec<RecentPlace> = self.recent.iter().take(shown).cloned().collect();
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
            .children(self.recovered_section(cx))
            .when(!recent.is_empty(), |this| {
                let tools = h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(self.recent_view_toggle(cx))
                    .child(self.see_all("recent-see-all", Page::Recent, cx));
                this.child(
                    v_flex()
                        .gap(px(12.))
                        .child(Self::section_head("Recent", Some(tools.into_any_element())))
                        .child(self.recent_body(&recent, grid, false, cx)),
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
                        .child(Self::section_head(
                            "My Games",
                            Some(see_all.into_any_element()),
                        ))
                        .child(body),
                )
            })
            .into_any_element()
    }

    pub(super) fn no_key_banner(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // A keychain that never answered at startup may still hold a key,
        // so the banner says that rather than "no key yet".
        let stalled = crate::key_store::stalled();
        let (eyebrow, title) = if stalled {
            ("KEYCHAIN DIDN'T ANSWER", "Your saved key wasn't loaded")
        } else {
            ("NO API KEY YET", "Open and publish your Roblox places")
        };
        h_flex()
            .min_h(px(172.))
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
                    .py(px(24.))
                    .child(
                        ui::text(10., 14.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(ui::accent())
                            .child(eyebrow),
                    )
                    .child(
                        ui::text(18., 24.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child(title),
                    )
                    .child(ui::text(12.5, 19.).max_w(px(490.)).text_color(tokens::text2()).child(
                        if stalled {
                            "The system keychain didn't answer when RbxNative started, so it opened without your key. Unlock the keychain and restart, or set up a key again. Local .rbxl and .rbxlx files work without it."
                        } else if self.pick.is_some() {
                            "Saving and publishing to Roblox need an Open Cloud API key. Add one from Home \u{203a} Manage key and your experiences are listed here."
                        } else {
                            "Add an Open Cloud API key once and My Games fills in with your experiences. Local .rbxl and .rbxlx files work without it."
                        },
                    ))
                    .child(
                        h_flex().gap(px(8.)).mt(px(6.)).child(
                            ui::icon_button(
                                "banner-set-up",
                                "key-round",
                                if self.pick.is_some() { "Manage key" } else { "Set up a key" },
                                Weight::Primary,
                                false,
                            )
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
                    .child(dot_grid(380., 240.))
                    .child(
                        h_flex()
                            .absolute()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .gap(px(18.))
                            .child(crate::shell::chrome::logo(40.).text_color(ui::accent()))
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
        let tile = |id: ElementId, dashed: bool| {
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
                    let this = tokens::hover_fx(this);
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
        // Five tiles outgrow a narrow window: the row wraps rather than
        // pushing "Open a file…" off the edge.
        h_flex()
            .flex_wrap()
            .gap(px(GAP))
            .children(
                crate::home::Template::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, t)| {
                        tile(("new-template", index).into(), false)
                            .child(template_thumb(t, 52.))
                            .child(label(t.name(), t.blurb()))
                            .on_click(cx.listener(move |this, _, _, cx| this.new_place(t, cx)))
                    }),
            )
            .child(
                tile("new-loren-project".into(), false)
                    .child(
                        h_flex()
                            .size(px(52.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(ui::bg())
                            .items_center()
                            .justify_center()
                            .text_color(ui::accent())
                            .child(ui::icon("flame", 20.)),
                    )
                    .child(label("Loren project", "Services & Controllers, by fabi"))
                    .on_click(cx.listener(|this, _, _, cx| this.new_loren_project(cx))),
            )
            .child(
                tile("new-open-file".into(), true)
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
