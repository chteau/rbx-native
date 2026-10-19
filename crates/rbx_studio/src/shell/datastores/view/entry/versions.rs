//! The entry pane’s Versions tab: the revisions Roblox kept, each viewable
//! and (but the newest) restorable.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::super::state::Load;
use super::super::super::DataStoresWindow;
use super::super::common::{list_state, load_more, time_label};
use super::mono;

impl DataStoresWindow {
    pub(super) fn versions_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = v_flex();
        for (i, rev) in self.pane.revisions.items.iter().enumerate() {
            let id = rev.revision_id.clone();
            let for_rollback = rev.clone();
            list = list.child(
                h_flex()
                    .h(px(30.))
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .text_color(tokens::text3())
                            .child(ui::icon("clock", 13.)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .child(time_label(rev.revision_create_time.as_deref())),
                    )
                    .child(
                        mono(rev.revision_id.clone())
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_ellipsis()
                            .text_color(tokens::text3()),
                    )
                    .when(i == 0, |d| {
                        d.child(div().flex_none().child(ui::tag(
                            "Current",
                            ui::green(),
                            ui::green_soft(),
                        )))
                    })
                    .when(rev.deleted, |d| {
                        d.child(div().flex_none().child(ui::tag(
                            "Deleted",
                            tokens::text3(),
                            tokens::hover(),
                        )))
                    })
                    .child(
                        ui::button(
                            SharedString::from(format!("ds-rev-view-{i}")),
                            "View",
                            Weight::Ghost,
                            true,
                        )
                        .flex_none()
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.view_revision(id.clone(), cx)),
                        ),
                    )
                    .when(i > 0, |d| {
                        d.child(
                            ui::button(
                                SharedString::from(format!("ds-rev-roll-{i}")),
                                "Roll back\u{2026}",
                                Weight::Ghost,
                                true,
                            )
                            .flex_none()
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.ask_rollback(for_rollback.clone(), cx),
                            )),
                        )
                    }),
            );
        }
        let has_items = !self.pane.revisions.items.is_empty();
        let state = list_state(&self.pane.revisions.load, has_items, "No versions yet.", cx);
        let more = self.pane.revisions.next.is_some().then(|| {
            load_more(
                "ds-revs-more",
                self.pane.revisions.load == Load::Busy,
                cx.listener(|this, _, _, cx| this.load_revisions(true, cx)),
            )
        });
        v_flex()
            .gap(px(8.))
            .child(
                div().text_color(tokens::text3()).child(
                    "Roblox keeps the first write of each UTC hour as a version, for 30 days.",
                ),
            )
            .child(list)
            .children(state)
            .children(more)
            .into_any_element()
    }
}
