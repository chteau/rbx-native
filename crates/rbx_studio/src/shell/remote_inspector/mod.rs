//! The Remote inspector dock (Local Run beta): every `RemoteEvent`,
//! `UnreliableRemoteEvent` and `RemoteFunction` call crossing the
//! simulated link, per player and direction, with its arguments as they
//! travel, its bytes and the script line that made it; rates per remote
//! and per player; what went over Roblox's limits or got lost on a netsim
//! link. The session only logs while one of these docks shows
//! (`Shell::remote_inspector_wanted`), in the running window or one of its
//! Server & Clients tabs.

mod model;

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_lua::server::Caller;
use rbx_sim::remote_log::{Arg, Direction};

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::Shell;
use crate::launcher::ui;
use crate::tokens;
pub(crate) use model::Store;
use model::{Filter, Row, SPARK, UNRELIABLE_PER_SECOND};

/// Calls listed, the newest first.
const ROWS: usize = 300;
const SPARK_H: f32 = 14.0;

#[derive(Default)]
pub(crate) struct RemoteInspectorDock {
    filter: Filter,
    /// The remote name field, made once a window can make it.
    input: Option<(Entity<InputState>, Subscription)>,
    picked: Option<u64>,
    /// Argument tree nodes folded shut, by their path of keys.
    folded: HashSet<String>,
    /// A Server & Clients tab asked this window to open a caller's script.
    reveal: Option<Caller>,
}

fn small(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .text_size(tokens::text_xs())
        .text_color(tokens::text2())
        .child(text.into())
}

fn heading(text: &'static str) -> Div {
    div()
        .px(px(8.))
        .pt(px(6.))
        .text_size(tokens::text_xs())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text3())
        .child(text)
}

fn toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    ui::button(
        id,
        label,
        if on {
            ui::Weight::Primary
        } else {
            ui::Weight::Ghost
        },
        true,
    )
}

fn cell(width: f32, text: impl Into<SharedString>) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .truncate()
        .text_size(tokens::text_xs())
        .child(text.into())
}

fn bytes_text(bytes: usize) -> String {
    if bytes >= 10_000 {
        format!("{:.1} KB", bytes as f64 / 1000.0)
    } else {
        format!("{bytes} B")
    }
}

fn short_direction(direction: Direction) -> &'static str {
    match direction {
        Direction::ToServer => "C → S",
        Direction::ToClient => "S → C",
        Direction::ToAll => "S → all",
    }
}

fn sparkline(bars: [u32; SPARK]) -> Div {
    let peak = bars.iter().copied().max().unwrap_or(0).max(1) as f32;
    h_flex()
        .h(px(SPARK_H))
        .items_end()
        .gap(px(1.))
        .children(bars.into_iter().map(move |n| {
            div()
                .w(px(2.))
                .h(px((n as f32 / peak * SPARK_H).max(if n > 0 {
                    1.0
                } else {
                    0.0
                })))
                .bg(ui::accent())
        }))
}

