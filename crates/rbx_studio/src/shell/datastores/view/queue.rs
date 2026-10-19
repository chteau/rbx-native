//! The queue half of the Memory Store section.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::shell::datastores::state::{ListId, Load};
use crate::shell::datastores::view::common::{json_area, list_row, problem_panel, time_label};
use crate::shell::datastores::view::entry::boxed;
use crate::shell::datastores::{json_tree, DataStoresWindow};
use crate::tokens;

use super::{error_line, side_column, Click, SideColumn};

impl DataStoresWindow {
    pub(super) fn queue_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let recent = self
            .queue
            .recent
            .iter()
            .cloned()
            .map(|name| {
                let n = name.clone();
                let click: Click = Box::new(
                    cx.listener(move |this, _, _, cx| this.open_queue_named(n.clone(), cx)),
                );
                (name, click)
            })
            .collect();
        let side = side_column(SideColumn {
            title: "QUEUE",
            fields: vec![("Name", self.inputs.queue_name.clone())],
            open: Box::new(cx.listener(|this, _, _, cx| this.open_queue(cx))),
            note: "Open Cloud can\u{2019}t list queues, so open one by name.",
            recent,
        });
        let main = if self.queue.name.is_some() {
            self.queue_main(cx)
        } else {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child("Open a queue to read from it.")
                .into_any_element()
        };
        h_flex()
            .size_full()
            .child(side)
            .child(main)
            .into_any_element()
    }

    fn queue_main(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let discard = self
            .queue
            .read
            .as_ref()
            .map(|r| r.items.len())
            .filter(|&n| n > 0);
        let card = div().p(px(16.)).pb(px(8.)).child(
            div()
                .p(px(12.))
                .rounded(tokens::radius())
                .border_1()
                .border_color(tokens::border())
                .text_color(tokens::text2())
                .child(
                    "Reading takes items from the front of the queue and hides them from game \
                     servers for the invisibility window. Unless you discard them, they come \
                     back when it ends.",
                ),
        );
        let toolbar = h_flex()
            .px(px(16.))
            .py(px(8.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .child(div().text_color(tokens::text2()).child("Items"))
            .child(boxed(&self.inputs.queue_count, 70.))
            .child(div().text_color(tokens::text2()).child("Invisible for (s)"))
            .child(boxed(&self.inputs.queue_invisibility, 70.))
            .child(
                ui::button("ds-queue-read", "Read", Weight::Primary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.read_queue(cx))),
            )
            .child(div().flex_1())
            .child(
                ui::icon_button(
                    "ds-queue-add",
                    "plus",
                    "Add item\u{2026}",
                    Weight::Secondary,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| this.start_queue_add(true, cx))),
            )
            .when_some(discard, |d, count| {
                d.child(
                    ui::button(
                        "ds-queue-discard",
                        SharedString::from(format!("Discard these {count} items\u{2026}")),
                        Weight::Danger,
                        true,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.ask_queue_discard(cx))),
                )
            });

        let mut area = v_flex()
            .id("ds-queue-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(8.))
            .p(px(16.));
        if self.queue.adding {
            area = area.child(self.queue_add_form(cx));
        }
        if let Load::Failed(problem) = &self.queue.load {
            area = area.child(problem_panel(problem, cx));
        }
        if self.queue.load == Load::Busy {
            area = area.child(ui::spinner("ds-queue-spin", 14.));
        }
        if let Some(read) = self.queue.read.clone() {
            if read.items.is_empty() {
                area = area.child(
                    div()
                        .text_color(tokens::text3())
                        .child("The queue had nothing to read."),
                );
            }
            for (i, item) in read.items.iter().enumerate() {
                area = area.child(
                    list_row(SharedString::from(format!("ds-qitem-{i}")), false)
                        .gap(px(12.))
                        .child(
                            div()
                                .w(px(140.))
                                .flex_none()
                                .truncate()
                                .font_family(tokens::font_mono())
                                .text_size(px(11.5))
                                .text_color(tokens::text3())
                                .child(item.id.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(tokens::text3())
                                .child(format!("priority {}", item.priority)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(tokens::text3())
                                .child(time_label(item.expire_time.as_deref())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_ellipsis()
                                .font_family(tokens::font_mono())
                                .text_size(px(12.))
                                .child(json_tree::short(&item.data, 120)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_row(ListId::QueueItems, i, cx)
                        })),
                );
            }
        }
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(card)
            .child(toolbar)
            .child(area)
            .into_any_element()
    }

    fn queue_add_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .gap(px(8.))
            .child(div().text_color(tokens::text2()).child("Data (JSON)"))
            .child(json_area(&self.inputs.queue_data))
            .child(div().text_color(tokens::text2()).child("Priority"))
            .child(boxed(&self.inputs.queue_priority, 120.))
            .child(
                div()
                    .text_color(tokens::text2())
                    .child("Expires after (seconds)"),
            )
            .child(boxed(&self.inputs.queue_ttl, 120.))
            .children(error_line(&self.queue.error))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        ui::button("ds-queue-cancel", "Cancel", Weight::Ghost, true).on_click(
                            cx.listener(|this, _, _, cx| this.start_queue_add(false, cx)),
                        ),
                    )
                    .child(
                        ui::button(
                            "ds-queue-add-go",
                            "Review and add\u{2026}",
                            Weight::Primary,
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.submit_queue_add(cx))),
                    ),
            )
            .into_any_element()
    }
}
