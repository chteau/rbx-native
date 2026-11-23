//! The Leak Finder dock (a Local Run beta): what keeps growing in a
//! running game, the server and each client apart. `rbx_lua::leaks` does
//! the counting; this shows it: a timeline per category (connections,
//! instances out of the place a script holds, entries of the largest
//! tables, the Luau heap), what looks like a leak with the script line
//! behind it, the heap and connections per script, and a diff between the
//! last two snapshots taken.
//!
//! Tracking costs nothing while the dock is shut: the session's owner
//! keeps a peer's tracking alive each frame only while a dock showing that
//! peer is open (this window's for every peer, a test tab's for its own).

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_lua::leaks::{self, Kind, Sample, Site, Suspect};
use rbx_sim::{Peer, Simulation};

use super::chrome;
use super::layout::{Home, Panel};
use super::menu::{self, MenuId};
use super::output::game::{Context as Side, Location};
use super::Shell;
use crate::launcher::ui;
use crate::tokens;

/// Samples the timelines show: three minutes.
const SHOWN: usize = 90;
const CHART_H: f32 = 28.0;
/// Snapshots kept per peer.
const SNAPSHOTS: usize = 8;
const ROWS: usize = 40;

impl LeakDock {
    fn snapshots(&self, peer: Peer) -> &[Sample] {
        self.snapshots
            .iter()
            .find(|(p, _)| *p == peer)
            .map_or(&[], |(_, kept)| kept.as_slice())
    }
}

#[derive(Default)]
pub(crate) struct LeakDock {
    /// The peer shown in a window that owns the session (a test tab shows
    /// its own); the Server when `None`.
    peer: Option<Peer>,
    snapshots: Vec<(Peer, Vec<Sample>)>,
    /// Shows the last two snapshots' diff.
    compare: bool,
}

fn peer_label(peer: Peer) -> String {
    match peer {
        Peer::Server => "Server".into(),
        Peer::Client(i) => format!("Client {}", i + 1),
    }
}

fn small(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .text_size(tokens::text_xs())
        .text_color(tokens::text2())
        .child(text.into())
}

fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .px(px(8.))
        .pt(px(6.))
        .text_size(tokens::text_xs())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text3())
        .child(text.into())
}

fn toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let weight = if on {
        ui::Weight::Primary
    } else {
        ui::Weight::Ghost
    };
    ui::button(id, label, weight, true)
}

fn kib(bytes: usize) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.2} MB", b as f64 / 1_048_576.0),
        b => format!("{:.1} KB", b as f64 / 1024.0),
    }
}

fn kind_color(kind: Kind) -> Rgba {
    match kind {
        Kind::Signals => ui::accent(),
        Kind::Instances => tokens::tool_scale(),
        Kind::Tables => tokens::warning(),
    }
}

