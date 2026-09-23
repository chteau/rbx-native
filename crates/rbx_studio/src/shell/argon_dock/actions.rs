//! The connection column's action row: the `host : port` field, the "?"
//! and the button for the connection's state, or Diff / Cancel / Accept
//! while a batch waits for review.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;

use crate::tokens;

use super::super::Shell;
use super::connection::View;
use super::Layout;

impl Shell {
    pub(super) fn action_row(
        &mut self,
        view: &View,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> Div {
        let readonly = !matches!(view, View::Disconnected);
        let row = if layout.wrap_actions {
            v_flex().w_full().gap(px(8.))
        } else {
            h_flex().w_full().items_center().gap(px(8.))
        };
        if let View::Review { .. } = view {
            let diff = secondary_button("argon-diff", "Diff", self.tab_order.next())
                .px(px(12.))
                .gap(px(6.))
                .child(Icon::new(IconName::GitCompare).size(px(13.)))
                .child("Diff")
                .on_click(cx.listener(|shell, _, _, cx| shell.open_argon_diff(cx)));
            let cancel = secondary_button("argon-cancel", "Cancel", self.tab_order.next())
                .child("Cancel")
                .on_click(cx.listener(|shell, _, _, cx| shell.argon_cancel_pending(cx)));
            let accept = primary_button("argon-accept", "Accept", self.tab_order.next())
                .on_click(cx.listener(|shell, _, _, cx| shell.argon_accept_pending(cx)));
            return if layout.wrap_actions {
                row.child(
                    h_flex()
                        .w_full()
                        .gap(px(8.))
                        .child(diff.flex_1())
                        .child(cancel.flex_1()),
                )
                .child(accept.w_full())
            } else {
                row.child(diff)
                    .child(div().flex_1())
                    .child(cancel.w(px(96.)))
                    .child(accept.w(px(96.)))
            };
        }

        let first = h_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .child(self.address_field(readonly))
            .child(self.help_button(layout));
        let button: Stateful<Div> = match view {
            View::Disconnected => primary_button("argon-connect", "Connect", self.tab_order.next())
                .on_click(cx.listener(|shell, _, _, cx| shell.argon_connect(cx))),
            View::Connecting { .. } => disabled_button("argon-connect", "Connect"),
            View::Connected { .. } => {
                secondary_button("argon-disconnect", "Disconnect", self.tab_order.next())
                    .child("Disconnect")
                    .on_click(cx.listener(|shell, _, _, cx| shell.argon_disconnect(cx)))
            }
            View::Error { .. } => {
                secondary_button("argon-dismiss", "Dismiss", self.tab_order.next())
                    .child("Dismiss")
                    .on_click(cx.listener(|shell, _, _, cx| shell.argon_disconnect(cx)))
            }
            View::Review { .. } => unreachable!("handled above"),
        };
        if layout.wrap_actions {
            row.child(first.w_full()).child(button.w_full())
        } else {
            row.child(first).child(button.w(px(96.)))
        }
    }

    /// `host : port` in one 34px box: two mono inputs, read-only (on
    /// `panel`, `text2`) while anything but disconnected.
    pub(super) fn address_field(&mut self, readonly: bool) -> Div {
        let host = self.argon_ui.host.clone();
        let port = self.argon_ui.port.clone();
        let (host_tab, port_tab) = (self.tab_order.next(), self.tab_order.next());
        let ink = if readonly {
            tokens::text2()
        } else {
            tokens::text()
        };
        let input = |state: &Entity<gpui_kit::component::input::InputState>, tab: isize| {
            Input::new(state)
                .appearance(false)
                .h_full()
                .readonly(readonly)
                .tab_index(tab)
                .px(px(10.))
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_md())
                .text_color(ink)
        };
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(34.))
            .items_center()
            .rounded(tokens::RADIUS)
            .bg(if readonly {
                tokens::dock()
            } else {
                tokens::field_select()
            })
            .border_1()
            .border_color(tokens::border())
            .track_focus(&self.argon_ui.field_focus)
            .in_focus(|this| this.border_color(tokens::accent_line()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(input(&host, host_tab)),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .text_color(tokens::text3())
                    .child(":"),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(66.))
                    .h_full()
                    .child(input(&port, port_tab)),
            )
    }
}

/// 34px, `accent` fill, `bg`-coloured bold label.
fn primary_button(id: &'static str, label: &'static str, tab: isize) -> Stateful<Div> {
    h_flex()
        .id(id)
        .tab_index(tab)
        .flex_none()
        .h(px(34.))
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .bg(tokens::check_on())
        .text_size(tokens::text_action())
        .line_height(tokens::line_action())
        .font_weight(tokens::WEIGHT_BOLD)
        .text_color(tokens::black())
        .cursor_pointer()
        .hover(|this| this.bg(tokens::accent_hover()))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .child(label)
}

/// 34px, `panel2` on a `border2` hairline, `text` at 600. The caller adds
/// the label (and an icon before it, for Diff).
fn secondary_button(id: &'static str, label: &'static str, tab: isize) -> Stateful<Div> {
    h_flex()
        .id(id)
        .tab_index(tab)
        .flex_none()
        .h(px(34.))
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border2())
        .text_size(tokens::text_action())
        .line_height(tokens::line_action())
        .font_weight(tokens::WEIGHT_SEMIBOLD)
        .text_color(tokens::text())
        .cursor_pointer()
        .hover(|this| this.bg(tokens::secondary_hover()))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .tooltip(move |window, cx| super::super::tooltip::text(label, window, cx))
}

/// The Connect button while a connection is under way: `panel2` on a
/// `border` hairline, `text3`, a spinner, and nothing to click.
fn disabled_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .h(px(34.))
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(tokens::RADIUS)
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .text_size(tokens::text_action())
        .line_height(tokens::line_action())
        .font_weight(tokens::WEIGHT_SEMIBOLD)
        .text_color(tokens::text3())
        .child(Icon::new(IconName::LoaderCircle).size(px(12.)))
        .child(label)
}