impl Shell {
    pub(in crate::shell) fn remote_inspector_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::RemoteInspectorOverflow,
            chrome::dock_options_trigger(
                "remote-inspector-overflow",
                IconName::Ellipsis,
                16.,
                "Remote Inspector options",
            ),
            self.move_items(Panel::RemoteInspector),
            cx,
        );
        self.ensure_remote_input(window, cx);
        if self.remote_store(cx).is_none() {
            let empty = div()
                .p(px(12.))
                .text_size(tokens::text_sm())
                .text_color(tokens::text2())
                .child(
                    "Start Run (local), Play (local) or Server & Clients to watch its remotes: \
                     every RemoteEvent, UnreliableRemoteEvent and RemoteFunction call each way, \
                     its arguments, bytes and script line, the rates per remote and per player, \
                     and what Roblox's limits or the simulated network dropped.",
                );
            return (
                Some(overflow.into_any_element()),
                Some(empty.into_any_element()),
            );
        }
        let body = v_flex()
            .id("remote-inspector-dock")
            .size_full()
            .child(self.remote_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(self.remote_calls(cx))
                    .child(self.remote_rates(cx)),
            );
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn ensure_remote_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_inspector.input.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Remote name contains"));
        let subscription = cx.subscribe(&input, |shell, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                shell.remote_inspector.filter.name = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        self.remote_inspector.input = Some((input, subscription));
    }

    fn remote_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let (paused, players) = self
            .remote_store(cx)
            .map(|s| (s.paused, s.players()))
            .unwrap_or_default();
        let filter = &self.remote_inspector.filter;
        let player_toggle = |label: String, player: Option<String>| {
            let on = filter.player == player;
            toggle(
                SharedString::from(format!("remote-player-{label}")),
                label,
                on,
            )
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.remote_inspector.filter.player = player.clone();
                cx.notify();
            }))
        };
        let direction_toggle = |label: &'static str, direction: Option<Direction>| {
            toggle(
                SharedString::from(format!("remote-direction-{label}")),
                label,
                filter.direction == direction,
            )
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.remote_inspector.filter.direction = direction;
                cx.notify();
            }))
        };
        let separator = || div().w(px(1.)).h(px(16.)).bg(tokens::border());
        h_flex()
            .px(px(8.))
            .py(px(4.))
            .gap(px(6.))
            .items_center()
            .flex_wrap()
            .when_some(self.remote_inspector.input.as_ref(), |this, (input, _)| {
                this.child(div().w(px(180.)).child(Input::new(input).xsmall()))
            })
            .child(separator())
            .child(player_toggle("All players".into(), None))
            .children(
                players
                    .into_iter()
                    .map(|p| player_toggle(p.clone(), Some(p))),
            )
            .child(separator())
            .child(direction_toggle("Both ways", None))
            .child(direction_toggle("C → S", Some(Direction::ToServer)))
            .child(direction_toggle("S → C", Some(Direction::ToClient)))
            .child(direction_toggle("S → all", Some(Direction::ToAll)))
            .child(div().flex_1())
            .child(
                toggle(
                    "remote-pause",
                    if paused { "Resume" } else { "Pause" },
                    paused,
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.remote_store_update(cx, |store| store.paused = !store.paused);
                })),
            )
            .child(
                ui::button("remote-clear", "Clear", ui::Weight::Ghost, true).on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.remote_inspector.picked = None;
                        shell.remote_store_update(cx, Store::clear);
                    },
                )),
            )
            .child(
                ui::button("remote-csv", "CSV", ui::Weight::Ghost, true)
                    .on_click(cx.listener(|shell, _, _, cx| shell.remote_export(false, cx))),
            )
            .child(
                ui::button("remote-json", "JSON", ui::Weight::Ghost, true)
                    .on_click(cx.listener(|shell, _, _, cx| shell.remote_export(true, cx))),
            )
    }

    fn remote_calls(&self, cx: &Context<Self>) -> impl IntoElement {
        let filter = self.remote_inspector.filter.clone();
        let picked = self.remote_inspector.picked;
        let Some(store) = self.remote_store(cx) else {
            return v_flex();
        };
        let now = store.now;
        let rows: Vec<(u64, AnyElement)> = store
            .rows()
            .rev()
            .filter(|r| filter.keeps(&r.call))
            .take(ROWS)
            .map(|row| (row.call.id, call_row(row, now, picked == Some(row.call.id))))
            .collect();
        let details = picked.and_then(|id| store.row(id)).map(|row| {
            v_flex()
                .px(px(8.))
                .pb(px(6.))
                .gap(px(1.))
                .border_t_1()
                .border_color(tokens::border())
                .child(heading("ARGUMENTS"))
                .child(small(format!(
                    "{}  ·  {}  ·  {}  ·  {}",
                    row.call.path,
                    row.call.direction.label(),
                    bytes_text(row.call.bytes),
                    row.call
                        .caller
                        .as_ref()
                        .map(|c| format!("{}:{}", c.script, c.line))
                        .unwrap_or_else(|| "no script line".into()),
                )))
                .children(arg_tree(
                    &row.call.args,
                    "",
                    0,
                    &self.remote_inspector.folded,
                    cx,
                ))
        });
        let empty = rows.is_empty();
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .px(px(8.))
                    .gap(px(8.))
                    .text_color(tokens::text3())
                    .child(cell(70., "Time"))
                    .child(cell(56., "Way"))
                    .child(cell(200., "Remote"))
                    .child(cell(80., "Player"))
                    .child(
                        div()
                            .flex_1()
                            .text_size(tokens::text_xs())
                            .child("Arguments"),
                    )
                    .child(cell(60., "Bytes"))
                    .child(cell(110., "Script"))
                    .child(cell(140., "Flag")),
            )
            .child(
                v_flex()
                    .id("remote-calls")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(empty, |this| {
                        this.child(
                            div()
                                .px(px(8.))
                                .child(small("No calls yet. Fire a remote in the running game.")),
                        )
                    })
                    .children(rows.into_iter().map(|(id, row)| {
                        div()
                            .id(SharedString::from(format!("remote-row-{id}")))
                            .child(row)
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.remote_pick(id, window, cx);
                            }))
                    })),
            )
            .children(details)
    }

    fn remote_rates(&self, cx: &App) -> impl IntoElement {
        let Some(store) = self.remote_store(cx) else {
            return v_flex().id("remote-rates");
        };
        let now = store.now;
        let rate_row = |name: &str, rate: &model::Rate, warn: Option<String>| {
            let (calls, bytes) = rate.per_second(now);
            v_flex()
                .px(px(8.))
                .py(px(2.))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(tokens::text_xs())
                                .child(name.to_owned()),
                        )
                        .child(sparkline(rate.spark(now))),
                )
                .child(small(format!("{calls}/s  ·  {}/s", bytes_text(bytes))))
                .when_some(warn, |this, warn| {
                    this.child(
                        div()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_error())
                            .child(warn),
                    )
                })
        };
        let lost = store
            .rows()
            .filter(|r| r.flag(now).is_some_and(|(_, error)| error))
            .count();
        v_flex()
            .id("remote-rates")
            .w(px(260.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(tokens::border())
            .child(heading("PER REMOTE"))
            .children(
                store
                    .by_remote
                    .iter()
                    .map(|(name, rate)| rate_row(name.rsplit('.').next().unwrap_or(name), rate, None)),
            )
            .child(heading("PER PLAYER"))
            .children(store.by_player.iter().map(|(name, rate)| {
                let peak = rate.unreliable_peak();
                let warn = (peak > UNRELIABLE_PER_SECOND).then(|| {
                    format!("{peak} unreliable/s to the server: over {UNRELIABLE_PER_SECOND}/s are dropped")
                });
                rate_row(name, rate, warn)
            }))
            .child(heading("DROPPED, LOST, FAILED"))
            .child(div().px(px(8.)).child(small(format!("{lost} of the calls kept"))))
    }

    fn remote_pick(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.remote_inspector.picked = Some(id);
        let caller = self
            .remote_store(cx)
            .and_then(|s| s.row(id))
            .and_then(|r| r.call.caller.clone());
        if let Some(caller) = caller {
            self.remote_reveal(caller, window, cx);
        }
        cx.notify();
    }

    /// Opens the edited place's copy of `caller`'s script at its line, in
    /// the window running the session.
    fn remote_reveal(&mut self, caller: Caller, window: &mut Window, cx: &mut Context<Self>) {
        if self.local_run_session().is_none() {
            if let Some(host) = self.play.live.local.peer.as_ref().and_then(|p| p.host()) {
                host.update(cx, |host, cx| {
                    host.remote_inspector.reveal = Some(caller);
                    cx.notify();
                });
            }
            return;
        }
        let Some(script) = self.remote_caller_script(&caller.script) else {
            self.output.push(
                "Play",
                crate::command_bar::Feedback::Warning(format!(
                    "{} only exists in the running game, so its source cannot be shown.",
                    caller.script
                )),
            );
            cx.notify();
            return;
        };
        let at = lsp_types::Position::new((caller.line as u32).saturating_sub(1), 0);
        self.reveal(script, lsp_types::Range::new(at, at), window, cx);
    }

    /// A tab's pick, opened here where the edited place is.
    pub(in crate::shell) fn realize_remote_reveal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(caller) = self.remote_inspector.reveal.take() {
            window.activate_window();
            self.remote_reveal(caller, window, cx);
        }
    }

    /// Saves the calls the filters keep as CSV or JSON.
    fn remote_export(&mut self, json: bool, cx: &mut Context<Self>) {
        let filter = self.remote_inspector.filter.clone();
        let Some(store) = self.remote_store(cx) else {
            return;
        };
        let rows: Vec<&Row> = store.rows().filter(|r| filter.keeps(&r.call)).collect();
        let text = if json {
            model::json(rows.into_iter())
        } else {
            model::csv(rows.into_iter())
        };
        let name = format!("remotes.{}", if json { "json" } else { "csv" });
        let directory = self
            .path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| ".".into());
        let picked = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(picked) = picked.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| {
                let feedback = match picked {
                    Ok(Some(path)) => match std::fs::write(&path, text) {
                        Ok(()) => crate::command_bar::Feedback::Info(format!(
                            "Exported the remote calls to {}",
                            path.display()
                        )),
                        Err(err) => crate::command_bar::Feedback::Error(format!(
                            "Couldn't write {}: {err}",
                            path.display()
                        )),
                    },
                    Ok(None) => return,
                    Err(err) => {
                        crate::command_bar::Feedback::Error(format!("no save dialog: {err:#}"))
                    }
                };
                shell.output.push("Remote Inspector", feedback);
                cx.notify();
            });
        })
        .detach();
    }
}

