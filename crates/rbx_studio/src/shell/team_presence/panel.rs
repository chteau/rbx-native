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
fn status_text(connection: &Connection, route: Option<&rbx_presence::client::Route>) -> String {
    use rbx_presence::client::Route;
    match (connection, route) {
        (Connection::Connected, Some(Route::Hosting(_))) => {
            return "Connected · hosting the room for collaborators".into()
        }
        (Connection::Retrying(_), Some(Route::Waiting)) => {
            return "Waiting for collaborators to open the place".into()
        }
        _ => {}
    }
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

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

/// "Ada made 3 edits to 2 properties across 1 instance" — with singulars.
fn author_line(a: &rbx_presence::catchup::AuthorChange) -> String {
    format!(
        "{} made {} {} to {} {} across {} {}",
        a.name,
        a.edits,
        plural(a.edits, "edit", "edits"),
        a.properties,
        plural(a.properties, "property", "properties"),
        a.instances,
        plural(a.instances, "instance", "instances"),
    )
}

/// A collaborator as Studio draws one beside a Team Create session: their
/// round Roblox headshot in a ring of their presence colour, or their
/// initial on that colour until the headshot arrives (or when it can't).
pub(in crate::shell) fn avatar(
    image: Option<std::sync::Arc<RenderImage>>,
    color: (u8, u8, u8),
    name: &str,
    size: f32,
) -> Div {
    let (r, g, b) = color;
    let colour = rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
    let ring = div()
        .flex_none()
        .size(px(size))
        .rounded_full()
        .border_2()
        .border_color(colour)
        .p(px(1.))
        .flex()
        .items_center()
        .justify_center();
    match image {
        Some(image) => ring.child(img(image).size_full().rounded_full()),
        None => ring.child(
            div()
                .size_full()
                .rounded_full()
                .bg(colour)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px((size * 0.45).round()))
                .font_weight(tokens::WEIGHT_BOLD)
                .text_color(tokens::black())
                .child(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default(),
                ),
        ),
    }
}

fn collaborator_row(who: &Collaborator, image: Option<std::sync::Arc<RenderImage>>) -> Div {
    // Under the name, past the avatar.
    let line = |text: String| {
        div()
            .pl(px(30.))
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
                .child(avatar(image, who.color, &who.name, 22.))
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
        let route = self.team_presence.route.clone();
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
                            match owner_kind {
                                rbx_presence::relay::RelayKind::Hosted => "Auto",
                                rbx_presence::relay::RelayKind::Worker => "Worker",
                                rbx_presence::relay::RelayKind::Ntfy => "ntfy",
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
                // Wraps inside the dock rather than running off its edge.
                .min_w_0()
                .text_size(tokens::text_xs())
                .text_color(if failed {
                    tokens::text_error()
                } else {
                    tokens::text2()
                })
                .child(status_text(&connection, route.as_ref())),
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

        // "While you were away": the value-less catch-up summary, dismissible.
        // Shows per-author counts only; a value diff would need the DOM diff
        // ("View changes"), which is a separate, transport-independent path.
        let has_before = self.team_presence.before.is_some();
        let away = self.team_presence.away_changes;
        let authors: &[rbx_presence::catchup::AuthorChange] = self
            .team_presence
            .catchup
            .as_ref()
            .map_or(&[], |summary| summary.authors.as_slice());
        let show_catchup = !self.team_presence.catchup_dismissed
            && (!authors.is_empty() || (has_before && away > 0));
        let catchup = show_catchup.then(|| {
            let mut block = v_flex()
                .m_2()
                .p_2()
                .gap_1()
                .border_1()
                .border_color(tokens::border())
                .rounded_sm()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text_strong())
                                .child("While you were away"),
                        )
                        .child(
                            div()
                                .id("collaborators-catchup-dismiss")
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text3())
                                .cursor_pointer()
                                .child("Dismiss")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|shell, _, _, cx| shell.dismiss_catchup(cx)),
                                ),
                        ),
                );
            for author in authors {
                block = block.child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text2())
                        .child(author_line(author)),
                );
            }
            if has_before && away > 0 {
                let plural = if away == 1 { "change" } else { "changes" };
                block
                    .child(
                        div()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text2())
                            .child(format!(
                                "{away} {plural} to the place since you were last here"
                            )),
                    )
                    .child(
                        div()
                            .id("collaborators-view-changes")
                            .text_size(tokens::text_xs())
                            .text_color(tokens::accent_line())
                            .cursor_pointer()
                            .child("View changes")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|shell, _, _, cx| shell.open_presence_diff(cx)),
                            ),
                    )
            } else {
                block.child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text3())
                        .child("Edit values are not shared; see the place history for details."),
                )
            }
        });

        let mut list = v_flex().w_full();
        for who in &self.team_presence.collaborators {
            let image = self.team_presence.avatars.get(&who.user_id).cloned();
            list = list.child(collaborator_row(who, image));
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
                    .children(catchup)
                    .child(list),
            )
            .scrollbar_y(&self.team_presence.scroll);

        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }
}

impl Shell {
    /// The editor's title bar, like Studio's: everyone sharing presence as
    /// overlapping avatar rings, the name and state on hover; a click opens
    /// the Collaborators dock. `None` when nobody is there.
    pub(in crate::shell) fn presence_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        const SIZE: f32 = 24.;
        const MAX: usize = 5;
        let sending: Vec<&Collaborator> = self
            .team_presence
            .collaborators
            .iter()
            .filter(|c| c.status == super::model::Status::Sending)
            .collect();
        if sending.is_empty() {
            return None;
        }
        let extra = sending.len().saturating_sub(MAX);
        let mut strip = h_flex()
            .id("presence-strip")
            .flex_none()
            .items_center()
            .cursor_pointer()
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.set_panel_open(Panel::Collaborators, true, cx);
            }));
        for (i, who) in sending.iter().take(MAX).enumerate() {
            let image = self.team_presence.avatars.get(&who.user_id).cloned();
            let label = SharedString::from(format!("{} \u{b7} {}", who.name, who.status.label()));
            strip = strip.child(
                div()
                    .id(SharedString::from(format!(
                        "presence-strip-{}-{i}",
                        who.user_id
                    )))
                    // Overlap like Studio's row of faces.
                    .when(i > 0, |this| this.ml(px(-6.)))
                    .rounded_full()
                    .bg(tokens::chrome())
                    .tooltip(move |window, cx| {
                        crate::shell::tooltip::text(label.clone(), window, cx)
                    })
                    .child(avatar(image, who.color, &who.name, SIZE)),
            );
        }
        if extra > 0 {
            strip = strip.child(
                div()
                    .ml(px(4.))
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text2())
                    .child(format!("+{extra}")),
            );
        }
        Some(strip.into_any_element())
    }
}
