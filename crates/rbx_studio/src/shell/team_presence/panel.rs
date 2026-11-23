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

/// Under [`NOTE`] with the hosted relay, the one that goes through ntfy.sh
/// and a Cloudflare quick tunnel: what Studio asks the first time.
const STUDIO_NOTE: &str = "First time in Studio? It asks if the plugin can reach ntfy.sh and 127.0.0.1. Allow both. With RbxNative on the same computer, that's all it ever asks. For someone in RbxNative on another computer, it also asks about a trycloudflare.com address, new every time they open RbxNative.";

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

/// Hosting on the loopback port only, while the tunnel waits for its next
/// try: Studio on this computer works, collaborators elsewhere don't yet.
fn paused_text(paused: &rbx_presence::client::Paused, now_ms: u64) -> String {
    let why = if paused.rate_limited {
        "Cloudflare is rate limiting this network"
    } else {
        paused.why.lines().next().unwrap_or_default()
    };
    let left = std::time::Duration::from_millis(paused.retry_at_ms.saturating_sub(now_ms));
    let when = if left.is_zero() {
        "retrying now".to_owned()
    } else {
        format!("retrying in {}", rbx_play::tunnel::minutes(left))
    };
    format!(
        "Connected · Studio on this computer sees you. Remote collaborators paused: {why}, {when}"
    )
}

/// The status line and whether it reads as a failure: two editors on
/// different keys first, since each would otherwise wait for the other
/// without a word, then the connection, with what to do about a key this
/// editor just made while nobody has joined.
fn status_line(presence: &super::TeamPresence) -> (String, bool) {
    // Seeing someone means the key works: a conflict then is someone else's.
    let conflict = presence
        .key_conflict
        .as_deref()
        .filter(|_| presence.collaborators.is_empty());
    if let Some(client) = conflict {
        return (
            conflict_text(client, presence.key_check.next.as_ref()),
            true,
        );
    }
    let mut text = status_text(&presence.connection, presence.route.as_ref());
    if let (Connection::Connected, Some(paused)) = (&presence.connection, &presence.remote_paused) {
        text = paused_text(paused, super::now_ms());
    }
    if presence.minted
        && presence.collaborators.is_empty()
        && presence.connection != Connection::Off
    {
        return (
            format!("{text}. This place had no presence key, so I made one. Save it to Roblox (File > Save to Roblox) so Studio gets it. If Studio already has the place open with its own key, save it from Studio instead and this switches over by itself."),
            false,
        );
    }
    (text, matches!(presence.connection, Connection::Rejected(_)))
}

/// What to do about `client` ("studio" or "rbx-native") having the place
/// open with another key, from what reading the saved place said (`next`;
/// `None` while it is read, an error when it can't be).
fn conflict_text(
    client: &str,
    next: Option<&Result<rbx_presence::converge::Next, String>>,
) -> String {
    use rbx_presence::converge::Next;
    let studio = client == "studio";
    let who = if studio {
        "Studio"
    } else {
        "Another RbxNative"
    };
    let head = format!(
        "{who} has this place open with a different presence key, so you can't see each other."
    );
    let todo = match (next, studio) {
        (Some(Ok(Next::PeerReopens)), true) => "Studio has an older copy: close the place in Studio and open it again from Roblox.".to_owned(),
        (Some(Ok(Next::PeerReopens)), false) => "It switches to the key saved on Roblox by itself.".to_owned(),
        (Some(Err(why)), true) => format!("Couldn't read the place saved on Roblox ({why}). Save the place from Studio (File > Save to Roblox), then reopen it here from My Games and pick Download the published version."),
        (Some(Err(why)), false) => format!("Couldn't read the place saved on Roblox ({why}). Publish it from one side, then reopen it on the other from My Games and pick Download the published version."),
        (_, true) => "Save the place in Studio (File > Save to Roblox) and this switches to Studio's key by itself.".to_owned(),
        (_, false) => "Publish the place from either one and the other switches over by itself.".to_owned(),
    };
    format!("{head} {todo}")
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

/// "Ada made 3 edits to 2 properties across 1 instance", with singulars.
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
        let relay_input = self.team_presence.relay_input.clone();
        let show_advanced = self.team_presence.show_advanced;
        let owner_kind = self.team_presence.owner_kind;
        let relay = rbx_presence::secret::read_relay(&self.dom);
        let hosted = relay
            .as_ref()
            .is_none_or(|config| config.kind == rbx_presence::relay::RelayKind::Hosted);
        let resolved = match relay {
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
            .child(NOTE)
            .when(hosted, |this| this.child(div().mt_1().child(STUDIO_NOTE)));

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

        // Owner setup: pick a transport, type its URL; each change goes into
        // the place so collaborators read it. Non-owners can ignore it.
        let owner_setup = v_flex()
            .w_full()
            .gap_1()
            .px_2()
            .child(
                div()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text3())
                    .child("Owner setup, saved into the place as you change it"),
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
                    .child(div().flex_1().child(Input::new(&relay_input))),
            )
            .child(
                div()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text3())
                    .child(resolved),
            );

        let (status_text, failed) = status_line(&self.team_presence);
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
                .child(status_text),
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
                    .children(self.remote_play_block(cx))
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

#[cfg(test)]
mod tests {
    use super::{conflict_text, paused_text};
    use rbx_presence::client::Paused;

    #[test]
    fn a_paused_tunnel_names_the_cause_and_the_wait() {
        let paused = Paused {
            why: "Cloudflare is rate limiting this network".into(),
            rate_limited: true,
            retry_at_ms: 1_000 + 4 * 60_000 + 1,
        };
        assert_eq!(
            paused_text(&paused, 1_000),
            "Connected · Studio on this computer sees you. Remote collaborators paused: Cloudflare is rate limiting this network, retrying in 5 min"
        );
        let failed = Paused {
            why: "couldn't open the tunnel (the tunnel exited before giving a URL:\nboom)".into(),
            rate_limited: false,
            retry_at_ms: 0,
        };
        assert!(paused_text(&failed, 1_000).ends_with(
            "paused: couldn't open the tunnel (the tunnel exited before giving a URL:, retrying now"
        ));
    }
    use rbx_presence::converge::Next;

    #[test]
    fn a_conflict_asks_for_one_step_on_the_other_side() {
        let saves = conflict_text("studio", Some(&Ok(Next::PeerSaves)));
        assert!(saves.contains("Save the place in Studio"));
        assert!(saves.contains("by itself"));
        assert!(!saves.contains("Download the published version"));
        assert_eq!(conflict_text("studio", None), saves);
        assert!(conflict_text("studio", Some(&Ok(Next::PeerReopens))).contains("open it again"));
        assert!(conflict_text("rbx-native", Some(&Ok(Next::PeerReopens))).contains("by itself"));
    }

    #[test]
    fn an_unreadable_saved_place_falls_back_to_the_manual_steps() {
        let text = conflict_text("studio", Some(&Err("HTTP 403".into())));
        assert!(text.contains("HTTP 403"));
        assert!(text.contains("Download the published version"));
    }
}
