//! The Script Editor's own docks: Argon (`argon-rbx/argon`) two-way file
//! sync and the Wally (`UpliftGames/wally`) package manager — the two
//! scripting tools `ROADMAP.md` names, seated beside Output so a scripter
//! never has to leave the bottom edge to reach them.
//!
//! Both are real clients, not placeholders. Argon's talks
//! `argon-rbx/argon`'s sync protocol (`crate::argon_client`, wired in here
//! by `shell::argon_sync`): Connect really opens an HTTP connection to a
//! locally-running `argon serve`, and the dock's states below
//! (`NotConnected`/`Connecting`/`Connected`/`Error`, plus the batch review
//! prompt) mirror Argon's own Studio plugin's state machine — drawn with
//! this editor's own chrome, tokens and `Button`, not Argon's. Wally's
//! searches the real `api.wally.run` registry and resolves a picked
//! result's whole dependency graph (`crate::wally_client`, wired in here by
//! `shell::wally_sync`).
//!
//! Shown only while the Script Editor document is up — see
//! `Shell::hidden_panels` — since neither means anything over the 3D view
//! or the UI Editor's canvas.

use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::*;

use crate::tokens;
use crate::wally_client;

use super::argon_sync::{SyncDirection, SyncState};
use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::wally_sync;
use super::workspace::search_field;
use super::Shell;

impl Shell {
    pub(super) fn argon_dock(
        &self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::ArgonOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "argon-overflow",
                IconName::Ellipsis,
                "Argon settings",
            )),
            self.move_items(Panel::Argon),
            cx,
        );

        let pending = self
            .argon_pending()
            .map(|p| (p.additions, p.updates, p.removals));
        let body: AnyElement = if let Some((additions, updates, removals)) = pending {
            let accept = Button::new("argon-accept")
                .label("Accept")
                .primary()
                .xsmall()
                .on_click(cx.listener(|shell, _, _, cx| shell.argon_accept_pending(cx)));
            let cancel = Button::new("argon-cancel")
                .label("Cancel")
                .outline()
                .xsmall()
                .on_click(cx.listener(|shell, _, _, cx| shell.argon_cancel_pending(cx)));
            let diff = Button::new("argon-diff")
                .label("Diff")
                .outline()
                .xsmall()
                .on_click(cx.listener(|shell, _, _, cx| shell.open_argon_diff(cx)));
            review_prompt_body(additions, updates, removals, diff, accept, cancel)
                .into_any_element()
        } else {
            match self.argon_state() {
                SyncState::NotConnected => {
                    let version = self.argon_version.clone();
                    let tab_index = self.tab_order.next();
                    let connect =
                        field_button(Button::new("argon-connect").label("Connect").primary())
                            .on_click(cx.listener(|shell, _, _, cx| shell.argon_connect(cx)));
                    not_connected_body(version, &self.argon_address, tab_index, connect, cx)
                        .into_any_element()
                }
                SyncState::Connecting => connecting_body().into_any_element(),
                SyncState::Connected {
                    project,
                    address,
                    last_sync,
                    direction,
                } => {
                    let disconnect = field_button(
                        Button::new("argon-disconnect")
                            .label("Disconnect")
                            .outline(),
                    )
                    .on_click(cx.listener(|shell, _, _, cx| shell.argon_disconnect(cx)));
                    connected_body(project, address, *last_sync, *direction, disconnect)
                        .into_any_element()
                }
                SyncState::Error(message) => {
                    let dismiss =
                        field_button(Button::new("argon-dismiss").label("Dismiss").outline())
                            .on_click(cx.listener(|shell, _, _, cx| shell.argon_disconnect(cx)));
                    error_body(message, dismiss).into_any_element()
                }
            }
        };

        (
            Some(overflow.into_any_element()),
            Some(chrome::dock_content(body).into_any_element()),
        )
    }

    pub(super) fn wally_dock(
        &self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::WallyOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "wally-overflow",
                IconName::Package,
                "Wally settings",
            )),
            self.move_items(Panel::Wally),
            cx,
        );

        let tab_index = self.tab_order.next();
        // Capped rather than scrolled — a dock this short has no real room
        // for a long list anyway, and a search narrows results faster than
        // scrolling would.
        let rows: Vec<AnyElement> = self
            .wally_results()
            .iter()
            .take(10)
            .enumerate()
            .map(|(index, result)| {
                let picked = result.clone();
                wally_result_row(
                    ("wally-result", index),
                    result,
                    cx.listener(move |shell, _, _, cx| {
                        shell.wally_install(picked.clone(), cx);
                    }),
                )
                .into_any_element()
            })
            .collect();

        let body = v_flex()
            .size_full()
            .gap(tokens::group_gap())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .child(div().w_full().max_w(px(260.)).child(search_field(
                tab_index,
                &self.wally_query,
                cx,
            )))
            .children(wally_status(self.wally_install_state()))
            .child(v_flex().gap(px(2.)).children(rows));

        (
            Some(overflow.into_any_element()),
            Some(chrome::dock_content(body).into_any_element()),
        )
    }
}

