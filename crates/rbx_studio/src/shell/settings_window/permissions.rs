//! The Permissions page: one place to see and change what every agent may
//! do. AI assistants (MCP clients) get Studio's MCP Permissions shape: an
//! access level, then Deny/Allow per capability. Each plugin gets a card
//! like Studio's plugin manager entry, its capabilities with Deny/Allow (and
//! Ask where the editor asks first) and the HTTP hosts it was answered for.
//! Every row reads `rbx_permissions` through the shell and writes through
//! `Shell::change_permissions`, which the very next check reads.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_permissions::{
    Capability, Decision, Level, PluginKind, PluginProfile, Policy, MCP_CAPABILITIES,
};

use crate::tokens;

use super::kit::{card, ghost_icon, icon, mono, segmented, text, OnPick};
use super::SettingsWindow;

/// Which plugin cards are open; AI assistants is always open.
#[derive(Default)]
pub(super) struct PermissionsView {
    open: HashSet<String>,
}

impl SettingsWindow {
    pub(super) fn permissions_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let shell = self.shell.read(cx);
        let policy = shell.permissions().borrow().policy().clone();
        let profiles = shell.plugin_profiles();
        v_flex()
            .gap(px(22.))
            .child(group("AI assistants"))
            .child(self.mcp_card(&policy))
            .child(group("Plugins"))
            .map(|page| {
                if profiles.is_empty() {
                    page.child(
                        card().p(px(14.)).child(
                            text(12., 17.)
                                .text_color(tokens::text2())
                                .child("No plugins are loaded. Plugins you install show up here."),
                        ),
                    )
                } else {
                    page.child(
                        v_flex().gap(px(10.)).children(
                            profiles
                                .iter()
                                .enumerate()
                                .map(|(i, p)| self.plugin_card(i, p, &policy, cx)),
                        ),
                    )
                }
            })
            .into_any_element()
    }

    fn mcp_card(&self, policy: &Policy) -> impl IntoElement {
        let shown = policy.mcp_level();
        let mut levels: Vec<(&'static str, bool, OnPick)> = Level::ALL
            .into_iter()
            .map(|level| {
                let pick = self.change(move |p| p.set_mcp_level(level));
                (level.label(), shown == Some(level), pick)
            })
            .collect();
        if shown.is_none() {
            levels.push(("Custom", true, Rc::new(|_: &mut Window, _: &mut App| {})));
        }
        let rows = MCP_CAPABILITIES.iter().enumerate().map(|(i, &capability)| {
            let allowed = policy.mcp_allows(capability);
            let choices = [Decision::Deny, Decision::Allow]
                .into_iter()
                .map(|d| {
                    let allow = d == Decision::Allow;
                    let pick = self.change(move |p| p.set_mcp(capability, allow));
                    (decision_label(d, capability), allowed == allow, pick)
                })
                .collect();
            capability_row(
                ("mcp", i),
                capability,
                segmented(id_for(0, i), 24., choices),
                None,
            )
        });
        card()
            .child(card_header(
                "bot",
                "AI assistants (MCP)",
                Some("Claude and any other MCP client connected to this editor. Never prompted: a refused tool says what is missing.".into()),
                segmented("mcp-level", 26., levels).into_any_element(),
            ))
            .children(rows)
    }

    fn plugin_card(
        &self,
        index: usize,
        profile: &PluginProfile,
        policy: &Policy,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.permissions.open.contains(&profile.name);
        let caps = profile.capabilities();
        let count = |d: Decision| {
            caps.iter()
                .filter(|c| shown(policy, profile, **c) == d)
                .count()
        };
        let summary = summary(
            count(Decision::Allow),
            count(Decision::Ask),
            count(Decision::Deny),
        );
        let name = profile.name.clone();
        let toggle = cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.permissions.open.remove(&name) {
                this.permissions.open.insert(name.clone());
            }
            cx.notify();
        });
        let head = h_flex()
            .id(("plugin-card", index))
            .cursor_pointer()
            .on_click(toggle)
            .child(card_header(
                "puzzle",
                profile.name.clone(),
                Some(format!("{} \u{b7} {summary}", profile.kind.label()).into()),
                div()
                    .text_color(tokens::text3())
                    .child(icon(
                        if open {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        14.,
                    ))
                    .into_any_element(),
            ));
        card().child(head).when(open, |card| {
            card.children(caps.iter().enumerate().map(|(i, &capability)| {
                self.plugin_row(index, i, profile, capability, policy)
            }))
            .when(profile.kind != PluginKind::Wasm, |card| {
                card.child(note(
                    "Always looks at your game: a Luau plugin reaches everything through game, in Studio too.",
                ))
            })
        })
    }

    fn plugin_row(
        &self,
        card: usize,
        row: usize,
        profile: &PluginProfile,
        capability: Capability,
        policy: &Policy,
    ) -> AnyElement {
        let current = policy.decide(profile, capability);
        // Network's middle segment is "Per host" for every plugin; Ask
        // for scripts is a Roblox plugin's prompt.
        let asks = capability == Capability::Network
            || (profile.kind == PluginKind::Roblox && capability == Capability::EditScripts);
        let options: &[Decision] = if asks {
            &[Decision::Deny, Decision::Ask, Decision::Allow]
        } else {
            &[Decision::Deny, Decision::Allow]
        };
        let choices = options
            .iter()
            .map(|&d| {
                let name = profile.name.clone();
                let pick = self.change(move |p| p.set(&name, capability, Some(d)));
                (decision_label(d, capability), current == d, pick)
            })
            .collect();
        let reset = policy.stored(&profile.name, capability).map(|_| {
            let name = profile.name.clone();
            let shell = self.shell.clone();
            ghost_icon(
                ("perm-reset", card * 100 + row),
                "rotate-ccw",
                "Back to the plugin's default",
            )
            .on_click(move |_, _, cx| {
                let name = name.clone();
                shell.update(cx, |shell, cx| {
                    shell.change_permissions(|p| p.set(&name, capability, None), cx)
                })
            })
            .into_any_element()
        });
        let hosts = (capability == Capability::Network && current == Decision::Ask)
            .then(|| self.host_list(card, profile, policy));
        capability_row(
            (SharedString::from(profile.name.clone()), row),
            capability,
            h_flex()
                .gap(px(6.))
                .items_center()
                .children(reset)
                .child(segmented(id_for(card + 1, row), 24., choices)),
            hosts,
        )
        .into_any_element()
    }

    /// The hosts a plugin was answered for (or its manifest lists), each
    /// with Deny/Allow and, once answered, a forget button.
    fn host_list(&self, card: usize, profile: &PluginProfile, policy: &Policy) -> AnyElement {
        let hosts = policy.hosts(profile);
        if hosts.is_empty() {
            return text(11.5, 16.)
                .text_color(tokens::text3())
                .child(if profile.kind == PluginKind::Roblox {
                    "No host asked for yet. The first request to a host asks you."
                } else {
                    "Its plugin.toml lists no host, so every request is refused."
                })
                .into_any_element();
        }
        v_flex()
            .gap(px(6.))
            .children(hosts.into_iter().enumerate().map(|(i, host)| {
                let allowed = policy.host(profile, &host) == Decision::Allow;
                let choices = [false, true]
                    .into_iter()
                    .map(|allow| {
                        let (name, host) = (profile.name.clone(), host.clone());
                        let pick = self.change(move |p| p.set_host(&name, &host, Some(allow)));
                        (if allow { "Allow" } else { "Deny" }, allowed == allow, pick)
                    })
                    .collect();
                let forget = policy.stored_host(&profile.name, &host).map(|_| {
                    let (name, host, shell) =
                        (profile.name.clone(), host.clone(), self.shell.clone());
                    ghost_icon(
                        ("host-forget", card * 1000 + i),
                        "x",
                        "Forget, ask again next time",
                    )
                    .on_click(move |_, _, cx| {
                        let (name, host) = (name.clone(), host.clone());
                        shell.update(cx, |shell, cx| {
                            shell.change_permissions(|p| p.set_host(&name, &host, None), cx)
                        })
                    })
                });
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().flex_1().min_w_0().child(mono(11.5, 16.).child(host)))
                    .children(forget)
                    .child(segmented(host_id(card, i), 22., choices))
            }))
            .into_any_element()
    }

    /// A segment's click: one change to the store, through the shell.
    fn change(&self, change: impl Fn(&mut Policy) + 'static) -> OnPick {
        let shell = self.shell.clone();
        let change = Rc::new(change);
        Rc::new(move |_: &mut Window, cx: &mut App| {
            let change = change.clone();
            shell.update(cx, |shell, cx| shell.change_permissions(|p| change(p), cx));
        })
    }
}

