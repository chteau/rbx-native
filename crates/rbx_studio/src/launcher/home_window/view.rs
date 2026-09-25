//! Home's pages, sidebar and dialogs, one file each; the layout grid and
//! the small drawing helpers they share live here.

mod dialogs;
mod games;
mod notes;
mod pages;
mod recent;
mod sidebar;

use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone};
use gpui_kit::component::select::SearchableVec;
use gpui_kit::component::IndexPath;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_cloud::Visibility;

use super::{file_name, HomeWindow, Page};
use crate::home::RecentPlace;
use crate::launcher::ui::{self};
use crate::tokens;

pub(super) const SIDEBAR: f32 = 232.;
pub(super) const GAP: f32 = 16.;

/// The width the page's content gets, and the My Games grid over it:
/// `clamp(floor((w + 16) / 192), 3, 8)` columns.
pub(super) struct Grid {
    pub(super) content: f32,
    pub(super) columns: usize,
    pub(super) card: f32,
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
        if self.owner_options_dirty {
            self.owner_options_dirty = false;
            let labels: Vec<SharedString> = self
                .owner_options
                .iter()
                .map(|(_, label)| label.clone())
                .collect();
            let selected = self
                .owner_options
                .iter()
                .position(|(owner, _)| *owner == self.owner)
                .unwrap_or(0);
            self.owner_select.update(cx, |select, cx| {
                select.set_items(SearchableVec::new(labels), window, cx);
                select.set_selected_index(Some(IndexPath::new(selected)), window, cx);
            });
        }
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
            .child(crate::shell::chrome::window_topbar(
                crumb.into(),
                true,
                |_, cx| cx.quit(),
            ))
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
}

/// A 30 % accent segment sliding along a 6 px track (still under reduced
/// motion). The download is one blocking request with no running total,
/// so a determinate bar would have nothing to show.
pub(super) fn indeterminate_bar() -> impl IntoElement {
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

/// The hero's right pane: dots at a 12 px pitch in text3 at 25 %.
pub(super) fn dot_grid(width: f32, height: f32) -> impl IntoElement {
    let dot = Rgba {
        a: 0.25,
        ..tokens::text3()
    };
    v_flex()
        .absolute()
        .top(px(5.))
        .left(px(5.))
        .gap(px(10.))
        .children((0..(height / 12.) as usize).map(move |_| {
            h_flex().gap(px(10.)).children(
                (0..(width / 12.) as usize).map(move |_| div().size(px(2.)).rounded_full().bg(dot)),
            )
        }))
}

/// Studio's own Baseplate thumbnail, drawn: sky over a grey chequered
/// floor.
pub(super) fn baseplate_thumb(size: f32) -> impl IntoElement {
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

pub(super) fn visibility_label(visibility: &Visibility) -> (&'static str, &'static str) {
    match visibility {
        Visibility::Public => ("Public", "globe"),
        Visibility::Private => ("Private", "lock"),
        Visibility::Other(_) => ("Friends", "users"),
    }
}

pub(super) fn recent_name(place: &RecentPlace) -> String {
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

pub(super) fn display_dir(path: &std::path::Path) -> String {
    display_path(path.parent().unwrap_or(path))
}

/// `Today, 1:05 AM`, `Yesterday, 6:46 PM`, `Sep 17, 1:23 PM`.
pub(super) fn opened_label(secs: i64) -> String {
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