/// One search result: `scope/name`, its description if it has one, a
/// click target for the whole row.
fn wally_result_row(
    id: impl Into<ElementId>,
    result: &wally_client::SearchResult,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    v_flex()
        .id(id.into())
        .w_full()
        .rounded(tokens::RADIUS)
        .px(tokens::label_gap())
        .py(px(4.))
        .cursor_pointer()
        .hover(|this| this.bg(tokens::hover()))
        .on_click(on_click)
        .child(
            div()
                .text_color(tokens::text_label())
                .child(format!("{}/{}", result.scope, result.name)),
        )
        .children(result.description.clone().map(|description| {
            div()
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text_muted())
                .child(description)
        }))
}

fn wally_status(state: &wally_sync::InstallState) -> Option<AnyElement> {
    Some(match state {
        wally_sync::InstallState::Idle => return None,
        wally_sync::InstallState::Installing { name } => {
            status_row(IconName::LoaderCircle, format!("Installing {name}…")).into_any_element()
        }
        wally_sync::InstallState::Installed { name, count } => {
            let text = match count {
                1 => format!("Installed {name}"),
                n => format!("Installed {name} and {} more", n - 1),
            };
            status_row(IconName::CircleCheck, text).into_any_element()
        }
        wally_sync::InstallState::Error(message) => v_flex()
            .gap(tokens::label_gap())
            .child(status_row(IconName::CircleAlert, "Couldn't install"))
            .child(
                div()
                    .text_color(tokens::text_error())
                    .child(message.clone()),
            )
            .into_any_element(),
    })
}

/// A button the same height and horizontal padding as `workspace::
/// search_field`, so an action sitting beside a field — or alone under one
/// — reads as the frame's own chrome instead of the toolkit's default
/// button box.
fn field_button(button: Button) -> Button {
    button.xsmall().h(tokens::input_height()).px(px(8.))
}

fn status_row(icon: IconName, status: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(tokens::label_gap())
        .child(Icon::new(icon).small().text_color(tokens::text_muted()))
        .child(div().text_color(tokens::text_label()).child(status.into()))
}

/// Disconnected: whether the CLI was actually found (see
/// [`detect_argon_version`]), the address its Studio plugin would connect
/// to (real, editable, local to this window), and Connect.
fn not_connected_body(
    version: Option<SharedString>,
    address: &Entity<InputState>,
    tab_index: isize,
    connect: Button,
    cx: &App,
) -> impl IntoElement {
    let (status_icon, status_text): (IconName, SharedString) = match &version {
        Some(version) => (
            IconName::CircleCheck,
            format!("Argon {version} found on PATH").into(),
        ),
        None => (
            IconName::CircleAlert,
            "Argon isn't installed — get it at argon.wiki".into(),
        ),
    };

    v_flex()
        .size_full()
        .gap(tokens::group_gap())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(status_row(status_icon, status_text))
        .child(
            div()
                .text_color(tokens::text_muted())
                .child("Two-way sync with an Argon project, at the address its CLI serves."),
        )
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(tokens::label_gap())
                .child(
                    div()
                        .flex_1()
                        .max_w(px(220.))
                        .child(search_field(tab_index, address, cx)),
                )
                .child(connect),
        )
}

