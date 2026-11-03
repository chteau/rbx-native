//! The Toolbox dock's search field, category chips, result cards and scripts notice.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Category, Load};
use crate::launcher::ui::{self, Weight};
use crate::shell::{chrome, menu, Panel, Shell};
use crate::tokens;

impl Shell {
    pub(in crate::shell) fn toolbox_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.ensure_toolbox_query(window, cx);
        let overflow = menu::dropdown(
            self,
            menu::MenuId::ToolboxOverflow,
            chrome::dock_options_trigger(
                "toolbox-overflow",
                IconName::Ellipsis,
                16.,
                "Toolbox options",
            ),
            self.move_items(Panel::Toolbox),
            cx,
        );

        let toolbox = &self.toolbox;
        let label = toolbox.category.label().to_lowercase();
        let (caption, warn) = match &toolbox.load {
            Load::Loading => ("Searching the Creator Store".to_owned(), false),
            Load::Ready if toolbox.text.is_empty() => (format!("Most popular free {label}"), false),
            Load::Ready => (
                format!(
                    "{} free {label} for \u{201c}{}\u{201d}",
                    toolbox.ids.len(),
                    toolbox.text
                ),
                false,
            ),
            Load::Failed(_) => ("Search failed".to_owned(), true),
            Load::Idle => (String::new(), false),
        };

        let field = toolbox.query.clone().map(|query| {
            h_flex()
                .h(px(32.))
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
                        Input::new(&query)
                            .appearance(false)
                            .h_full()
                            .px(px(0.))
                            .text_size(tokens::text_md())
                            .text_color(tokens::text()),
                    ),
                )
        });

        let selected = toolbox.category;
        let chips = Category::ALL.map(|category| {
            let on = category == selected;
            div()
                .id(SharedString::from(format!(
                    "toolbox-category-{}",
                    category.label()
                )))
                .flex_1()
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::radius_tiny())
                .cursor_pointer()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(if on { tokens::text() } else { tokens::text2() })
                .when(on, |this| this.bg(tokens::accent_soft()))
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.toolbox_set_category(category, cx);
                }))
                .child(category.label())
        });

        let results = match &toolbox.load {
            Load::Loading => h_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .text_color(tokens::text2())
                .text_size(tokens::text_sm())
                .child(ui::spinner("toolbox-searching", 14.))
                .child("Searching the Creator Store")
                .into_any_element(),
            Load::Failed(err) => h_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text_error())
                .child(format!("Couldn't reach the Creator Store: {err}"))
                .into_any_element(),
            Load::Ready if toolbox.ids.is_empty() => h_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text2())
                .child(format!(
                    "Nothing free matches \u{201c}{}\u{201d}.",
                    toolbox.text
                ))
                .into_any_element(),
            _ => {
                let cards: Vec<AnyElement> = toolbox
                    .ids
                    .iter()
                    .filter_map(|&id| self.toolbox_card(id, cx))
                    .collect();
                v_flex()
                    .id("toolbox-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(div().grid().grid_cols(2).gap(px(8.)).children(cards))
                    .into_any_element()
            }
        };

        let body = v_flex()
            .size_full()
            .p(px(10.))
            .gap(px(10.))
            .children(field)
            .child(
                h_flex()
                    .w_full()
                    .p(px(2.))
                    .gap(px(2.))
                    .rounded(tokens::radius_segment())
                    .bg(tokens::field_select())
                    .children(chips),
            )
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
            .when_some(toolbox.error.clone(), |this, error| {
                this.child(
                    div()
                        .whitespace_normal()
                        .text_size(tokens::text_xs())
                        .line_height(tokens::line_xs())
                        .text_color(tokens::text_error())
                        .child(error),
                )
            })
            .child(results)
            .into_any_element();

        (Some(overflow.into_any_element()), Some(body))
    }

    fn toolbox_card(&self, id: u64, cx: &mut Context<Self>) -> Option<AnyElement> {
        let item = self.toolbox.items.get(&id)?;
        let thumbnail = match self.toolbox.icons.get(&id) {
            Some(Some(icon)) => img(icon.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            _ => div()
                .text_color(tokens::text3())
                .child(ui::icon("package", 20.))
                .into_any_element(),
        };
        let thumbnail = div()
            .relative()
            .w_full()
            .h(px(96.))
            .rounded(tokens::radius_tiny())
            .overflow_hidden()
            .bg(tokens::tile())
            .flex()
            .items_center()
            .justify_center()
            .child(thumbnail)
            .when(self.toolbox.busy == Some(id), |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(tokens::field_select())
                        .opacity(0.85)
                        .child(ui::spinner(format!("toolbox-busy-{id}"), 16.)),
                )
            });
        Some(
            v_flex()
                .id(SharedString::from(format!("toolbox-card-{id}")))
                .p(px(6.))
                .gap(px(6.))
                .rounded(tokens::radius_tile())
                .bg(tokens::field_select())
                .border_1()
                .border_color(tokens::border())
                .cursor_pointer()
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .on_click(cx.listener(move |shell, _, _, cx| shell.toolbox_insert(id, cx)))
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
                        .child(item.name.clone()),
                )
                .child(
                    div()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(tokens::text_xs())
                        .line_height(tokens::line_xs())
                        .text_color(tokens::text3())
                        .child(format!("by {}", item.creator_name)),
                )
                .into_any_element(),
        )
    }

    pub(in crate::shell) fn toolbox_notice_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let notice = self.toolbox.notice.as_ref()?;
        let title = if notice.scripts == 1 {
            format!("{} includes 1 script.", notice.name)
        } else {
            format!("{} includes {} scripts.", notice.name, notice.scripts)
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .child(ui::dialog(
                    440.,
                    ui::dialog_glyph("triangle-alert", ui::accent(), ui::wash()),
                    title,
                    "Review them before you play, so they don't change your experience in ways you didn't expect. Cancel takes the asset back out.".to_string(),
                    None,
                    vec![
                        ui::button("toolbox-notice-cancel", "Cancel", Weight::Secondary, false)
                            .on_click(cx.listener(|shell, _, _, cx| shell.toolbox_take_back(cx)))
                            .into_any_element(),
                        ui::button("toolbox-notice-ok", "OK", Weight::Primary, false)
                            .on_click(cx.listener(|shell, _, _, cx| shell.toolbox_keep(cx)))
                            .into_any_element(),
                    ],
                ))
                .into_any_element(),
        )
    }
}
