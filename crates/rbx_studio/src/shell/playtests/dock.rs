//! The Playtests dock's view.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::chrome;
use super::super::layout::Panel;
use super::super::menu::{self, MenuId};
use super::super::rows;
use super::super::Shell;
use super::playback::clock;
use super::{RATES, SPEEDS};
use crate::launcher::ui;
use crate::playtests::heat::HeatKind;
use crate::playtests::replay;
use crate::tokens;

const EMPTY: &str = "Nothing recorded for this place yet. Every Play session records each \
player's position, camera, deaths, chat and errors here, to replay over the 3D view. Game \
scripts can mark moments too, guarded so a published game skips it: \
local rbxnative = shared.rbxnative; if rbxnative then rbxnative.mark(\"boss_down\", { boss = \"Golem\" }) end";

const HEATS: [(&str, Option<HeatKind>); 4] = [
    ("Off", None),
    ("Presence", Some(HeatKind::Presence)),
    ("Deaths", Some(HeatKind::Deaths)),
    ("Stuck", Some(HeatKind::Stuck)),
];

fn linear_to_color(color: [f32; 3]) -> Hsla {
    let srgb = |c: f32| {
        if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    Rgba {
        r: srgb(color[0]),
        g: srgb(color[1]),
        b: srgb(color[2]),
        a: 1.0,
    }
    .into()
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

/// "1 player", "2 players".
fn counted(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        _ => format!("{n} {noun}s"),
    }
}

fn small_text(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .text_size(tokens::text_xs())
        .text_color(tokens::text2())
        .child(text.into())
}

/// The fraction of the scrubber `x` sits at.
fn scrub_fraction(bounds: Option<Bounds<Pixels>>, x: Pixels) -> Option<f32> {
    let bounds = bounds?;
    let width = f32::from(bounds.size.width);
    (width > 0.0).then(|| (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0))
}

impl Shell {
    pub(in crate::shell) fn playtests_dock(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.playtests_refresh_if_moved(cx);
        self.playtests_apply_env(cx);
        let overflow = menu::dropdown(
            self,
            MenuId::PlaytestsOverflow,
            chrome::dock_options_trigger(
                "playtests-overflow",
                IconName::Ellipsis,
                16.,
                "Playtests dock options",
            ),
            self.move_items(Panel::Playtests),
            cx,
        );
        let body = v_flex()
            .id("playtests-dock")
            .size_full()
            .gap_1()
            .py_1()
            .overflow_y_scroll()
            .children(self.playtests_repro_card(cx))
            .child(self.playtests_toolbar(cx))
            .child(self.playtests_rows(cx));
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn playtests_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = &self.playtests;
        let longest = state.longest();
        let fraction = if longest > 0.0 {
            (state.time / longest).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let ticks: Vec<Div> = if longest > 0.0 {
            state
                .shown()
                .iter()
                .flat_map(|(_, rec)| {
                    rec.events
                        .iter()
                        .map(|event| {
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(relative((event.t / longest).clamp(0.0, 1.0)))
                                .w(px(2.))
                                .bg(linear_to_color(replay::event_color(event.kind)))
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        } else {
            Vec::new()
        };
        let report = state.scrub_bounds.clone();
        let down_bounds = state.scrub_bounds.clone();
        let move_bounds = state.scrub_bounds.clone();
        let scrubber = div()
            .id("playtests-scrubber")
            .relative()
            .flex_1()
            .min_w(px(60.))
            .h(px(14.))
            .rounded_sm()
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .cursor_pointer()
            .child(
                canvas(move |b, _, _| report.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(ticks)
            .child(
                div()
                    .absolute()
                    .top(px(-2.))
                    .bottom(px(-2.))
                    .left(relative(fraction))
                    .w(px(2.))
                    .bg(tokens::text()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                    if let Some(f) = scrub_fraction(down_bounds.get(), event.position.x) {
                        let longest = shell.playtests.longest();
                        shell.playtests_set_time(f * longest, cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |shell, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                if let Some(f) = scrub_fraction(move_bounds.get(), event.position.x) {
                    let longest = shell.playtests.longest();
                    shell.playtests_set_time(f * longest, cx);
                }
            }));

        let play = chrome::icon_button(
            "playtests-play",
            if state.playing {
                IconName::Pause
            } else {
                IconName::Play
            },
            if state.playing { "Pause" } else { "Play" },
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.playtests_toggle_play(cx)));

        let speeds = SPEEDS.iter().map(|&speed| {
            toggle(
                SharedString::from(format!("playtests-speed-{speed}")),
                format!("{speed}×"),
                state.speed == speed,
            )
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.playtests.speed = speed;
                cx.notify();
            }))
        });
        let heats = HEATS.iter().map(|&(label, heat)| {
            toggle(
                SharedString::from(format!("playtests-heat-{label}")),
                label,
                state.heat == heat,
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.playtests_set_heat(heat, cx)))
        });
        let rates = RATES.iter().map(|&rate| {
            let label = if rate == 0.0 {
                "Off".to_owned()
            } else {
                format!("{rate} Hz")
            };
            toggle(
                SharedString::from(format!("playtests-rate-{rate}")),
                label,
                state.rate_hz == rate,
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.playtests_set_rate(rate, cx)))
        });

        v_flex()
            .w_full()
            .gap_1()
            .px_2()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(play)
                    .child(scrubber)
                    .child(small_text(format!(
                        "{} / {}",
                        clock(state.time),
                        clock(longest)
                    ))),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap_1()
                    .items_center()
                    .child(small_text("Speed"))
                    .children(speeds)
                    .child(small_text("Heat"))
                    .children(heats),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap_1()
                    .items_center()
                    .child(small_text("Record"))
                    .children(rates),
            )
    }

    fn playtests_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.playtests.entries.is_empty() {
            return div()
                .m_2()
                .text_size(tokens::text_xs())
                .text_color(tokens::text2())
                .child(EMPTY)
                .into_any_element();
        }
        let rows = self.playtests.entries.iter().enumerate().map(|(i, entry)| {
            let delete_path = entry.path.clone();
            let delete = chrome::icon_button(("playtests-delete", i), IconName::Trash, "Delete")
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.playtests_delete(delete_path.clone(), cx)
                }));
            let summary = match &entry.summary {
                Ok(summary) => summary,
                Err(err) => {
                    let file = entry
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    return h_flex()
                        .id(("playtests-row", i))
                        .w_full()
                        .gap_2()
                        .px_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text_error())
                                .child(format!("Can't read {file}: {err}")),
                        )
                        .child(delete);
                }
            };
            let visible = entry.visible;
            let renaming = self
                .playtests
                .renaming
                .as_ref()
                .filter(|(path, _)| *path == entry.path)
                .map(|(_, input)| input.clone());
            let name: AnyElement = match renaming {
                Some(input) => div()
                    .flex_1()
                    .min_w_0()
                    .on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            shell.playtests_cancel_rename(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(Input::new(&input))
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text())
                    .child(summary.name.clone())
                    .into_any_element(),
            };
            let date = chrono::DateTime::from_timestamp(summary.started as i64, 0)
                .map(|at| {
                    at.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            let rename_path = entry.path.clone();
            let rename_name = summary.name.clone();
            let in_repro = self.playtests.repro.is_some();
            let more = self.playtests_row_menu(i, &entry.path, &summary.name, in_repro, cx);
            h_flex()
                .id(("playtests-row", i))
                .w_full()
                .gap_2()
                .px_2()
                .items_center()
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |shell, _, _, cx| {
                        shell.open_menu = Some(MenuId::PlaytestsRow(i));
                        shell.menu_nav.cursor = None;
                        cx.notify();
                    }),
                )
                .child(rows::checkbox(
                    ("playtests-visible", i),
                    visible,
                    cx.listener(move |shell, _, _, cx| {
                        shell.playtests_set_visible(i, !visible, cx)
                    }),
                ))
                .child(
                    div()
                        .flex_none()
                        .w(px(3.))
                        .h(px(14.))
                        .bg(linear_to_color(replay::recording_color(i))),
                )
                .child(name)
                .child(small_text(date))
                .child(small_text(clock(summary.duration)))
                .child(small_text(counted(summary.players, "player")))
                .child(small_text(counted(summary.deaths, "death")))
                .when(!in_repro, |row| {
                    row.child(
                        chrome::icon_button(("playtests-rename", i), IconName::Pencil, "Rename")
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.playtests_start_rename(
                                    rename_path.clone(),
                                    rename_name.clone(),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(delete)
                })
                .child(more)
        });
        v_flex().w_full().gap_1().children(rows).into_any_element()
    }

    /// A row's "…" menu, also opened by a right-click on the row. Rename
    /// needs a window for its field, which a menu action doesn't get, so it
    /// stays a row button.
    fn playtests_row_menu(
        &self,
        i: usize,
        path: &std::path::Path,
        name: &str,
        in_repro: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let export_path = path.to_path_buf();
        let copy_path = path.to_path_buf();
        let delete_path = path.to_path_buf();
        let mut items = vec![
            menu::item("Export repro…")
                .icon(IconName::Package)
                .hint(format!(
                    "Save {name} as one .rbxrepro file to attach to a bug report"
                ))
                .on_click(move |shell, cx| shell.playtests_request_export(export_path.clone(), cx)),
            menu::item("Copy bug report")
                .icon(IconName::Copy)
                .hint("Copy a Markdown summary: time, players, deaths, errors, markers")
                .on_click(move |shell, cx| shell.playtests_copy_report(&copy_path, cx)),
        ];
        if in_repro {
            items.remove(0);
        } else {
            items.push(menu::separator());
            items.push(
                menu::item("Delete")
                    .icon(IconName::Trash)
                    .on_click(move |shell, cx| shell.playtests_delete(delete_path.clone(), cx)),
            );
        }
        menu::dropdown_at(
            self,
            MenuId::PlaytestsRow(i),
            chrome::Trigger::new(chrome::icon_button(
                ("playtests-more", i),
                IconName::Ellipsis,
                "More",
            )),
            items,
            Anchor::TopRight,
            cx,
        )
    }
}