fn connecting_body() -> impl IntoElement {
    v_flex()
        .size_full()
        .gap(tokens::group_gap())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(status_row(IconName::LoaderCircle, "Connecting…"))
}

/// Connected: the project name, the address, which way the last sync ran,
/// and how long ago — live, ticking up every poll tick while this state
/// holds (see `Shell::drain_argon_events`) — and Disconnect.
fn connected_body(
    project: &str,
    address: &str,
    last_sync: Option<Instant>,
    direction: SyncDirection,
    disconnect: Button,
) -> impl IntoElement {
    let arrow = match direction {
        SyncDirection::Down => "↓",
        SyncDirection::Up => "↑",
    };
    let synced = match last_sync {
        Some(at) => format!("{arrow} Synced {}", format_elapsed(at)),
        None => format!("{arrow} Synced"),
    };

    v_flex()
        .size_full()
        .gap(tokens::group_gap())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_color(tokens::text_strong())
                        .child(project.to_owned()),
                )
                .child(div().text_color(tokens::text_muted()).child(synced)),
        )
        .child(
            div()
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_color(tokens::text_muted())
                .child(address.to_owned()),
        )
        .child(disconnect)
}

fn error_body(message: &str, dismiss: Button) -> impl IntoElement {
    v_flex()
        .size_full()
        .gap(tokens::group_gap())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(status_row(IconName::CircleAlert, "Argon"))
        .child(
            div()
                .text_color(tokens::text_error())
                .child(message.to_owned()),
        )
        .child(dismiss)
}

/// Argon's own `argon-roblox` plugin shows this as a floating dialog
/// ("There will be N additions/updates/removals applied compared to the
/// server", Cancel/Diff/Accept) once an incoming batch crosses its
/// `Config.ChangesThreshold`; this dock shows the same copy inline, plus a
/// real Diff button opening `shell::argon_diff_window` — a second window
/// rather than a layer over the dock, the same shape as the
/// `ColorSequence`/`NumberSequence` graph, because a batch's own
/// before/after property values need real room to read.
fn review_prompt_body(
    additions: usize,
    updates: usize,
    removals: usize,
    diff: Button,
    accept: Button,
    cancel: Button,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .gap(tokens::group_gap())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(status_row(IconName::CircleAlert, "Review before applying"))
        .child(
            div().text_color(tokens::text_muted()).child(format!(
                "There will be {additions} additions, {updates} updates and {removals} removals applied."
            )),
        )
        .child(
            h_flex()
                .gap(tokens::label_gap())
                .child(accept)
                .child(cancel)
                .child(diff),
        )
}

/// `"Ns ago"` / `"Nm ago"` / `"Nh ago"` / `"Nd ago"` — the same coarse
/// steps `argon-roblox`'s own `Pages/Connected.luau` uses for its live
/// readout.
fn format_elapsed(at: Instant) -> String {
    let secs = at.elapsed().as_secs();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

/// Whether the `argon` CLI is on PATH, and the version it reports — probed
/// once at startup (`Shell::new`) and cached, never re-run per frame. A
/// machine without it, or a build that prints something this can't parse,
/// answers "not found" rather than failing: the dock reads that as "isn't
/// installed", which for anything this can't confirm is the honest answer.
pub(super) fn detect_argon_version() -> Option<SharedString> {
    let output = std::process::Command::new("argon")
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.trim().rsplit(' ').next()?.trim_start_matches('v');
    if version.is_empty() {
        return None;
    }
    Some(format!("v{version}").into())
}
