//! My Games: the sections and cards, loading and empty states, the
//! partial-listing and group notes, and the add-by-link row.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rbx_cloud::Experience;

use super::*;
use crate::launcher::home_window::{Games, HomeWindow};
use crate::launcher::ui::{self};
use crate::tokens;

impl HomeWindow {
    pub(super) fn games_page(&mut self, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
        let title = if self.pick.is_some() {
            "Pick a game"
        } else {
            "My Games"
        };
        if !self.has_key() {
            return v_flex()
                .gap(px(20.))
                .child(self.page_head(title, grid, false, cx))
                .child(self.no_key_banner(cx))
                .into_any_element();
        }
        let head = self.page_head(title, grid, true, cx);
        let body: AnyElement = match &self.games {
            Games::Loading => v_flex()
                .gap(px(12.))
                .child(
                    h_flex()
                        .h(px(22.))
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("house", 12.).text_color(tokens::text3()))
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
            Games::Loaded(_) => {
                let query = self.search.read(cx).value().to_lowercase();
                let games: Vec<Experience> = self
                    .owner_games()
                    .into_iter()
                    .filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query))
                    .collect();
                let group = self.owner;
                let fetching = group.is_some() && self.group_loading == group;
                let busy = if group.is_some() {
                    fetching
                } else {
                    self.refreshing
                };
                let (glyph, title): (&'static str, SharedString) = match group {
                    None => ("house", "Personal".into()),
                    Some(id) => (
                        "users",
                        self.owner_options
                            .iter()
                            .find(|(o, _)| *o == Some(id))
                            .map(|(_, name)| name.clone())
                            .unwrap_or_else(|| format!("Group {id}").into()),
                    ),
                };
                let section: AnyElement = if !games.is_empty() {
                    self.games_section(glyph, title, &games, busy, grid, cx)
                        .into_any_element()
                } else if fetching {
                    v_flex()
                        .gap(px(12.))
                        .child(self.section_head_busy(glyph, title, true))
                        .child(self.skeleton_row(grid, grid.columns))
                        .into_any_element()
                } else if group.is_some() || !query.is_empty() {
                    v_flex()
                        .gap(px(12.))
                        .child(self.section_head_busy(glyph, title, false))
                        .child(ui::text(12.5, 19.).text_color(tokens::text2()).child(
                            if query.is_empty() {
                                "This group has no public experiences."
                            } else {
                                "No experience matches."
                            },
                        ))
                        .into_any_element()
                } else {
                    return v_flex()
                        .gap(px(20.))
                        .child(head)
                        .child(self.empty_state(
                            "No experiences to show",
                            "Your key sees every experience you own, public and private. Publish a new place, or add one you can edit by its place ID or link.".to_string(),
                            true,
                            cx,
                        ))
                        .into_any_element();
                };
                v_flex()
                    .gap(px(20.))
                    .when(self.pick.is_some(), |this| {
                        this.child(starting_place_note())
                    })
                    .when(self.note_visible(), |this| {
                        this.child(self.partial_note(cx))
                    })
                    .when(!self.note_visible() && self.link_open, |this| {
                        this.child(self.link_row(cx))
                    })
                    .child(section)
                    .when(group.is_none() && self.groups_off(), |this| {
                        this.child(self.groups_off_note(None, cx))
                    })
                    .into_any_element()
            }
        };
        v_flex()
            .gap(px(20.))
            .child(head)
            .child(body)
            .into_any_element()
    }

    pub(super) fn games_section(
        &self,
        glyph: &'static str,
        title: SharedString,
        games: &[Experience],
        busy: bool,
        grid: &Grid,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap(px(12.))
            .child(
                self.section_head_busy(glyph, title, busy).child(
                    ui::mono(11., 15.)
                        .text_color(tokens::text3())
                        .child(games.len().to_string()),
                ),
            )
            .child(self.card_row(games, grid, cx))
    }

    /// A section's glyph and title, and "Updating…" while a cached list is
    /// being refreshed from Roblox.
    pub(super) fn section_head_busy(
        &self,
        glyph: &'static str,
        title: SharedString,
        busy: bool,
    ) -> Div {
        h_flex()
            .h(px(22.))
            .items_center()
            .gap(px(8.))
            .child(ui::icon(glyph, 12.).text_color(tokens::text3()))
            .child(Self::section_title(title))
            .when(busy, |this| {
                this.child(
                    h_flex()
                        .ml(px(4.))
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .line_height(px(16.))
                        .text_color(tokens::text3())
                        .child(ui::spinner("games-updating", 12.))
                        .child("Updating\u{2026}"),
                )
            })
    }

    pub(super) fn card_row(
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

    pub(super) fn card(
        &self,
        game: &Experience,
        grid: &Grid,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let thumb = grid.card - 22.;
        let open = game.clone();
        let whole = game.clone();
        let universe = game.universe_id;
        let (visibility, glyph) = visibility_label(&game.visibility);
        // Offline, a game opens only from the copy on this computer; the
        // picker hands the game over and downloads nothing.
        let offline = crate::offline::blocked() && self.pick.is_none();
        let cut_off = offline && crate::home::local_copy(game).is_none();
        v_flex()
            .id(SharedString::from(format!("game-{}", game.universe_id)))
            .w(px(grid.card))
            .gap(px(8.))
            .p(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .map(|this| match cut_off {
                true => this.opacity(0.5).cursor_not_allowed().tooltip(|window, cx| {
                    crate::shell::tooltip::text(crate::offline::TIP, window, cx)
                }),
                false => this.cursor_pointer().hover(|this| {
                    tokens::hover_fx(this).border_color(tokens::border2()).bg(tokens::secondary_hover())
                }),
            })
            .child(self.icon_box(game, thumb, 6.))
            .child(
                h_flex()
                    .h(px(24.))
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(ui::pill(visibility, Some(glyph), false))
                            .when(self.is_archived(universe), |this| {
                                this.child(super::recent::archived_tag())
                            }),
                    )
                    .child(h_flex().gap(px(2.)).child(
                        h_flex()
                            .id(SharedString::from(format!("game-all-{universe}")))
                            .size(px(24.))
                            .rounded(px(5.))
                            .items_center()
                            .justify_center()
                            .text_color(tokens::text3())
                            .child(ui::icon("layers", 12.))
                            // Always a download: every place, fresh.
                            .map(|this| match offline {
                                true => this.opacity(0.5).cursor_not_allowed().tooltip(|window, cx| {
                                    crate::shell::tooltip::text(crate::offline::TIP, window, cx)
                                }),
                                false => this
                                    .hover(|this| tokens::hover_fx(this).bg(ui::wash()).text_color(tokens::text()))
                                    .tooltip(|window, cx| {
                                        crate::shell::tooltip::text("Open every place as one game", window, cx)
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.open_whole_game(whole.clone(), cx);
                                    })),
                            }),
                    )
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("game-more-{universe}")))
                            .size(px(24.))
                            .rounded(px(5.))
                            .items_center()
                            .justify_center()
                            .text_color(tokens::text3())
                            .hover(|this| tokens::hover_fx(this).bg(ui::wash()).text_color(tokens::text()))
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text("Open on the Creator Dashboard", window, cx)
                            })
                            .child(ui::icon("external-link", 12.))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                cx.open_url(&format!(
                                    "https://create.roblox.com/dashboard/creations/experiences/{universe}/overview"
                                ))
                            }),
                    )),
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
            .when(!cut_off, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| this.open_experience(open.clone(), cx)))
            })
    }

    /// The experience's icon, or the "No icon yet" placeholder.
    pub(super) fn icon_box(&self, game: &Experience, size: f32, radius: f32) -> impl IntoElement {
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

    pub(super) fn skeleton_row(&self, grid: &Grid, count: usize) -> impl IntoElement {
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
}

/// Roblox's listings only carry an experience's starting place, so that is
/// the one a card links to; another place is reached by its own ID.
fn starting_place_note() -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .text_size(px(12.))
        .line_height(px(17.))
        .text_color(tokens::text2())
        .child(ui::icon("info", 14.).text_color(tokens::text3()))
        .child(
            "A game links this file to its starting place. To link another place of the same experience, add it by its place ID or link.",
        )
}