/// Settings › Reset page: every agent back to its defaults.
pub(super) fn reset(shell: &mut crate::shell::Shell, cx: &mut Context<crate::shell::Shell>) {
    shell.change_permissions(|p| *p = Policy::default(), cx);
}

/// What a capability amounts to, for a card's summary: "Per host" only
/// asks for a Roblox plugin; for an rbx-native one it is its manifest's
/// hosts, allowed if there are any.
fn shown(policy: &Policy, profile: &PluginProfile, capability: Capability) -> Decision {
    match policy.decide(profile, capability) {
        Decision::Ask
            if capability == Capability::Network && profile.kind != PluginKind::Roblox =>
        {
            let any = policy
                .hosts(profile)
                .iter()
                .any(|host| policy.host(profile, host) == Decision::Allow);
            if any {
                Decision::Allow
            } else {
                Decision::Deny
            }
        }
        decision => decision,
    }
}

fn decision_label(decision: Decision, capability: Capability) -> &'static str {
    match decision {
        Decision::Allow => "Allow",
        Decision::Deny => "Deny",
        Decision::Ask if capability == Capability::Network => "Per host",
        Decision::Ask => "Ask",
    }
}

fn summary(allowed: usize, asks: usize, denied: usize) -> String {
    let mut parts = vec![format!("{allowed} allowed")];
    if asks > 0 {
        parts.push(format!("{asks} ask{}", if asks == 1 { "s" } else { "" }));
    }
    parts.push(format!("{denied} denied"));
    parts.join(", ")
}

