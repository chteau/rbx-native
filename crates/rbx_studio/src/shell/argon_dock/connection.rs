//! The Argon dock's connection column: the identity row with the
//! connection's status, one line saying what's going on, and (in
//! `actions`) the row of controls that changes it, with the help popover
//! (`help`) behind "?".

use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::argon_sync::{SyncDirection, SyncState};
use super::super::Shell;
use super::Layout;

/// The two halves of `host:port`, with the plugin's defaults for whatever
/// is missing (`Config.luau:41-42`).
pub(super) fn split_address(address: &str) -> (String, String) {
    let address = address.trim();
    if address.is_empty() {
        return ("localhost".to_owned(), "8000".to_owned());
    }
    match address.split_once(':') {
        Some((host, port)) => (host.trim().to_owned(), port.trim().to_owned()),
        None => (address.to_owned(), "8000".to_owned()),
    }
}

/// `"Ns ago"` / `"Nm ago"` / `"Nh ago"` / `"Nd ago"`, the coarse readout
/// the plugin's Connected page shows.
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

/// The installed CLI's version as `vX.Y.Z`, or `None` when there is no
/// CLI. Asked of the binary the file lookup found, once at startup: the
/// badge next to the dock's title is the only thing that needs it.
pub(crate) fn detect_argon_version() -> Option<SharedString> {
    let binary = super::super::argon_sync::argon_cli_path()?;
    let output = std::process::Command::new(binary)
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

/// What the column is showing, read off `SyncState` and the pending
/// review once per frame.
pub(super) enum View {
    Disconnected,
    Connecting {
        address: String,
    },
    Connected {
        project: String,
        synced: String,
        direction: SyncDirection,
    },
    Error {
        message: String,
    },
    Review {
        project: String,
        additions: usize,
        updates: usize,
        removals: usize,
    },
}

impl Shell {
    fn argon_view(&self, cx: &App) -> View {
        if let Some(pending) = self.argon_pending() {
            let project = match self.argon_state() {
                SyncState::Connected { project, .. } => project.clone(),
                _ => String::new(),
            };
            return View::Review {
                project,
                additions: pending.additions,
                updates: pending.updates,
                removals: pending.removals,
            };
        }
        match self.argon_state() {
            SyncState::NotConnected => View::Disconnected,
            SyncState::Connecting => View::Connecting {
                address: self.argon_ui.address(cx),
            },
            SyncState::Connected {
                project,
                last_sync,
                direction,
                ..
            } => View::Connected {
                project: project.clone(),
                synced: match last_sync {
                    Some(at) => format!("Synced {}", format_elapsed(*at)),
                    None => "Synced".to_owned(),
                },
                direction: *direction,
            },
            SyncState::Error(message) => View::Error {
                message: message.clone(),
            },
        }
    }

    /// The whole column: identity row, the line, the action row.
    pub(super) fn argon_connection(&mut self, layout: Layout, cx: &mut Context<Self>) -> Div {
        let view = self.argon_view(cx);
        let version = self.argon_ui.version.clone();
        v_flex()
            .flex_none()
            .gap(px(14.))
            .child(identity_row(&view, version))
            .child(status_line(&view, layout))
            .child(self.action_row(&view, layout, cx))
    }
}

/// 28px: the accent mark, "Argon", the CLI version, and the status.
fn identity_row(view: &View, version: Option<SharedString>) -> Div {
    h_flex()
        .h(px(28.))
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .flex_none()
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS_TILE)
                .bg(tokens::accent_soft())
                .text_color(tokens::check_on())
                .child(Icon::new(IconName::RefreshCw).size(px(16.))),
        )
        .child(
            div()
                .text_size(tokens::text_lg())
                .line_height(tokens::line_lg())
                .font_weight(tokens::WEIGHT_BOLD)
                .text_color(tokens::text())
                .child("Argon"),
        )
        .child(div().flex_none().children(version.map(|version| {
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded(tokens::RADIUS_BADGE)
                .border_1()
                .border_color(tokens::border2())
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text2())
                .child(version)
        })))
        .child(div().flex_1())
        .child(status(view))
}