fn call_row(row: &Row, now: f64, picked: bool) -> AnyElement {
    let call = &row.call;
    let flag = row.flag(now);
    h_flex()
        .px(px(8.))
        .py(px(1.))
        .gap(px(8.))
        .when(picked, |this| this.bg(tokens::selection()))
        .hover(|this| this.bg(tokens::hover()))
        .child(cell(70., format!("{:.3}", call.time)).text_color(tokens::text2()))
        .child(cell(56., short_direction(call.direction)))
        .child(cell(200., call.path.clone()))
        .child(cell(80., call.player.clone()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(tokens::text_xs())
                .text_color(tokens::text2())
                .child(row.summary()),
        )
        .child(cell(60., bytes_text(call.bytes)))
        .child(cell(110., row.caller()).text_color(ui::accent()))
        .child(match flag {
            Some((text, error)) => cell(140., text).text_color(if error {
                tokens::text_error()
            } else {
                tokens::warning()
            }),
            None => cell(140., ""),
        })
        .into_any_element()
}

/// `args` as an indented tree under `path`; a table folds open and shut.
fn arg_tree(
    args: &[Arg],
    path: &str,
    depth: usize,
    folded: &HashSet<String>,
    cx: &Context<Shell>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let key = arg.key.clone().unwrap_or_else(|| format!("#{}", index + 1));
        let here = format!("{path}/{key}");
        let table = !arg.children.is_empty();
        let shut = folded.contains(&here);
        let marker = match (table, shut) {
            (false, _) => "  ",
            (true, true) => "▸ ",
            (true, false) => "▾ ",
        };
        let id = here.clone();
        out.push(
            h_flex()
                .id(SharedString::from(format!("remote-arg-{here}")))
                .pl(px(depth as f32 * 14.))
                .gap(px(6.))
                .text_size(tokens::text_xs())
                .child(format!("{marker}{key}"))
                .child(div().text_color(tokens::text3()).child(arg.kind.clone()))
                .child(div().text_color(tokens::text2()).child(arg.text.clone()))
                .when(table, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            let folded = &mut shell.remote_inspector.folded;
                            if !folded.remove(&id) {
                                folded.insert(id.clone());
                            }
                            cx.notify();
                        }))
                })
                .into_any_element(),
        );
        if table && !shut {
            out.extend(arg_tree(&arg.children, &here, depth + 1, folded, cx));
        }
    }
    out
}