fn id_for(card: usize, row: usize) -> SharedString {
    format!("perm-{card}-{row}").into()
}

fn host_id(card: usize, row: usize) -> SharedString {
    format!("perm-host-{card}-{row}").into()
}

fn group(title: &'static str) -> impl IntoElement {
    text(10.5, 14.)
        .font_weight(FontWeight::BOLD)
        .text_color(tokens::text3())
        .child(title.to_uppercase())
        .mb(px(-14.))
}

fn card_header(
    glyph: &'static str,
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    control: AnyElement,
) -> Div {
    h_flex()
        .w_full()
        .gap(px(12.))
        .px(px(14.))
        .py(px(12.))
        .items_center()
        .child(
            h_flex()
                .flex_none()
                .size(px(30.))
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .bg(tokens::accent_soft())
                .text_color(tokens::check_on())
                .child(icon(glyph, 15.)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    text(13., 18.)
                        .font_weight(FontWeight::BOLD)
                        .child(title.into()),
                )
                .children(subtitle.map(|s| text(12., 17.).text_color(tokens::text2()).child(s))),
        )
        .child(control)
}

fn capability_row(
    id: impl Into<ElementId>,
    capability: Capability,
    control: impl IntoElement,
    below: Option<AnyElement>,
) -> Stateful<Div> {
    v_flex()
        .id(id.into())
        .gap(px(10.))
        .px(px(14.))
        .py(px(11.))
        .border_t_1()
        .border_color(tokens::border())
        .child(
            h_flex()
                .gap(px(12.))
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .w(px(30.))
                        .flex()
                        .justify_center()
                        .text_color(tokens::text2())
                        .child(icon(capability.glyph(), 14.)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            text(12.5, 17.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(capability.label()),
                        )
                        .child(
                            text(12., 17.)
                                .text_color(tokens::text2())
                                .child(capability.description()),
                        ),
                )
                .child(control),
        )
        .children(below.map(|below| div().pl(px(42.)).child(below)))
}

fn note(text_: &'static str) -> Div {
    div()
        .px(px(14.))
        .py(px(10.))
        .border_t_1()
        .border_color(tokens::border())
        .child(text(11.5, 16.).text_color(tokens::text3()).child(text_))
}

#[cfg(test)]
mod tests;
