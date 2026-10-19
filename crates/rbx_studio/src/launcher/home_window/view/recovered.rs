//! Recovered places: copies Auto-Recovery left from a session that crashed
//! or was killed, offered above Recent until they are opened or discarded.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::*;
use crate::launcher::home_window::HomeWindow;
use crate::launcher::ui::{self, Weight};
use crate::recovery::{self, Leftover};
use crate::tokens;

impl HomeWindow {
    /// Reads the recovery folder again, after a copy was discarded.
    fn refresh_recovered(&mut self) {
        self.recovered = recovery::folder()
            .map(|folder| recovery::leftovers(&folder))
            .unwrap_or_default();
    }

    /// The section, or nothing when there is nothing to recover.
    pub(super) fn recovered_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let last = self.recovered.len().checked_sub(1)?;
        let rows = self.recovered.iter().enumerate().map(|(index, left)| {
            let (open, discard) = (left.path.clone(), left.path.clone());
            h_flex()
                .h(px(48.))
                .items_center()
                .gap(px(GAP))
                .px(px(14.))
                .when(index != last, |this| {
                    this.border_b_1().border_color(tokens::border())
                })
                .child(
                    ui::icon("file", 14.)
                        .flex_none()
                        .text_color(tokens::text2()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            ui::text(12.5, 17.)
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text())
                                .child(left.name.clone()),
                        )
                        .child(
                            ui::text(11.5, 16.)
                                .text_color(tokens::text3())
                                .child(format!("Auto-saved {}", written_label(left))),
                        ),
                )
                .child(
                    ui::button(
                        SharedString::from(format!("recovered-open-{index}")),
                        "Open",
                        Weight::Secondary,
                        true,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.open_path_later(open.clone(), cx)),
                    ),
                )
                .child(
                    ui::button(
                        SharedString::from(format!("recovered-discard-{index}")),
                        "Discard",
                        Weight::Ghost,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let _ = recovery::discard(&discard);
                        this.refresh_recovered();
                        cx.notify();
                    })),
                )
        });
        Some(
            v_flex()
                .gap(px(12.))
                .child(Self::section_head("Recovered places", None))
                .child(
                    v_flex()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(tokens::border())
                        .bg(ui::panel2())
                        .overflow_hidden()
                        .children(rows),
                )
                .into_any_element(),
        )
    }
}

fn written_label(left: &Leftover) -> String {
    let secs = left
        .modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    opened_label(secs)
}