/// The four timelines: name, colour, the figure of a sample, its text.
type Series = (&'static str, Rgba, fn(&Sample) -> f64, fn(f64) -> String);

fn series() -> [Series; 4] {
    [
        (
            "Signals",
            ui::accent(),
            |s| s.connections as f64,
            |v| format!("{v:.0}"),
        ),
        (
            "Instances",
            tokens::tool_scale(),
            |s| s.held_instances() as f64,
            |v| format!("{v:.0} held"),
        ),
        (
            "Luau tables",
            tokens::warning(),
            |s| s.table_entries() as f64,
            |v| format!("{v:.0} entries"),
        ),
        (
            "LuaHeap",
            tokens::tool_rotate(),
            |s| s.heap as f64 / 1_048_576.0,
            |v| format!("{v:.2} MB"),
        ),
    ]
}

/// A bar per sample, scaled to the largest shown.
fn sparkline(values: &[f64], color: Rgba) -> Div {
    let max = values.iter().copied().fold(0.0, f64::max).max(1e-9);
    h_flex()
        .flex_1()
        .h(px(CHART_H))
        .items_end()
        .gap(px(1.))
        .children(values.iter().map(|v| {
            div()
                .flex_1()
                .max_w(px(6.))
                .h(px(((v / max) as f32 * CHART_H).max(1.)))
                .bg(color)
        }))
}

impl Shell {
    /// Whether this window's Leak Finder dock is open (shown or behind
    /// another tab).
    pub(in crate::shell) fn leak_finder_open(&self) -> bool {
        crate::beta::enabled(super::local_run::BETA_ID)
            && !matches!(self.layout.home_of(Panel::LeakFinder), Home::Closed)
    }

    /// Each frame of the session this window owns: its dock tracks every
    /// peer while open.
    pub(in crate::shell) fn leak_finder_keep_alive(&self) {
        if !self.leak_finder_open() {
            return;
        }
        if let Some(session) = self.local_run_session() {
            for peer in session_peers(session) {
                if let Some(sim) = session.sim(peer) {
                    sim.keep_leak_tracking();
                }
            }
        }
    }

    /// The peers this dock can show: every one of the session this window
    /// owns, else the one its test tab is on.
    fn leak_peers(&self, cx: &App) -> Vec<Peer> {
        if let Some(session) = self.local_run_session() {
            return session_peers(session);
        }
        match &self.play.live.local.peer {
            Some(link) => {
                let (host, view) = link.shown_peer();
                host.upgrade()
                    .filter(|h| h.read(cx).local_run_session().is_some())
                    .map(|_| vec![view])
                    .unwrap_or_default()
            }
            None => Vec::new(),
        }
    }

    fn leak_with_sim<R>(
        &self,
        peer: Peer,
        cx: &App,
        f: impl FnOnce(&Simulation) -> R,
    ) -> Option<R> {
        if let Some(session) = self.local_run_session() {
            return session.sim(peer).map(f);
        }
        let (host, _) = self.play.live.local.peer.as_ref()?.shown_peer();
        let host = host.upgrade()?;
        let host = host.read(cx);
        host.local_run_session()?.sim(peer).map(f)
    }

    fn leak_take_snapshot(&mut self, peer: Peer, cx: &mut Context<Self>) {
        let Some(sample) = self.leak_with_sim(peer, cx, Simulation::leak_sample) else {
            return;
        };
        let at = match self.leaks.snapshots.iter().position(|(p, _)| *p == peer) {
            Some(at) => at,
            None => {
                self.leaks.snapshots.push((peer, Vec::new()));
                self.leaks.snapshots.len() - 1
            }
        };
        let kept = &mut self.leaks.snapshots[at].1;
        if kept.len() == SNAPSHOTS {
            kept.remove(0);
        }
        kept.push(sample);
        cx.notify();
    }

    fn leak_reveal(
        &mut self,
        peer: Peer,
        site: &Site,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let side = match peer {
            Peer::Server => Side::Server,
            Peer::Client(_) => Side::Client,
        };
        let location = Location {
            path: site.script.clone(),
            line: site.line.max(1) as u32,
        };
        self.play_reveal(side, &location, window, cx);
    }

    pub(in crate::shell) fn leak_finder_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::LeakFinderOverflow,
            chrome::dock_options_trigger(
                "leak-finder-overflow",
                IconName::Ellipsis,
                16.,
                "Leak Finder options",
            ),
            self.move_items(Panel::LeakFinder),
            cx,
        );
        let peers = self.leak_peers(cx);
        let peer = match self.leaks.peer {
            Some(peer) if peers.contains(&peer) => peer,
            _ => peers.first().copied().unwrap_or(Peer::Server),
        };
        let history: Option<Vec<Sample>> = self.leak_with_sim(peer, cx, |sim| {
            sim.leak_tracker()
                .map(|t| t.borrow().history().iter().cloned().collect())
                .unwrap_or_default()
        });
        let Some(history) = history else {
            self.leaks.snapshots.clear();
            let empty = div()
                .p(px(12.))
                .text_size(tokens::text_sm())
                .text_color(tokens::text2())
                .child(
                    "Start Run (local) or Play (local) to watch it for leaks: connections left \
                     on instances that left the place, nil-parented instances piling up, tables \
                     that only grow and each script's Luau heap, for the server and every \
                     client apart.",
                );
            return (
                Some(overflow.into_any_element()),
                Some(empty.into_any_element()),
            );
        };
        let suspects = leaks::suspects(&history.iter().cloned().collect());
        let latest = history.last().cloned();
        let body = v_flex()
            .id("leak-finder-dock")
            .size_full()
            .gap_1()
            .pb_2()
            .overflow_y_scroll()
            .child(self.leak_toolbar(&peers, peer, cx))
            .child(self.leak_timelines(&history))
            .child(heading(format!("SUSPECTS ({})", suspects.len())))
            .child(self.leak_suspects(peer, &suspects, cx))
            .when(self.leaks.compare, |this| {
                this.child(heading("COMPARE WITH PREVIOUS SNAPSHOT"))
                    .child(self.leak_diff(peer, cx))
            })
            .child(heading("PER SCRIPT"))
            .child(self.leak_scripts(peer, latest.as_ref(), cx))
            .child(self.leak_notes(latest.as_ref()));
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn leak_toolbar(&self, peers: &[Peer], peer: Peer, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshots = self.leaks.snapshots(peer).len();
        h_flex()
            .px(px(8.))
            .pt(px(6.))
            .gap(px(6.))
            .items_center()
            .flex_wrap()
            .when(peers.len() > 1, |this| {
                this.children(peers.iter().map(|&p| {
                    toggle(
                        SharedString::from(format!("leak-peer-{}", peer_label(p))),
                        peer_label(p),
                        p == peer,
                    )
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.leaks.peer = Some(p);
                        cx.notify();
                    }))
                }))
            })
            .when(peers.len() == 1, |this| this.child(small(peer_label(peer))))
            .child(div().flex_1())
            .child(small(format!(
                "{snapshots} snapshot{}",
                if snapshots == 1 { "" } else { "s" }
            )))
            .child(
                toggle("leak-compare", "Compare", self.leaks.compare).on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.leaks.compare = !shell.leaks.compare;
                        cx.notify();
                    },
                )),
            )
            .child(
                ui::button("leak-snapshot", "Take snapshot", ui::Weight::Primary, true).on_click(
                    cx.listener(move |shell, _, _, cx| shell.leak_take_snapshot(peer, cx)),
                ),
            )
    }

    fn leak_timelines(&self, history: &[Sample]) -> impl IntoElement {
        let shown = &history[history.len().saturating_sub(SHOWN)..];
        v_flex()
            .px(px(8.))
            .gap(px(4.))
            .children(series().map(|(name, color, value, text)| {
                let values: Vec<f64> = shown.iter().map(value).collect();
                let now = values
                    .last()
                    .copied()
                    .map(text)
                    .unwrap_or_else(|| "—".into());
                h_flex()
                    .gap(px(8.))
                    .items_end()
                    .child(
                        v_flex()
                            .w(px(96.))
                            .flex_none()
                            .child(
                                div()
                                    .text_size(tokens::text_xs())
                                    .text_color(tokens::text())
                                    .child(name),
                            )
                            .child(small(now)),
                    )
                    .child(sparkline(&values, color))
            }))
    }

    fn leak_suspects(
        &self,
        peer: Peer,
        suspects: &[Suspect],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if suspects.is_empty() {
            return v_flex().px(px(8.)).child(small(format!(
                "Nothing keeps growing. A figure has to rise at {} samples in a row ({} s of \
                 game time) to show here; a connection, once its instance has been out of the \
                 place for two.",
                leaks::GROWING,
                leaks::GROWING as f64 * leaks::SAMPLE_EVERY
            )));
        }
        v_flex()
            .px(px(8.))
            .gap(px(2.))
            .children(suspects.iter().take(ROWS).enumerate().map(|(i, suspect)| {
                let growth = match suspect.growth {
                    0 => String::new(),
                    g => format!("  +{g}"),
                };
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .w(px(72.))
                            .flex_none()
                            .text_size(tokens::text_xs())
                            .text_color(kind_color(suspect.kind))
                            .child(suspect.kind.label()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text())
                            .child(suspect.what.clone()),
                    )
                    .child(small(format!("×{}{growth}", suspect.count)))
                    .child(self.leak_site_link(peer, ("leak-suspect", i), suspect.site.clone(), cx))
            }))
    }

    /// `Script:line`, opening that line on a click.
    fn leak_site_link(
        &self,
        peer: Peer,
        id: impl Into<ElementId>,
        site: Option<Site>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(site) = site.filter(|s| s.script.contains('.')) else {
            return small(String::new()).into_any_element();
        };
        div()
            .id(id)
            .flex_none()
            .text_size(tokens::text_xs())
            .text_color(ui::accent())
            .cursor_pointer()
            .child(site.short())
            .on_click(cx.listener(move |shell, _, window, cx| {
                shell.leak_reveal(peer, &site, window, cx);
            }))
            .into_any_element()
    }

    fn leak_diff(&self, peer: Peer, cx: &mut Context<Self>) -> impl IntoElement {
        let kept = self.leaks.snapshots(peer);
        let [.., before, after] = kept else {
            return v_flex()
                .px(px(8.))
                .child(small("Take two snapshots to compare them."));
        };
        let rows = leaks::diff(before, after);
        let header = small(format!(
            "{:.0} s → {:.0} s of game time · LuaHeap {} → {}",
            before.time,
            after.time,
            kib(before.heap),
            kib(after.heap)
        ));
        v_flex()
            .px(px(8.))
            .gap(px(2.))
            .child(header)
            .when(rows.is_empty(), |this| {
                this.child(small("Nothing changed."))
            })
            .children(rows.into_iter().take(ROWS).enumerate().map(|(i, row)| {
                let delta = row.delta();
                let (before, after) = match row.category {
                    "LuaHeap" => (kib(row.before), kib(row.after)),
                    _ => (row.before.to_string(), row.after.to_string()),
                };
                let script = row
                    .site
                    .as_ref()
                    .filter(|s| s.line == 0)
                    .map(|s| format!("{} ", s.short()))
                    .unwrap_or_default();
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .w(px(72.))
                            .flex_none()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text3())
                            .child(row.category),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text())
                            .child(format!("{script}{}", row.what)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(tokens::text_xs())
                            .text_color(if delta > 0 {
                                tokens::warning()
                            } else {
                                tokens::text2()
                            })
                            .child(format!("{before} → {after}")),
                    )
                    .child(self.leak_site_link(peer, ("leak-diff", i), row.site.clone(), cx))
            }))
    }

    fn leak_scripts(
        &self,
        peer: Peer,
        latest: Option<&Sample>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(latest) = latest else {
            return v_flex()
                .px(px(8.))
                .child(small("The first sample comes in a moment."));
        };
        v_flex()
            .px(px(8.))
            .gap(px(2.))
            .children(
                latest
                    .scripts
                    .iter()
                    .take(ROWS)
                    .enumerate()
                    .map(|(i, script)| {
                        let site = Site {
                            script: script.name.clone(),
                            line: 0,
                        };
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(tokens::text_xs())
                                    .text_color(tokens::text())
                                    .child(script.name.clone()),
                            )
                            .child(small(format!("LuaHeap {}", kib(script.heap))))
                            .child(small(format!("{} connections", script.connections)))
                            .child(self.leak_site_link(peer, ("leak-script", i), Some(site), cx))
                    }),
            )
    }

    fn leak_notes(&self, latest: Option<&Sample>) -> impl IntoElement {
        let mut notes = vec![format!(
            "Sampled every {} s of game time while this dock is open.",
            leaks::SAMPLE_EVERY
        )];
        if let Some(latest) = latest {
            let unheld: usize = latest.instances.iter().map(|g| g.unheld).sum();
            if unheld > 0 {
                notes.push(format!(
                    "{unheld} nil-parented instances nothing holds are left out: Roblox collects \
                     those."
                ));
            }
            notes.push(format!(
                "{} of the heap is the engine's or was allocated before tracking started.",
                kib(latest.unattributed)
            ));
            if latest.truncated {
                notes.push(
                    "The table walk stopped at its budget: some tables were not looked at."
                        .to_owned(),
                );
            }
        }
        v_flex()
            .px(px(8.))
            .pt(px(6.))
            .gap(px(2.))
            .children(notes.into_iter().map(small))
    }
}

fn session_peers(session: &rbx_sim::Session) -> Vec<Peer> {
    std::iter::once(Peer::Server)
        .chain((0..session.clients.len()).map(Peer::Client))
        .collect()
}