fn status(view: &View) -> AnyElement {
    let badge = |fill: Rgba, ink: Rgba| {
        h_flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(2.))
            .rounded(tokens::RADIUS_BADGE)
            .bg(fill)
            .text_size(tokens::text_badge())
            .line_height(tokens::line_badge())
            .font_weight(tokens::WEIGHT_SEMIBOLD)
            .text_color(ink)
    };
    let dot = || {
        div()
            .flex_none()
            .size(px(6.))
            .rounded_full()
            .bg(tokens::check_on())
    };
    match view {
        View::Disconnected => h_flex()
            .items_center()
            .gap(px(6.))
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text2())
            .child(Icon::new(IconName::CircleCheck).size(px(13.)))
            .child("Found on PATH")
            .into_any_element(),
        View::Connecting { .. } => badge(tokens::hover(), tokens::text2())
            .child(Icon::new(IconName::LoaderCircle).size(px(12.)))
            .child("Connecting")
            .into_any_element(),
        View::Connected { .. } => badge(tokens::accent_soft(), tokens::check_on())
            .child(dot())
            .child("Connected")
            .into_any_element(),
        View::Error { .. } => badge(tokens::error_soft(), tokens::text_error())
            .child(Icon::new(IconName::CircleAlert).size(px(12.)))
            .child("Error")
            .into_any_element(),
        View::Review { .. } => badge(tokens::accent_soft(), tokens::check_on())
            .child(dot())
            .child("Review changes")
            .into_any_element(),
    }
}

/// The 18px line under the identity row. Only the disconnected
/// description may wrap, and only in the stacked layout.
fn status_line(view: &View, layout: Layout) -> Div {
    let line = h_flex()
        .items_center()
        .text_size(tokens::text_md())
        .line_height(tokens::line_md_tall())
        .text_color(tokens::text2());
    let separator = || {
        div()
            .flex_none()
            .size(px(3.))
            .rounded_full()
            .bg(tokens::text3())
    };
    let strong = |s: String| {
        div()
            .font_weight(tokens::WEIGHT_SEMIBOLD)
            .text_color(tokens::text())
            .child(s)
    };
    let count = |n: usize, singular: &str, plural: &str| {
        h_flex()
            .child(strong(n.to_string()))
            .child(format!(" {}", if n == 1 { singular } else { plural }))
    };
    match view {
        // The one line that may wrap, in the stacked layout: a block, not a
        // row, so the text breaks at the dock's width.
        View::Disconnected => line.when(layout.wide, |this| this.h(px(18.))).child(
            div()
                .w_full()
                .when(layout.wide, |this| this.whitespace_nowrap())
                .child("Two-way sync with an Argon project, at the address its CLI serves."),
        ),
        View::Connecting { address } => line
            .h(px(18.))
            .child("Reaching the Argon server at\u{a0}")
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text())
                    .child(address.clone()),
            )
            .child("…"),
        View::Connected {
            project,
            synced,
            direction,
        } => line
            .h(px(18.))
            .gap(px(10.))
            .child(strong(project.clone()))
            .child(separator())
            .child(
                h_flex()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        Icon::new(match direction {
                            SyncDirection::Down => IconName::ArrowDown,
                            SyncDirection::Up => IconName::ArrowUp,
                        })
                        .size(px(12.)),
                    )
                    .child(synced.clone()),
            ),
        View::Error { message } => line
            .h(px(18.))
            .text_color(tokens::text_error())
            .child(div().truncate().child(message.clone())),
        View::Review {
            project,
            additions,
            updates,
            removals,
        } => line
            .h(px(18.))
            .gap(px(10.))
            .child(strong(project.clone()))
            .child(separator())
            .child(
                h_flex()
                    .child(count(*additions, "addition", "additions"))
                    .child(", ")
                    .child(count(*updates, "update", "updates"))
                    .child(", ")
                    .child(count(*removals, "removal", "removals")),
            ),
    }
}
