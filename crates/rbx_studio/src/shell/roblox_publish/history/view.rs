//! The window's drawing: the place header, the contributor filter, the
//! version rows and the empty/loading/failed states, in the launcher's own
//! kit so it reads like the publish confirmation (see `dialogs`).

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rbx_cloud::PlaceVersion;

use crate::launcher::ui::{self, Weight};

use super::super::super::chrome;
use super::model::{date_label, Since};
use super::{tokens, HistoryWindow, Listing};

impl Render for HistoryWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match (&self.link, &self.listing) {
            (None, _) => self.unlinked(cx),
            (Some(_), Listing::Loading) => state(
                ui::spinner("history-loading", 20.),
                "Loading versions\u{2026}",
                None,
                None,
            ),
            (Some(_), Listing::Failed(message)) => state(
                ui::dialog_glyph("cloud-off", ui::red(), ui::red_soft()),
                "Couldn\u{2019}t list this place\u{2019}s versions",
                Some(message.clone()),
                Some(
                    ui::button("history-retry", "Try again", Weight::Secondary, false)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.reload(this.history.filter, cx)),
                        )
                        .into_any_element(),
                ),
            ),
            (Some(_), Listing::Ready) => self.list(cx),
        };
        v_flex()
            .id("history-window")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_size(px(13.))
            .text_color(tokens::text())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.handle_key(&event.keystroke, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(chrome::window_topbar(
                "Version History".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .children(self.header(cx))
            .children(self.status())
            .child(div().flex_1().min_h_0().child(body))
            .children(self.dialog(cx))
    }
}

impl HistoryWindow {
    fn header(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let link = self.link.as_ref()?;
        let name = link
            .name
            .clone()
            .unwrap_or_else(|| format!("Universe {}", link.universe_id));
        let filter = self.history.filter;
        let mut pills = vec![(None, "Everyone".to_string())];
        pills.extend(
            self.history
                .contributors
                .iter()
                .map(|user| (Some(*user), self.history.name(*user))),
        );
        Some(
            v_flex()
                .flex_none()
                .gap(px(10.))
                .px(px(16.))
                .py(px(12.))
                .border_b_1()
                .border_color(tokens::border())
                .child(
                    h_flex()
                        .gap(px(12.))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    ui::text(15., 21.)
                                        .font_weight(FontWeight::BOLD)
                                        .truncate()
                                        .child(name),
                                )
                                .child(
                                    ui::text(12., 18.)
                                        .text_color(tokens::text2())
                                        .child(format!("Place {}", link.place_id)),
                                ),
                        )
                        .child(
                            ui::icon_button(
                                "history-link",
                                "link",
                                "Change place\u{2026}",
                                Weight::Ghost,
                                true,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.link_place(cx))),
                        )
                        .child(
                            ui::icon_button(
                                "history-refresh",
                                "refresh-cw",
                                "Refresh",
                                Weight::Secondary,
                                true,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.reload(this.history.filter, cx)),
                            ),
                        ),
                )
                .child(
                    ui::field_frame(None, ui::panel2(), tokens::border(), "search").child(
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
                .child(
                    h_flex()
                        .gap(px(12.))
                        .items_start()
                        .child(
                            // One saver is no filter worth offering.
                            h_flex().flex_1().min_w_0().flex_wrap().gap(px(6.)).when(
                                pills.len() > 2,
                                |this| {
                                    this.children(pills.into_iter().enumerate().map(
                                        |(i, (user, label))| {
                                            div()
                                                .id(("history-contributor", i))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.reload(user, cx)
                                                }))
                                                .child(ui::pill(label, None, user == filter))
                                        },
                                    ))
                                },
                            ),
                        )
                        .child(h_flex().flex_none().gap(px(6.)).children(
                            Since::ALL.into_iter().enumerate().map(|(i, since)| {
                                div()
                                    .id(("history-since", i))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.since = since;
                                        cx.notify();
                                    }))
                                    .child(ui::pill(
                                        since.label(),
                                        (i == 0).then_some("calendar"),
                                        since == self.since,
                                    ))
                            }),
                        )),
                )
                .into_any_element(),
        )
    }

    /// The running action, or how the last one went.
    fn status(&self) -> Option<AnyElement> {
        let (glyph, color, text) = match (&self.busy, &self.notice) {
            (Some(what), _) => (
                ui::spinner("history-busy", 13.),
                tokens::text2(),
                what.clone(),
            ),
            (None, Some(notice)) if notice.ok => (
                ui::icon("circle-check", 13.).into_any_element(),
                ui::green(),
                notice.text.clone(),
            ),
            (None, Some(notice)) => (
                ui::icon("circle-alert", 13.).into_any_element(),
                ui::red(),
                notice.text.clone(),
            ),
            (None, None) => return None,
        };
        Some(
            h_flex()
                .flex_none()
                .gap(px(8.))
                .px(px(16.))
                .py(px(8.))
                .border_b_1()
                .border_color(tokens::border())
                .bg(ui::panel2())
                .text_color(color)
                .child(glyph)
                .child(
                    ui::text(12., 18.)
                        .flex_1()
                        .min_w_0()
                        .text_color(tokens::text())
                        .child(text),
                )
                .into_any_element(),
        )
    }

    fn unlinked(&self, cx: &mut Context<Self>) -> AnyElement {
        state(
            ui::dialog_glyph("link", ui::accent(), ui::wash()),
            "This file isn\u{2019}t linked to a Roblox place",
            Some("Link it to see the place\u{2019}s saved and published versions.".into()),
            Some(
                ui::button(
                    "history-link-first",
                    "Link to Roblox Place\u{2026}",
                    Weight::Primary,
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.link_place(cx)))
                .into_any_element(),
            ),
        )
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.history.versions.is_empty() {
            return state(
                ui::dialog_glyph("clock", tokens::text2(), ui::wash()),
                "No versions",
                Some("Nothing matches this filter.".into()),
                None,
            );
        }
        let query = self.search.read(cx).value();
        let query = query.trim();
        let now = chrono::Utc::now();
        let shown: Vec<&PlaceVersion> = self
            .history
            .versions
            .iter()
            .filter(|v| {
                let label = self.labels.get(&v.version).map(String::as_str);
                self.since.holds(v, now) && self.history.matches(v, label, query)
            })
            .collect();
        let more_pages = self.history.next_cursor.is_some();
        // A search only sees the pages already loaded: say how many, and
        // keep Load more under it to reach further back.
        let narrowed = !query.is_empty() || self.since != Since::AllTime;
        let searched = narrowed.then(|| {
            let loaded = self.history.versions.len();
            let older = if more_pages {
                ", older ones aren\u{2019}t loaded yet"
            } else {
                ""
            };
            ui::text(12., 18.)
                .px(px(16.))
                .py(px(10.))
                .text_color(tokens::text2())
                .child(format!(
                    "{} of {loaded} loaded versions match{older}",
                    shown.len()
                ))
        });
        let more = more_pages.then(|| {
            h_flex()
                .justify_center()
                .py(px(14.))
                .child(if self.loading_more {
                    ui::spinner("history-more", 16.)
                } else {
                    ui::button("history-more", "Load more", Weight::Secondary, true)
                        .on_click(cx.listener(|this, _, _, cx| this.load_more(cx)))
                        .into_any_element()
                })
        });
        if shown.is_empty() && !more_pages {
            return state(
                ui::dialog_glyph("search", tokens::text2(), ui::wash()),
                "Nothing matches your filters",
                Some("Try adjusting or clearing your filters.".into()),
                Some(
                    ui::button("history-clear", "Clear filters", Weight::Secondary, false)
                        .on_click(cx.listener(|this, _, window, cx| this.clear_filters(window, cx)))
                        .into_any_element(),
                ),
            );
        }
        let latest = self.history.latest();
        v_flex()
            .id("history-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(
                shown
                    .into_iter()
                    .map(|version| self.row(version, Some(version.version) == latest, cx)),
            )
            .children(searched)
            .children(more)
            .into_any_element()
    }

    fn row(&self, version: &PlaceVersion, latest: bool, cx: &mut Context<Self>) -> AnyElement {
        let n = version.version;
        let label = self.labels.get(&n).cloned();
        let editing = self.editing == Some(n);
        let title = version.title.clone().unwrap_or_else(|| {
            if version.is_published {
                "Published".into()
            } else {
                "Saved".into()
            }
        });
        let mut meta = date_label(&version.created_time);
        if let Some(author) = self.history.author(version) {
            meta = format!("{meta} \u{b7} {author}");
        }
        h_flex()
            .id(("history-row", n as usize))
            .gap(px(12.))
            .px(px(16.))
            .py(px(9.))
            .border_b_1()
            .border_color(tokens::border())
            .hover(|this| this.bg(ui::wash()))
            .child(
                ui::mono(12., 18.)
                    .w(px(56.))
                    .flex_none()
                    .text_color(tokens::text2())
                    .child(format!("v{n}")),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            // Pinned to the title's line height so the label
                            // field, taller than the text, doesn't grow the row
                            // while it's being typed into.
                            .h(px(19.))
                            .gap(px(6.))
                            .when(editing, |this| {
                                this.child(
                                    ui::field_frame(
                                        None,
                                        ui::panel2(),
                                        tokens::accent_line(),
                                        "tag",
                                    )
                                    .h(px(21.))
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&self.label_input)
                                                .appearance(false)
                                                .px_0()
                                                .text_size(px(12.5))
                                                .line_height(px(17.))
                                                .text_color(tokens::text()),
                                        ),
                                    ),
                                )
                            })
                            .when(!editing, |this| {
                                this.when_some(label.clone(), |this, label| {
                                    this.child(
                                        ui::icon("tag", 12.).flex_none().text_color(ui::accent()),
                                    )
                                    .child(
                                        ui::text(13., 19.)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .truncate()
                                            .child(label),
                                    )
                                })
                                .child(
                                    ui::text(13., 19.)
                                        .when(label.is_none(), |this| {
                                            this.font_weight(FontWeight::SEMIBOLD)
                                        })
                                        .when(label.is_some(), |this| {
                                            this.text_color(tokens::text2())
                                        })
                                        .flex_none()
                                        .child(title),
                                )
                            })
                            .when(version.is_published, |this| {
                                this.child(ui::tag(
                                    "PUBLISHED",
                                    ui::accent(),
                                    tokens::accent_soft(),
                                ))
                            })
                            .when(latest, |this| {
                                this.child(ui::tag("LATEST", ui::green(), ui::green_soft()))
                            }),
                    )
                    .child(
                        ui::text(12., 18.)
                            .text_color(tokens::text2())
                            .truncate()
                            .child(meta),
                    )
                    .children(version.description.clone().map(|notes| {
                        ui::text(12., 18.)
                            .text_color(tokens::text3())
                            .truncate()
                            .child(notes)
                    })),
            )
            .child(
                ui::icon_button(
                    ("history-label", n as usize),
                    "tag",
                    if label.is_some() {
                        "Rename label"
                    } else {
                        "Label this version"
                    },
                    Weight::Ghost,
                    true,
                )
                .on_click(cx.listener(move |this, _, window, cx| this.edit_label(n, window, cx))),
            )
            .child(
                ui::icon_button(
                    ("history-open", n as usize),
                    "folder-open",
                    "Open",
                    Weight::Ghost,
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_version(n, cx))),
            )
            // Restoring the newest version would only copy it.
            .when(!latest, |this| {
                this.child(
                    ui::button(
                        ("history-restore", n as usize),
                        "Restore\u{2026}",
                        Weight::Secondary,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.ask_restore(n, cx))),
                )
            })
            .into_any_element()
    }
}

/// A centred glyph, line, explanation and action: unlinked, loading,
/// failed, empty.
fn state(
    glyph: AnyElement,
    title: &'static str,
    text: Option<String>,
    action: Option<AnyElement>,
) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .px(px(40.))
        .child(glyph)
        .child(
            ui::text(14., 20.)
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .children(text.map(|text| {
            ui::text(12.5, 19.)
                .max_w(px(460.))
                .text_center()
                .text_color(tokens::text2())
                .child(text)
        }))
        .children(action.map(|action| div().mt(px(6.)).child(action)))
        .into_any_element()
}
