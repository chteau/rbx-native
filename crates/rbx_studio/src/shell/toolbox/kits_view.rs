//! The Free kits source's header, pack and model cards, drawn like the
//! Creator Store's results.

use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Load;
use crate::kits::{self, KitModel, Pack, Thumb};
use crate::launcher::ui::{self, Weight};
use crate::shell::Shell;
use crate::tokens;

/// How long the cards take to rise in when a list or pack opens.
const ENTRANCE: Duration = Duration::from_millis(220);

impl Shell {
    pub(super) fn kits_body(
        &self,
        sources: Div,
        field: Option<Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let browser = &self.toolbox.kits;
        let filter = self
            .toolbox
            .query
            .as_ref()
            .map(|query| query.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
        let open = browser.open_pack();

        let header = match open {
            Some(pack) => self.kits_pack_header(pack, cx),
            None => h_flex()
                .w_full()
                .gap(px(8.))
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(tokens::text_xs())
                        .line_height(tokens::line_xs())
                        .text_color(tokens::text2())
                        .child(match browser.load {
                            Load::Ready => format!(
                                "{} CC0 packs from Poly Haven and the packs you add",
                                browser.packs.len()
                            ),
                            _ => "CC0 packs from Poly Haven and the packs you add".to_owned(),
                        }),
                )
                .child(
                    ui::icon_button(
                        "kits-add-pack",
                        "folder-plus",
                        "Add pack",
                        Weight::Ghost,
                        true,
                    )
                    .flex_none()
                    .on_click(cx.listener(|shell, _, _, cx| shell.kits_choose_zip(cx))),
                )
                .into_any_element(),
        };

        let status = browser.status.clone().map(|status| {
            let (glyph, color, text) = match status {
                Ok(text) => ("circle-check", tokens::text2(), text),
                Err(text) => ("circle-alert", tokens::text_error(), text),
            };
            h_flex()
                .gap(px(6.))
                .items_start()
                .text_color(color)
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .child(div().pt(px(1.)).child(ui::icon(glyph, 12.)))
                .child(div().flex_1().min_w_0().whitespace_normal().child(text))
        });
        let offline = browser
            .offline
            .clone()
            .filter(|_| browser.load == Load::Ready && open.is_none())
            .map(|err| {
                div()
                    .whitespace_normal()
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(tokens::warning())
                    .child(format!("Poly Haven\u{2019}s packs are missing: {err}"))
            });

        let cards: Vec<AnyElement> = match open {
            Some(pack) => pack
                .models
                .iter()
                .filter(|model| matches(&model.name))
                .map(|model| self.kits_model_card(model, cx))
                .collect(),
            None => browser
                .packs
                .iter()
                .filter(|pack| matches(&pack.name) || pack.models.iter().any(|m| matches(&m.name)))
                .map(|pack| self.kits_pack_card(pack, cx))
                .collect(),
        };
        let results = match &browser.load {
            Load::Loading | Load::Idle => centred(
                h_flex()
                    .gap(px(8.))
                    .text_color(tokens::text2())
                    .child(ui::spinner("kits-loading", 14.))
                    .child("Loading free kits"),
            ),
            Load::Failed(err) => centred(
                div()
                    .text_color(tokens::text_error())
                    .child(super::view::unreachable("Poly Haven", err)),
            ),
            Load::Ready if cards.is_empty() => centred(
                div()
                    .text_color(tokens::text2())
                    .child(format!("Nothing matches \u{201c}{filter}\u{201d}.")),
            ),
            Load::Ready => {
                let key = open.map_or("packs", |pack| pack.id.as_str()).to_owned();
                v_flex()
                    .id("kits-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .grid()
                            .grid_cols(2)
                            .gap(px(8.))
                            .children(cards)
                            .with_animation(
                                SharedString::from(format!("kits-enter-{key}")),
                                Animation::new(ENTRANCE).with_easing(tokens::easing_soft),
                                |this, delta| this.opacity(delta).mt(px(8. * (1. - delta))),
                            ),
                    )
                    .into_any_element()
            }
        };

        v_flex()
            .size_full()
            .p(px(10.))
            .gap(px(10.))
            .child(sources)
            .children(field)
            .child(header)
            .children(status)
            .children(offline)
            .child(results)
            .into_any_element()
    }

