//! The frame chart, its legend and the picked frame's timeline.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_sim::profile::FrameProfile;
use rbx_sim::Peer;

use super::{ms, parts, small, Parts, BUDGET_MS, CHART_H, MIN_SCALE_MS, SHOWN};
use crate::shell::Shell;
use crate::tokens;

impl Shell {
    pub(super) fn profiler_legend(&self, frames: &[FrameProfile]) -> impl IntoElement {
        let recent: Vec<&FrameProfile> = frames.iter().rev().take(60).collect();
        let n = recent.len().max(1) as f64;
        let mut sums = [0.0; 5];
        for frame in &recent {
            for (sum, part) in sums.iter_mut().zip(parts(frame)) {
                *sum += part.1;
            }
        }
        let names = frames.last().map(parts);
        let total: f64 = sums.iter().sum::<f64>() / n;
        h_flex()
            .px(px(8.))
            .gap(px(10.))
            .flex_wrap()
            .items_center()
            .children(
                names
                    .into_iter()
                    .flatten()
                    .zip(sums)
                    .map(|((name, _, colour), sum)| {
                        h_flex()
                            .gap(px(4.))
                            .items_center()
                            .child(div().size(px(8.)).rounded(px(2.)).bg(colour))
                            .child(small(format!("{name} {:.2} ms", sum / n)))
                    }),
            )
            .child(div().flex_1())
            .child(small(format!(
                "avg {total:.2} ms of {BUDGET_MS:.1} ms, last {} frames",
                recent.len()
            )))
    }

    pub(super) fn profiler_chart(
        &self,
        frames: &[FrameProfile],
        picked: Option<u64>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let bars: Vec<(u64, Parts)> = frames.iter().map(|f| (f.frame, parts(f))).collect();
        let peak = bars
            .iter()
            .map(|(_, p)| p.iter().map(|x| x.1).sum::<f64>())
            .fold(MIN_SCALE_MS, f64::max);
        let report = self.profiler.chart.clone();
        let click = self.profiler.chart.clone();
        let numbers: Vec<u64> = frames.iter().map(|f| f.frame).collect();
        let budget_y = (1.0 - BUDGET_MS / peak) as f32;
        div()
            .id("profiler-chart")
            .flex_none()
            .mx(px(8.))
            .h(px(CHART_H))
            .relative()
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .overflow_hidden()
            .cursor_pointer()
            .child(
                canvas(
                    move |bounds, _, _| report.set(Some(bounds)),
                    move |bounds, _, window, _| {
                        let w = f32::from(bounds.size.width);
                        let h = f32::from(bounds.size.height);
                        let slot = w / SHOWN as f32;
                        let start = SHOWN.saturating_sub(bars.len());
                        for (i, (number, parts)) in bars.iter().enumerate() {
                            let x = f32::from(bounds.origin.x) + (start + i) as f32 * slot;
                            let mut y = f32::from(bounds.origin.y) + h;
                            if picked == Some(*number) {
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: point(px(x), bounds.origin.y),
                                        size: size(px(slot.max(1.0)), bounds.size.height),
                                    },
                                    tokens::track(),
                                ));
                            }
                            for (_, value, colour) in parts {
                                let bar = (*value / peak) as f32 * h;
                                if bar <= 0.0 {
                                    continue;
                                }
                                y -= bar;
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: point(px(x), px(y)),
                                        size: size(px((slot - 1.0).max(1.0)), px(bar)),
                                    },
                                    *colour,
                                ));
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(budget_y))
                    .h(px(1.))
                    .bg(tokens::text_error()),
            )
            .child(
                div()
                    .absolute()
                    .right(px(4.))
                    .top(px(2.))
                    .child(small(format!("{peak:.0} ms"))),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                    let Some(bounds) = click.get() else {
                        return;
                    };
                    let w = f32::from(bounds.size.width);
                    let slot = (f32::from(event.position.x - bounds.origin.x) / w * SHOWN as f32)
                        .floor() as usize;
                    let start = SHOWN.saturating_sub(numbers.len());
                    if let Some(number) = slot.checked_sub(start).and_then(|i| numbers.get(i)) {
                        shell.profiler.picked = Some(*number);
                        // Picking a frame holds the chart, so it stays put.
                        if shell.profiler.held.is_none() {
                            shell.profiler.held = Some(shell.profiler_frames());
                        }
                        cx.notify();
                    }
                }),
            )
    }
}

/// The picked frame: its parts as one split bar, then its scopes on its
/// timeline, a row per nesting depth.
pub(super) fn profiler_frame(frame: &FrameProfile) -> impl IntoElement {
    let total = ms(frame.total()).max(1e-6);
    let split = h_flex()
        .h(px(10.))
        .mx(px(8.))
        .rounded(px(2.))
        .overflow_hidden()
        .bg(tokens::track())
        .children(parts(frame).into_iter().map(|(_, value, colour)| {
            div()
                .h_full()
                .w(relative((value / total) as f32))
                .bg(colour)
        }));
    let span = frame
        .labels
        .iter()
        .map(|(_, l)| l.end.duration_since(frame.started))
        .max()
        .map_or(total, |end| ms(end).max(total));
    let depth = frame
        .labels
        .iter()
        .map(|(_, l)| l.depth)
        .max()
        .map_or(0, |d| d + 1);
    let bars = frame.labels.iter().map(|(peer, label)| {
        let left = ms(label.start.saturating_duration_since(frame.started)) / span;
        let width = (ms(label.end.duration_since(label.start)) / span).max(0.004);
        div()
            .absolute()
            .top(px(label.depth as f32 * 18.))
            .left(relative(left as f32))
            .w(relative(width as f32))
            .h(px(16.))
            .px(px(3.))
            .rounded(px(3.))
            .overflow_hidden()
            .bg(match peer {
                Peer::Server => tokens::output_server(),
                Peer::Client(_) => tokens::output_client(),
            })
            .text_size(tokens::text_xs())
            .text_color(tokens::text_full())
            .truncate()
            .child(format!(
                "{} {:.2} ms",
                label.name,
                ms(label.end.duration_since(label.start))
            ))
    });
    v_flex()
        .gap(px(4.))
        .child(
            h_flex()
                .px(px(8.))
                .gap(px(10.))
                .child(small(format!("Frame {}", frame.frame)))
                .child(small(format!("{total:.2} ms")))
                .child(small(format!(
                    "{} scope{}",
                    frame.labels.len(),
                    if frame.labels.len() == 1 { "" } else { "s" }
                ))),
        )
        .child(split)
        .child(
            div()
                .mx(px(8.))
                .relative()
                .h(px((depth.max(1) as f32) * 18.))
                .children(bars),
        )
}
