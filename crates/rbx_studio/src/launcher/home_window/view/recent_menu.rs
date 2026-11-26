//! Recent's per-place menu and the Archive/Restore confirmation it leads to.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::home::RecentPlace;
use crate::launcher::home_window::recent_actions::RecentAction;
use crate::launcher::home_window::{file_name, HomeWindow};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

impl HomeWindow {
    pub(super) fn recent_menu_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.recent_menu.as_ref()?;
        let signed_in = self.session.is_some();
        let offline = crate::offline::blocked();
        let rows = RecentAction::menu(self.place_archived(&menu.place)).map(|action| {
            let why = action.unavailable(&menu.place, signed_in, offline);
            h_flex()
                .id(SharedString::from(format!("recent-menu-{action:?}")))
                .h(px(28.))
                .px(px(10.))
                .items_center()
                .rounded(px(5.))
                .text_size(px(12.5))
                .line_height(px(17.))
                .map(|this| match why {
                    None => this
                        .cursor_pointer()
                        .text_color(tokens::text())
                        .hover(|this| tokens::hover_fx(this).bg(ui::wash()))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.run_recent_action(action, cx)),
                        ),
                    Some(why) => this
                        .cursor_not_allowed()
                        .text_color(tokens::text_disabled())
                        .tooltip(move |window, cx| crate::shell::tooltip::text(why, window, cx)),
                })
                .child(action.label())
        });
        let card = v_flex()
            .id("recent-menu")
            .min_w(px(200.))
            .p(px(4.))
            .gap(px(1.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border2())
            .bg(ui::panel())
            .shadow(tokens::elevation())
            .occlude()
            .track_focus(&menu.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.recent_menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(rows);
        // A press anywhere else only dismisses, as a desktop menu's does:
        // the window under the menu is covered while it is up, so that press
        // can't also open the row or press the button it lands on.
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                        this.recent_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(
                        anchored()
                            .position(menu.at)
                            .snap_to_window_with_margin(px(8.))
                            .child(card),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    pub(super) fn archive_dialog(
        &self,
        place: &RecentPlace,
        restore: bool,
        busy: bool,
        error: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = place.name.clone().unwrap_or_else(|| file_name(&place.path));
        let body = error.map(|error| {
            div()
                .mt(px(14.))
                .mx(px(20.))
                .py(px(10.))
                .px(px(12.))
                .rounded(px(6.))
                .border_1()
                .border_color(ui::red_line())
                .bg(ui::red_soft())
                .text_size(px(11.5))
                .line_height(px(17.))
                .text_color(tokens::text())
                .child(error.to_string())
                .into_any_element()
        });
        let (verb, busy_label, weight, glyph, fg, fill, text) = if restore {
            (
                "Restore",
                "Restoring\u{2026}",
                Weight::Primary,
                "archive-restore",
                ui::accent(),
                tokens::accent_soft(),
                "It goes back to your experiences on Roblox, set up as it was before it was archived. Your local file stays as is.",
            )
        } else {
            (
                "Archive",
                "Archiving\u{2026}",
                Weight::Danger,
                "archive",
                ui::red(),
                ui::red_soft(),
                "It comes off Roblox and nobody can join it. You can restore it from this menu or the Creator Dashboard, and your local file stays as is.",
            )
        };
        let confirm = if busy {
            ui::disabled_button("archive-confirm", busy_label).into_any_element()
        } else {
            ui::button("archive-confirm", verb, weight, false)
                .on_click(cx.listener(|this, _, _, cx| this.confirm_archive(cx)))
                .into_any_element()
        };
        ui::dialog(
            480.,
            ui::dialog_glyph(glyph, fg, fill),
            format!("{verb} {name}?"),
            text,
            body,
            vec![
                ui::button("archive-cancel", "Cancel", Weight::Secondary, false)
                    .when(!busy, |this| {
                        this.on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                    })
                    .into_any_element(),
                confirm,
            ],
        )
    }
}
