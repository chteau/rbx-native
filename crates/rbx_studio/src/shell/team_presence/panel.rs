//! The Collaborators dock.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::chrome::{self, ScrollbarY as _};
use super::super::layout::Panel;
use super::super::menu::{self, MenuId};
use super::super::rows;
use super::super::Shell;
use super::model::Collaborator;
use super::Connection;
use crate::launcher::ui;
use crate::tokens;

/// Shown at the top of the dock at all times, whatever the state.
pub(super) const NOTE: &str = "Presence only works for collaborators who have Studio open with the rbx-native presence plugin running.";

/// The status line's words for a connection.
fn status_text(connection: &Connection) -> String {
    match connection {
        Connection::Off => "Off".into(),
        Connection::NoPlace => "Link this file to a Roblox place (Publish) first".into(),
        Connection::NoSecret => "This place has no presence key".into(),
        Connection::NoUser => {
            "Sign in to Roblox (Settings > Account) so collaborators know who you are".into()
        }
        Connection::Connecting => "Connecting...".into(),
        Connection::Connected => "Connected".into(),
        Connection::Rejected(why) => format!("Refused: {why}"),
        Connection::Retrying(why) => format!("Reconnecting: {why}"),
        Connection::Mock => "Mock room".into(),
    }
}

fn collaborator_row(who: &Collaborator) -> Div {
    let (r, g, b) = who.color;
    let colour = rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
    // Under the name, past the colour dot.
    let line = |text: String| {
        div()
            .pl(px(18.))
            .text_size(tokens::text_xs())
            .text_color(tokens::text2())
            .child(text)
    };
    v_flex()
        .w_full()
        .gap_1()
        .px_2()
        .py_1()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().flex_none().size(px(10.)).rounded_full().bg(colour))
                .child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_strong())
                        .child(who.name.clone()),
                )
                .child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text3())
                        .child(who.status.label()),
                ),
        )
        .when(!who.selection.is_empty() || who.unresolved > 0, |this| {
            let total = who.selection.len() + who.unresolved;
            let mut text = format!("Selected: {total}");
            if who.unresolved > 0 {
                text.push_str(&format!(" ({} not in this place)", who.unresolved));
            }
            this.child(line(text))
        })
        .children(
            who.script
                .as_ref()
                .map(|(path, _)| line(format!("Has {path} open"))),
        )
        .children(who.edits.iter().max_by_key(|edit| edit.at).map(|edit| {
            line(format!(
                "{} of {} -> {}",
                edit.property, edit.path, edit.value
            ))
        }))
}

impl Shell {
    /// The dock's overflow trigger and its content: the note, the on/off
    /// switch with the relay URL field, the connection line (with "Create
    /// presence key" when the place has none), then one row per
    /// collaborator.
    pub(in crate::shell) fn collaborators_dock(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::CollaboratorsOverflow,
            chrome::dock_options_trigger(
                "collaborators-overflow",
                IconName::Ellipsis,
                16.,
                "Collaborators dock options",
            ),
            self.move_items(Panel::Collaborators),
            cx,
        );

        let enabled = self.team_presence.settings.enabled;
        let connection = self.team_presence.connection.clone();
        let relay_input = self.team_presence.relay_input.clone();
        let show_advanced = self.team_presence.show_advanced;
        let owner_kind = self.team_presence.owner_kind;
        let resolved = match rbx_presence::secret::read_relay(&self.dom) {
            Some(config) => format!("Relay: {} ({})", config.url, config.kind.as_str()),
            None => format!(
                "Relay: {} ({})",
                rbx_presence::relay::COMMUNITY_URL,
                rbx_presence::relay::COMMUNITY_LABEL
            ),
        };

        let note = div()
            .m_2()
            .p_2()
            .border_1()
            .border_color(tokens::border())
            .rounded_sm()
            .text_size(tokens::text_xs())
            .text_color(tokens::text2())
            .child(NOTE);

        let toggle_row = h_flex()
            .w_full()
            .gap_2()
            .px_2()
            .items_center()
            .child(rows::checkbox(
                "collaborators-share",
                enabled,
                cx.listener(move |shell, _, _, cx| {
                    shell.set_team_presence_enabled(!enabled, cx);
                }),
            ))
            .child(
                div()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text())
                    .child("Share my presence"),
            );

        // Owner setup: pick a transport, type its URL, and write both into
        // the place so collaborators read them. Non-owners can ignore it.
        let owner_setup = v_flex()
            .w_full()
            .gap_1()
            .px_2()
            .child(
                div()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text3())
                    .child("Owner setup — saved into the place for everyone"),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        ui::button(
                            "collaborators-relay-kind",
                            if owner_kind == rbx_presence::relay::RelayKind::Ntfy {
                                "ntfy"
                            } else {
                                "Worker"
                            },
                            ui::Weight::Secondary,
                            true,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| shell.toggle_owner_kind(cx))),
                    )
                    .child(div().flex_1().child(Input::new(&relay_input)))
                    .child(
                        ui::button(
                            "collaborators-relay-save",
                            "Save to place",
                            ui::Weight::Secondary,
                            true,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| shell.save_presence_relay(cx))),
                    ),
            )
            .child(
                div()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text3())
                    .child(resolved),
            );

        let failed = matches!(connection, Connection::Rejected(_));
        let status = h_flex().w_full().gap_2().px_2().items_center().child(
            div()
                .flex_1()
                .text_size(tokens::text_xs())
                .text_color(if failed {
                    tokens::text_error()
                } else {
                    tokens::text2()
                })
                .child(status_text(&connection)),
        );

        // The relay URL lives behind an Advanced reveal, so the dock is just
        // the toggle, the status and the list unless someone opts in.
        let advanced_toggle = div().px_2().child(
            div()
                .id("collaborators-advanced")
                .text_size(tokens::text_xs())
                .text_color(tokens::text3())
                .cursor_pointer()
                .child(if show_advanced {
                    "Advanced ▾"
                } else {
                    "Advanced ▸"
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|shell, _, _, cx| shell.toggle_presence_advanced(cx)),
                ),
        );

        let mut list = v_flex().w_full();
        for who in &self.team_presence.collaborators {
            list = list.child(collaborator_row(who));
        }

        let body = div()
            .id("collaborators-body")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.team_presence.scroll)
            .pr(chrome::SCROLLBAR_GUTTER)
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(note)
                    .child(toggle_row)
                    .child(status)
                    .child(advanced_toggle)
                    .when(show_advanced, |this| this.child(owner_setup))
                    .child(list),
            )
            .scrollbar_y(&self.team_presence.scroll);

        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }
}
