//! The Figma window's drawing, in the launcher's kit: the header with the
//! sign-in button, the page, and the status line.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_figma::infer::{Confidence, CLASSES};
use rbx_figma::oauth;

use super::super::super::chrome;
use super::model::{self, ReviewRow, TreeRow};
use super::window::{Browse, FigmaWindow, Page, Review};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

const INDENT: f32 = 14.;

impl Render for FigmaWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.page {
            Page::Home => self.home_page(cx),
            Page::File(browse) => self.file_page(browse, cx),
            Page::Review(review) => self.review_page(review, cx),
        };
        v_flex()
            .id("figma-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_size(px(13.))
            .text_color(tokens::text())
            .child(chrome::window_topbar(
                "Import from Figma".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().child(body))
            .child(self.status_line())
    }
}

impl FigmaWindow {
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy > 0;
        let (crumb, back): (SharedString, bool) = match &self.page {
            Page::Home => ("Home".into(), false),
            Page::File(browse) => (
                browse
                    .outline
                    .as_ref()
                    .map_or("Loading\u{2026}".into(), |o| o.name.clone().into()),
                true,
            ),
            Page::Review(review) => (
                format!("Review \u{b7} node {}", review.link.node_id).into(),
                true,
            ),
        };
        let account = match &self.source {
            Some(super::source::Source::Fixture(_)) => {
                ui::pill("Fixture", Some("flask-conical"), false).into_any_element()
            }
            Some(_) => ui::button("figma-disconnect", "Disconnect", Weight::Ghost, true)
                .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx)))
                .into_any_element(),
            None if oauth::CLIENT_SECRET.is_none() => div().into_any_element(),
            None => ui::button("figma-connect", "Connect to Figma", Weight::Primary, true)
                .when(busy, |this| this.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !busy {
                        this.connect(cx);
                    }
                }))
                .into_any_element(),
        };
        h_flex()
            .w_full()
            .flex_none()
            .gap(px(8.))
            .px(px(16.))
            .py(px(10.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .when(back, |this| {
                this.child(
                    ui::icon_button("figma-back", "chevron-left", "Back", Weight::Ghost, true)
                        .on_click(cx.listener(|this, _, _, cx| match this.page {
                            Page::Review(_) => this.back(cx),
                            _ => this.home(cx),
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crumb),
            )
            .child(account)
    }

    fn status_line(&self) -> impl IntoElement {
        let line = h_flex()
            .w_full()
            .flex_none()
            .h(px(28.))
            .gap(px(8.))
            .px(px(16.))
            .items_center()
            .border_t_1()
            .border_color(tokens::border())
            .text_size(tokens::text_sm());
        if self.busy > 0 {
            let text = self.progress.lock().unwrap().clone();
            return line
                .text_color(tokens::text_muted())
                .child(ui::spinner("figma-busy", 12.))
                .child(div().truncate().child(text));
        }
        match &self.status {
            Some((true, text)) => line
                .text_color(tokens::text_error())
                .child(div().truncate().child(text.clone())),
            Some((false, text)) => line
                .text_color(tokens::text_muted())
                .child(div().truncate().child(text.clone())),
            None => line,
        }
    }

    fn home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = v_flex()
            .id("figma-home")
            .size_full()
            .overflow_y_scroll()
            .gap(px(14.))
            .p(px(16.));
        if self.source.is_none() {
            let text = match oauth::CLIENT_SECRET {
                None => oauth::NO_SECRET,
                Some(_) => "Connect your Figma account to browse your files. rbx-native only asks to read file content.",
            };
            page = page.child(
                div()
                    .p(px(12.))
                    .rounded(px(6.))
                    .bg(ui::wash())
                    .text_color(tokens::text2())
                    .child(text),
            );
        }
        let connected = self.source.is_some();
        page = page.child(
            h_flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&self.link).with_size(tokens::field_size())),
                )
                .child(
                    ui::button("figma-open-link", "Open", Weight::Secondary, true)
                        .when(!connected, |this| this.opacity(0.5))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if connected {
                                this.open_link(cx);
                            }
                        })),
                ),
        );
        page = page.child(
            div()
                .text_size(tokens::text_sm())
                .text_color(tokens::text3())
                .font_weight(FontWeight::SEMIBOLD)
                .child("RECENT FILES"),
        );
        if self.recent.files.is_empty() {
            return page
                .child(
                    div()
                        .text_color(tokens::text_muted())
                        .child("Files you open here show up in this list."),
                )
                .into_any_element();
        }
        let cards = self.recent.files.iter().enumerate().map(|(index, file)| {
            let key = file.key.clone();
            let thumb = match self.thumbs.get(&file.key) {
                Some(path) => img(path.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => h_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .text_color(tokens::text3())
                    .child(ui::icon("image", 24.))
                    .into_any_element(),
            };
            v_flex()
                .id(("figma-recent", index))
                .w(px(200.))
                .rounded(px(6.))
                .overflow_hidden()
                .border_1()
                .border_color(tokens::border2())
                .cursor_pointer()
                .hover(|this| this.border_color(tokens::text3()))
                .child(div().w_full().h(px(120.)).bg(ui::panel2()).child(thumb))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(8.))
                        .truncate()
                        .child(file.name.clone()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.source.is_some() {
                        this.open_file(key.clone(), cx);
                    }
                }))
        });
        page.child(h_flex().flex_wrap().gap(px(12.)).children(cards))
            .into_any_element()
    }

    fn file_page(&self, browse: &Browse, cx: &mut Context<Self>) -> AnyElement {
        let Some(outline) = &browse.outline else {
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(ui::spinner("figma-outline", 20.))
                .into_any_element();
        };
        let filter = self.filter.read(cx).value().to_string();
        let rows = model::tree_rows(&outline.pages, &browse.expanded, &filter);
        let selected = browse.selected.clone();
        let selected_name = rows
            .iter()
            .find(|r| Some(&r.id) == selected.as_ref())
            .map(|r| format!("{} \u{b7} {}", r.name, model::kind_label(&r.kind)));
        let tree = v_flex()
            .id("figma-tree")
            .flex_1()
            .overflow_y_scroll()
            .py(px(4.))
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| self.tree_row(index, row, selected.as_deref(), cx)),
            );
        let left = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .border_r_1()
            .border_color(tokens::border())
            .child(
                div()
                    .p(px(8.))
                    .child(Input::new(&self.filter).with_size(tokens::field_size())),
            )
            .child(tree);
        h_flex()
            .size_full()
            .child(left)
            .child(self.preview_pane(browse, selected_name, cx))
            .into_any_element()
    }

    fn tree_row(
        &self,
        index: usize,
        row: TreeRow,
        selected: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = row.id.clone();
        let toggle_id = row.id.clone();
        let is_selected = selected == Some(row.id.as_str());
        let arrow = div()
            .id(("figma-arrow", index))
            .size(px(16.))
            .flex_none()
            .text_color(tokens::text3())
            .when(row.expandable, |this| {
                this.child(ui::icon(
                    if row.expanded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.,
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle(toggle_id.clone(), cx);
                }))
            });
        h_flex()
            .id(("figma-row", index))
            .h(tokens::tree_row_height())
            .flex_none()
            .gap(px(6.))
            .pl(px(8. + row.depth as f32 * INDENT))
            .pr(px(8.))
            .items_center()
            .cursor_pointer()
            .map(|this| match is_selected {
                true => this.bg(tokens::selection()),
                false => this.hover(|this| this.bg(tokens::hover())),
            })
            .child(arrow)
            .child(div().flex_1().min_w_0().truncate().child(row.name))
            .child(
                div()
                    .flex_none()
                    .text_size(tokens::text_sm())
                    .text_color(if is_selected {
                        tokens::text2()
                    } else {
                        tokens::text3()
                    })
                    .child(model::kind_label(&row.kind)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
    }

    fn preview_pane(
        &self,
        browse: &Browse,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pane = v_flex()
            .w(px(320.))
            .flex_none()
            .h_full()
            .gap(px(12.))
            .p(px(16.));
        let Some(id) = &browse.selected else {
            return pane.child(
                div()
                    .text_color(tokens::text_muted())
                    .child("Select a layer to preview it."),
            );
        };
        let picture = match &browse.preview {
            Some(path) => img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(ui::spinner("figma-preview", 18.))
                .into_any_element(),
        };
        pane.child(
            div()
                .w_full()
                .h(px(220.))
                .p(px(8.))
                .rounded(px(6.))
                .bg(ui::panel2())
                .child(picture),
        )
        .child(
            v_flex()
                .gap(px(2.))
                .children(name.map(|name| {
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name)
                }))
                .child(
                    div()
                        .text_size(tokens::text_sm())
                        .text_color(tokens::text3())
                        .child(format!("Node {id}")),
                ),
        )
        .child(
            ui::button("figma-review", "Review import", Weight::Primary, true)
                .on_click(cx.listener(|this, _, _, cx| this.review_selected(cx))),
        )
    }

    fn review_page(&self, review: &Review, cx: &mut Context<Self>) -> AnyElement {
        let Some(tree) = &review.tree else {
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(ui::spinner("figma-prepare", 20.))
                .into_any_element();
        };
        let applied = model::apply(tree.clone(), &review.edits);
        let rows = model::review_rows(&applied, &review.edits);
        let low = rows
            .iter()
            .filter(|r| r.confidence == Confidence::Low)
            .count();
        let busy = self.busy > 0;
        let summary = match low {
            0 => format!("{} layers", rows.len()),
            n => format!("{} layers \u{b7} {n} to check", rows.len()),
        };
        let table = v_flex()
            .id("figma-review-rows")
            .flex_1()
            .overflow_y_scroll()
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| self.review_row(index, row, review, cx)),
            );
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px(px(16.))
                    .h(px(28.))
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text3())
                    .child(div().flex_1().child("FIGMA LAYER"))
                    .child(div().w(px(150.)).child("CLASS"))
                    .child(div().w(px(70.)).child("CONFIDENCE"))
                    .child(div().w(px(110.)).child("")),
            )
            .child(table)
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(10.))
                    .items_center()
                    .border_t_1()
                    .border_color(tokens::border())
                    .child(
                        div()
                            .flex_1()
                            .text_color(tokens::text_muted())
                            .child(summary),
                    )
                    .child(
                        ui::button("figma-cancel", "Back", Weight::Secondary, false)
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    )
                    .child(
                        ui::button("figma-import", "Import", Weight::Primary, false)
                            .when(busy, |this| this.opacity(0.5))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.import(cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn review_row(
        &self,
        index: usize,
        row: ReviewRow,
        review: &Review,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let low = row.confidence == Confidence::Low;
        let menu_open = review.menu.as_deref() == Some(row.id.as_str());
        let (confidence, fg, fill) = match row.confidence {
            Confidence::Low => ("LOW", ui::red(), ui::red_soft()),
            Confidence::Medium => ("MEDIUM", tokens::warning(), ui::wash()),
            Confidence::High => ("HIGH", ui::green(), ui::green_soft()),
        };
        let menu_id = row.id.clone();
        let flat_id = row.id.clone();
        let class_button = div()
            .relative()
            .w(px(150.))
            .flex_none()
            .child(
                h_flex()
                    .id(("figma-class", index))
                    .h(px(24.))
                    .px(px(8.))
                    .gap(px(4.))
                    .items_center()
                    .rounded(px(4.))
                    .bg(ui::panel2())
                    .border_1()
                    .border_color(tokens::border2())
                    .cursor_pointer()
                    .when(row.flattened, |this| this.opacity(0.5))
                    .child(div().flex_1().truncate().child(row.class))
                    .child(ui::icon("chevron-down", 12.))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_menu(menu_id.clone(), cx)),
                    ),
            )
            .when(menu_open, |this| {
                let current = row.class;
                let id = row.id.clone();
                this.child(deferred(
                    anchored()
                        .position_mode(AnchoredPositionMode::Local)
                        .position(point(px(0.), px(28.)))
                        .child(
                            v_flex()
                                .id("figma-class-menu")
                                .occlude()
                                .w(px(150.))
                                .p(px(4.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(tokens::border2())
                                .bg(tokens::dock())
                                .shadow_md()
                                .children(CLASSES.iter().enumerate().map(|(n, &class)| {
                                    let id = id.clone();
                                    h_flex()
                                        .id(("figma-class-pick", n))
                                        .h(px(24.))
                                        .px(px(8.))
                                        .items_center()
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .when(class == current, |this| this.bg(tokens::selection()))
                                        .hover(|this| this.bg(tokens::hover()))
                                        .child(class)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.pick_class(id.clone(), class, cx)
                                        }))
                                })),
                        ),
                ))
            });
        let notes =
            row.notes.clone().into_iter().chain(
                (!row.modifiers.is_empty()).then(|| format!("+ {}", row.modifiers.join(", "))),
            );
        h_flex()
            .id(("figma-review-row", index))
            .w_full()
            .min_h(px(36.))
            .px(px(16.))
            .py(px(4.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .when(low, |this| this.bg(ui::red_soft()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .pl(px(row.depth as f32 * INDENT))
                    .child(div().truncate().child(row.name.clone()))
                    .children(notes.map(|note| {
                        div()
                            .truncate()
                            .text_size(tokens::text_sm())
                            .text_color(tokens::text_muted())
                            .child(note)
                    })),
            )
            .child(class_button)
            .child(
                div()
                    .w(px(70.))
                    .flex_none()
                    .child(ui::tag(confidence, fg, fill)),
            )
            .child(
                ui::button(
                    ("figma-flat", index),
                    if row.flattened {
                        "Image \u{2713}"
                    } else {
                        "Flatten to image"
                    },
                    if row.flattened {
                        Weight::Secondary
                    } else {
                        Weight::Ghost
                    },
                    true,
                )
                .w(px(110.))
                .flex_none()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_flat(flat_id.clone(), cx))),
            )
    }
}