    fn kits_pack_header(&self, pack: &Pack, cx: &mut Context<Self>) -> AnyElement {
        let browser = &self.toolbox.kits;
        let picked = browser.selected.len();
        let busy = !browser.busy.is_empty();
        let action = if picked > 0 {
            Some((format!("Import {picked}"), browser.selected.clone()))
        } else if pack.models.len() <= kits::BATCH_LIMIT {
            let all = pack.models.iter().map(|m| m.id.clone()).collect();
            Some(("Import all".to_owned(), all))
        } else {
            None
        };
        h_flex()
            .w_full()
            .gap(px(8.))
            .items_center()
            .child(
                div()
                    .id("kits-back")
                    .flex_none()
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::radius_tiny())
                    .cursor_pointer()
                    .text_color(tokens::text2())
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .on_click(cx.listener(|shell, _, window, cx| shell.kits_back(window, cx)))
                    .child(ui::icon("chevron-left", 16.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child(pack.name.clone()),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(tokens::text_xs())
                            .line_height(tokens::line_xs())
                            .text_color(tokens::text3())
                            .child(match action {
                                // The pack's source stays named while its
                                // cards name each model's artist.
                                Some(_) => format!(
                                    "{} models \u{b7} {} \u{b7} CC0",
                                    pack.models.len(),
                                    pack.author
                                ),
                                None => format!(
                                    "{} models \u{b7} {} \u{b7} CC0 \u{b7} tick up to {}",
                                    pack.models.len(),
                                    pack.author,
                                    kits::BATCH_LIMIT
                                ),
                            }),
                    ),
            )
            .when_some(action, |this, (label, ids)| {
                this.child(if busy {
                    ui::disabled_button("kits-import", "Importing\u{2026}")
                        .h(px(ui::button_height(true)))
                        .into_any_element()
                } else {
                    ui::icon_button("kits-import", "download", label, Weight::Primary, true)
                        .flex_none()
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.kits_import(ids.clone(), cx);
                        }))
                        .into_any_element()
                })
            })
            .into_any_element()
    }

    fn kits_pack_card(&self, pack: &Pack, cx: &mut Context<Self>) -> AnyElement {
        let id = pack.id.clone();
        card(
            format!("kits-pack-{}", pack.id),
            self.kits_thumbnail(pack.thumb(), "package", None),
            pack.name.clone(),
            format!("{} models \u{b7} {}", pack.models.len(), pack.author),
        )
        .on_click(cx.listener(move |shell, _, window, cx| shell.kits_open(id.clone(), window, cx)))
        .into_any_element()
    }

    fn kits_model_card(&self, model: &KitModel, cx: &mut Context<Self>) -> AnyElement {
        let browser = &self.toolbox.kits;
        let picked = browser.selected.contains(&model.id);
        let busy = browser.busy.contains(&model.id);
        let toggle_id = model.id.clone();
        let check = div()
            .id(SharedString::from(format!("kits-pick-{}", model.id)))
            .absolute()
            .top(px(6.))
            .left(px(6.))
            .size(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(if picked {
                ui::accent()
            } else {
                tokens::text3()
            })
            .bg(if picked {
                ui::accent()
            } else {
                tokens::field_select()
            })
            .text_color(tokens::text())
            .cursor_pointer()
            .hover(|this| tokens::hover_fx(this).border_color(ui::accent()))
            .on_click(cx.listener(move |shell, _, _, cx| {
                cx.stop_propagation();
                shell.kits_toggle(toggle_id.clone(), cx);
            }))
            .when(picked, |this| this.child(ui::icon("check", 12.)));
        let id = model.id.clone();
        card(
            format!("kits-model-{}", model.id),
            self.kits_thumbnail(&model.thumb, "box", Some(busy)),
            model.name.clone(),
            format!("{} \u{b7} CC0", model.author),
        )
        .relative()
        .when(picked, |this| this.border_color(ui::accent()))
        .child(check)
        .on_click(cx.listener(move |shell, _, _, cx| shell.kits_import(vec![id.clone()], cx)))
        .into_any_element()
    }

    fn kits_thumbnail(&self, thumb: &Thumb, glyph: &'static str, busy: Option<bool>) -> Div {
        let picture = match self.toolbox.kits.thumbs.get(thumb) {
            Some(Some(image)) => img(image.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            _ => div()
                .text_color(tokens::text3())
                .child(ui::icon(glyph, 20.))
                .into_any_element(),
        };
        div()
            .relative()
            .w_full()
            .h(px(96.))
            .rounded(tokens::radius_tiny())
            .overflow_hidden()
            .bg(tokens::tile())
            .flex()
            .items_center()
            .justify_center()
            .child(picture)
            .when(busy == Some(true), |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(tokens::field_select())
                        .opacity(0.85)
                        .child(ui::spinner("kits-busy", 16.)),
                )
            })
    }
}

/// A result card: picture, a name and a line under it, as the Creator
/// Store's cards are drawn.
fn card(id: String, thumbnail: Div, name: String, line: String) -> Stateful<Div> {
    v_flex()
        .id(SharedString::from(id))
        .p(px(6.))
        .gap(px(6.))
        .rounded(tokens::radius_tile())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .cursor_pointer()
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .child(thumbnail)
        .child(
            div()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .child(name),
        )
        .child(
            div()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text3())
                .child(line),
        )
}

fn centred(content: impl IntoElement) -> AnyElement {
    h_flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(content)
        .into_any_element()
}
