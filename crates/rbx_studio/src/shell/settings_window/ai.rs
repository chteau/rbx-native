//! The AI assistants page: whether the MCP server runs and a client is
//! attached, the exact setup each client takes (copied in one click, or
//! merged into Claude Desktop's config for you), and what the tools do.
//! Everything it says comes from `crate::mcp::connect` and the running
//! server; this page keeps only what was clicked.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::mcp::connect::desktop::{self, Change, Installed};
use crate::mcp::connect::probe::{self, Probe};
use crate::mcp::connect::{Client, Launch, Os, SERVER_NAME};
use crate::tokens;

use super::kit::{icon, mono, secondary_button, segmented, text, toggle, OnPick, Row, Section};
use super::nav::Page;
use super::SettingsWindow;

/// How long "Copied" shows on the copy button.
const COPIED_FOR: Duration = Duration::from_secs(2);

pub(super) struct AiControls {
    client: Client,
    launch: Launch,
    home: PathBuf,
    probe: Testing,
    installed: Option<Result<Installed, String>>,
    copied: Option<Instant>,
    /// Redraws the page once a second while it shows, for the client count
    /// and "last call" age.
    ticker: Option<Task<()>>,
}

enum Testing {
    Idle,
    Running,
    Done(Result<Probe, String>),
}

impl AiControls {
    pub(super) fn new() -> Self {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_default();
        AiControls {
            client: Client::ClaudeCode,
            launch: Launch::current(),
            home,
            probe: Testing::Idle,
            installed: None,
            copied: None,
            ticker: None,
        }
    }
}

impl SettingsWindow {
    pub(super) fn ai_page(&mut self, cx: &mut Context<Self>) -> Vec<Section> {
        self.keep_ticking(cx);
        vec![
            Section::new("Server", vec![self.server_row(cx), self.clients_row(cx)]),
            Section::new("Connect a client", self.client_rows(cx)),
            Section::new("What it can do", capabilities()),
        ]
    }

    fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ai.ticker.is_some() {
            return;
        }
        self.ai.ticker = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let showing = this.update(cx, |this, cx| {
                let showing = this.page == Page::Ai;
                if showing {
                    cx.notify();
                } else {
                    this.ai.ticker = None;
                }
                showing
            });
            if !matches!(showing, Ok(true)) {
                break;
            }
        }));
    }

    fn server_row(&self, cx: &mut Context<Self>) -> Row {
        let shell = self.shell.read(cx);
        let running = shell.mcp.as_ref().map(|server| server.endpoint().clone());
        let enabled = shell.mcp_enabled();
        let (status, color, description): (&str, Rgba, SharedString) = match &running {
            Some(endpoint) => (
                "RUNNING",
                tokens::diff_add(),
                format!(
                    "Listening on 127.0.0.1:{} for the stdio bridge \u{b7} studio {}",
                    endpoint.port, endpoint.studio_id
                )
                .into(),
            ),
            None if enabled => (
                "FAILED",
                tokens::text_error(),
                "It couldn\u{2019}t start; the Output says why.".into(),
            ),
            None => (
                "OFF",
                tokens::text3(),
                "Switch it on to let an AI client read and edit the open place.".into(),
            ),
        };
        let shell = self.shell.clone();
        let control = h_flex()
            .gap(px(12.))
            .items_center()
            .child(tag(status, color))
            .child(toggle("ai-mcp-toggle", enabled, move |_, _, cx| {
                shell.update(cx, |shell, cx| shell.set_mcp_enabled(!enabled, cx));
            }));
        Row::new("MCP server", control).describe(description)
    }

    fn clients_row(&self, cx: &mut Context<Self>) -> Row {
        let shell = self.shell.read(cx);
        let description: SharedString = match shell.mcp.as_ref() {
            None => "None while the server is off.".into(),
            Some(server) => {
                let ago = server.last_call().map(|at| age(at.elapsed()));
                match (server.clients(), ago) {
                    (0, None) => "None yet. A client attaches on its first tool call.".into(),
                    (0, Some(ago)) => format!("None attached \u{b7} last call {ago}").into(),
                    (n, ago) => {
                        let who = if n == 1 {
                            "1 attached".to_owned()
                        } else {
                            format!("{n} attached")
                        };
                        match ago {
                            Some(ago) => format!("{who} \u{b7} last call {ago}").into(),
                            None => who.into(),
                        }
                    }
                }
            }
        };
        let running = matches!(self.ai.probe, Testing::Running);
        let button = secondary_button(
            "ai-test",
            if running { "loader" } else { "plug-zap" },
            if running {
                "Testing\u{2026}"
            } else {
                "Test connection"
            },
        )
        .when(!running, |button| {
            button.on_click(cx.listener(|this, _, _, cx| this.test_connection(cx)))
        });
        let row = Row::new("Clients", button).describe(description);
        match &self.ai.probe {
            Testing::Done(outcome) => {
                let ours = shell.mcp.as_ref().map(|s| s.endpoint().studio_id.clone());
                row.below(probe_line(outcome, ours.as_deref()))
            }
            _ => row,
        }
    }

    fn test_connection(&mut self, cx: &mut Context<Self>) {
        self.ai.probe = Testing::Running;
        let exe = self.ai.launch.probe_target();
        let env = self.ai.launch.env.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move { probe::run(&exe, &env) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.ai.probe = Testing::Done(outcome);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn client_rows(&self, cx: &mut Context<Self>) -> Vec<Row> {
        let client = self.ai.client;
        let setup = client.setup(&self.ai.launch, Os::CURRENT, &self.ai.home);
        let picker = segmented(
            "ai-client",
            26.,
            Client::ALL
                .into_iter()
                .map(|each| {
                    let pick: OnPick = {
                        let this = cx.entity().downgrade();
                        std::rc::Rc::new(move |_, cx| {
                            let _ = this.update(cx, |this: &mut SettingsWindow, cx| {
                                this.ai.client = each;
                                this.ai.copied = None;
                                cx.notify();
                            });
                        })
                    };
                    (each.label(), each == client, pick)
                })
                .collect(),
        );
        let copied = self.ai.copied.is_some_and(|at| at.elapsed() < COPIED_FOR);
        let snippet = setup.snippet.clone();
        let copy = secondary_button(
            "ai-copy",
            if copied { "check" } else { "copy" },
            if copied { "Copied" } else { "Copy" },
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(snippet.clone()));
            this.ai.copied = Some(Instant::now());
            cx.notify();
        }));
        let mut rows = vec![Row::new("Client", picker)
            .describe(setup.how)
            .below(code_block(&setup.snippet, copy))];
        if let Some(file) = setup.file {
            let mut row = Row::new("Config file", div()).describe_mono(super::tilde(&file));
            if client == Client::ClaudeDesktop {
                let path = file.clone();
                row.control = secondary_button("ai-install", "plus", "Add to Claude Desktop")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let stamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |since| since.as_secs());
                        let entry = this.ai.launch.entry();
                        this.ai.installed = Some(desktop::install(&path, &entry, stamp));
                        cx.notify();
                    }))
                    .into_any_element();
                if let Some(installed) = &self.ai.installed {
                    row = row.below(install_line(installed));
                }
            }
            rows.push(row);
        }
        rows
    }
}

/// "4 s ago", "3 min ago".
fn age(elapsed: Duration) -> String {
    match elapsed.as_secs() {
        0..=59 => format!("{} s ago", elapsed.as_secs()),
        60..=3599 => format!("{} min ago", elapsed.as_secs() / 60),
        secs => format!("{} h ago", secs / 3600),
    }
}

/// The Account page's status tag: caps on a 12 % wash of its colour.
fn tag(label: &'static str, color: Rgba) -> Div {
    h_flex()
        .flex_none()
        .h(px(18.))
        .px(px(6.))
        .gap(px(5.))
        .items_center()
        .rounded(px(4.))
        .bg(Rgba { a: 0.12, ..color })
        .text_color(color)
        .text_size(px(10.5))
        .line_height(px(14.))
        .font_weight(FontWeight::BOLD)
        .child(div().size(px(6.)).rounded_full().bg(color))
        .child(label)
}

/// The snippet as it is copied, in mono on the dock colour, with its copy
/// button pinned top right.
fn code_block(snippet: &str, copy: Stateful<Div>) -> Div {
    h_flex()
        .items_start()
        .gap(px(12.))
        .p(px(12.))
        .border_1()
        .border_color(tokens::border())
        .rounded(px(6.))
        .bg(tokens::dock())
        .child(
            mono(11.5, 17.)
                .flex_1()
                .min_w_0()
                .text_color(tokens::text())
                .child(snippet.to_owned()),
        )
        .child(copy)
}

/// One status line under a row: a glyph and a sentence, in the outcome's
/// colour.
fn outcome_line(ok: bool, message: String) -> Div {
    let color = if ok {
        tokens::diff_add()
    } else {
        tokens::text_error()
    };
    h_flex()
        .gap(px(8.))
        .items_start()
        .child(
            div()
                .pt(px(1.))
                .text_color(color)
                .child(icon(if ok { "circle-check" } else { "circle-alert" }, 14.)),
        )
        .child(
            text(11.5, 16.)
                .flex_1()
                .min_w_0()
                .text_color(tokens::text2())
                .child(message),
        )
}

