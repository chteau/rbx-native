//! The ScriptProfiler half: callgraph or flat function list.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{peer_label, small, toggle};
use crate::launcher::ui;
use crate::shell::Shell;
use crate::tokens;

impl Shell {
    pub(super) fn profiler_scripts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let flat = self.profiler.flat;
        let views = h_flex()
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .child(
                toggle("profiler-callgraph", "Callgraph", !flat).on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.profiler.flat = false;
                        cx.notify();
                    },
                )),
            )
            .child(
                toggle("profiler-functions", "Functions", flat).on_click(cx.listener(
                    |shell, _, _, cx| {
                        shell.profiler.flat = true;
                        cx.notify();
                    },
                )),
            )
            .child(div().flex_1())
            .when_some(self.profiler.report.as_ref(), |this, report| {
                this.child(small(format!("{:.1} ms sampled", report.total_ms)))
            });
        let rows: Vec<AnyElement> = match &self.profiler.report {
            None => vec![div()
                .px(px(8.))
                .child(small(format!(
                    "Start samples {}'s scripts {} times a second; Stop shows where the time \
                     went, by frame task (Callgraph) or by function.",
                    peer_label(self.profiler.peer),
                    self.profiler.frequency
                )))
                .into_any_element()],
            Some(report) => {
                let lines = if flat {
                    &report.functions
                } else {
                    &report.callgraph
                };
                let total = report.total_ms.max(1e-9);
                lines
                    .iter()
                    .map(|line| {
                        let share = (line.ms / total).clamp(0.0, 1.0) as f32;
                        h_flex()
                            .px(px(8.))
                            .h(px(20.))
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .pl(px(line.depth as f32 * 12.))
                                    .truncate()
                                    .text_size(tokens::text_xs())
                                    .text_color(if line.depth == 0 && !flat {
                                        tokens::text_strong()
                                    } else {
                                        tokens::text()
                                    })
                                    .child(line.name.clone()),
                            )
                            .child(
                                div()
                                    .w(px(160.))
                                    .flex_none()
                                    .truncate()
                                    .text_size(tokens::text_xs())
                                    .text_color(tokens::text3())
                                    .child(line.place.clone()),
                            )
                            .child(
                                div()
                                    .w(px(60.))
                                    .h(px(4.))
                                    .flex_none()
                                    .rounded(px(2.))
                                    .bg(tokens::track())
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(share))
                                            .rounded(px(2.))
                                            .bg(ui::accent()),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(64.))
                                    .flex_none()
                                    .text_right()
                                    .text_size(tokens::text_xs())
                                    .text_color(tokens::text2())
                                    .child(format!("{:.2} ms", line.ms)),
                            )
                            .into_any_element()
                    })
                    .collect()
            }
        };
        v_flex().gap(px(2.)).child(views).children(rows)
    }
}