fn probe_line(outcome: &Result<Probe, String>, ours: Option<&str>) -> Div {
    match outcome {
        Err(error) => outcome_line(false, format!("The bridge didn\u{2019}t work: {error}.")),
        Ok(probe) => {
            let reached = ours.is_some_and(|id| probe.studios.iter().any(|s| s == id));
            let editors = match probe.studios.len() {
                1 => "1 editor".to_owned(),
                n => format!("{n} editors"),
            };
            if reached {
                outcome_line(
                    true,
                    format!(
                        "The bridge answered with {} tools and reached {editors}, this one \
                         included. A client set up as below will work.",
                        probe.tools
                    ),
                )
            } else {
                outcome_line(
                    false,
                    format!(
                        "The bridge answered with {} tools but didn\u{2019}t reach this editor \
                         ({editors} found). Is the MCP server running here?",
                        probe.tools
                    ),
                )
            }
        }
    }
}

fn install_line(installed: &Result<Installed, String>) -> Div {
    let installed = match installed {
        Err(error) => return outcome_line(false, error.clone()),
        Ok(installed) => installed,
    };
    let kept = match installed.others {
        0 => String::new(),
        1 => " Your other server was kept.".to_owned(),
        n => format!(" Your {n} other servers were kept."),
    };
    let backup = installed
        .backup
        .as_ref()
        .map(|backup| format!(" The old file is saved as {}.", super::tilde(backup)))
        .unwrap_or_default();
    let message = match &installed.change {
        Change::Unchanged => {
            format!("{SERVER_NAME} is already set up there; nothing changed.")
        }
        Change::Added => {
            format!("Added {SERVER_NAME}.{kept}{backup} Quit and reopen Claude Desktop to load it.")
        }
        Change::Replaced(old) => format!(
            "Updated {SERVER_NAME} (it ran {}).{kept}{backup} Quit and reopen Claude Desktop \
             to load it.",
            old["command"].as_str().unwrap_or("something else")
        ),
    };
    outcome_line(true, message)
}

/// The tools, grouped by what a user would ask for, with how many each
/// group holds (29 in all, as `tools/list` answers).
const GROUPS: [(&str, &str, &str, usize); 8] = [
    (
        "Explore the place",
        "search",
        "Search the tree, inspect any instance\u{2019}s properties, tell open editors apart.",
        4,
    ),
    (
        "Read and edit scripts",
        "file-code",
        "Read, search and grep every script. Each edit is an undo step of its own.",
        4,
    ),
    (
        "Run Luau",
        "terminal",
        "Run code in the edited place, or in a playtest\u{2019}s client or server.",
        1,
    ),
    (
        "Playtest",
        "play",
        "Start and stop Play, read the Output, capture the viewport, walk the character, \
         type and click.",
        6,
    ),
    (
        "Assets",
        "image",
        "Search the Creator Store, insert models, upload and store images.",
        4,
    ),
    (
        "Roblox docs and guides",
        "book-open",
        "Read creator-docs pages, and guides on scripting, UI style, monetization, \
         performance and more.",
        2,
    ),
    (
        "Helper agents and saving",
        "bot",
        "Hand a task to a helper on your client\u{2019}s own model (when it supports \
         sampling), and save the place.",
        2,
    ),
    (
        "Studio\u{2019}s AI generation",
        "sparkles",
        "Listed so prompts written for Studio still work. Roblox has no public API for \
         them, so each says why it can\u{2019}t run here.",
        6,
    ),
];

fn capabilities() -> Vec<Row> {
    GROUPS
        .into_iter()
        .map(|(label, glyph, description, tools)| {
            let count = if tools == 1 {
                "1 tool".to_owned()
            } else {
                format!("{tools} tools")
            };
            Row::new(
                label,
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .text_color(tokens::text3())
                    .child(text(11.5, 16.).child(count))
                    .child(icon(glyph, 14.)),
            )
            .describe(description)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{age, GROUPS};
    use std::time::Duration;

    #[test]
    fn the_groups_add_up_to_every_tool_listed() {
        let listed: usize = GROUPS.iter().map(|group| group.3).sum();
        assert_eq!(listed, crate::mcp::tool_count());
    }

    #[test]
    fn ages_read_in_the_largest_whole_unit() {
        assert_eq!(age(Duration::from_secs(4)), "4 s ago");
        assert_eq!(age(Duration::from_secs(185)), "3 min ago");
        assert_eq!(age(Duration::from_secs(7300)), "2 h ago");
    }
}
