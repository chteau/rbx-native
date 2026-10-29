//! The Performance view's section: which measure tints the viewport, the
//! colour ramp's legend, the place's total, and the costliest parts (and,
//! for the `perf-models` beta's measures, models), each a row that selects
//! its part or model and puts the camera on it.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::perf::Metric;

use crate::tokens;

use super::super::menu::{self, MenuId};
use super::super::rows::select_box;
use super::super::{chrome, Shell};
use super::rows::{row, CONTROL_HEIGHT, CONTROL_WIDTH};

/// The dropdown's measures; the beta's two round the shipped five.
fn metrics() -> Vec<Metric> {
    if !crate::beta::enabled(super::super::PERF_BETA_ID) {
        return Metric::ALL.to_vec();
    }
    std::iter::once(Metric::Total)
        .chain(Metric::ALL)
        .chain([Metric::Physics])
        .collect()
}

impl Shell {
    pub(super) fn perf_section(&mut self, cx: &mut Context<Self>) -> Div {
        let current = self.perf.as_ref().map(|perf| perf.metric);
        let items = std::iter::once(
            menu::item("Off")
                .checked(current.is_none())
                .on_click(|shell, cx| shell.set_perf_metric(None, cx)),
        )
        .chain(metrics().into_iter().map(|metric| {
            menu::item(metric.label())
                .checked(current == Some(metric))
                .on_click(move |shell, cx| shell.set_perf_metric(Some(metric), cx))
        }))
        .collect();
        let live = self.live_shown();
        let trigger = select_box()
            .id("viewport-performance")
            .tab_index(self.tab_order.next())
            .w(px(CONTROL_WIDTH))
            .h(px(CONTROL_HEIGHT))
            .px(px(9.))
            .flex()
            .items_center()
            .justify_between()
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text2())
            .cursor_pointer()
            .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
            .child(current.map_or("Off", Metric::label))
            .child(Icon::new(IconName::ChevronDown).size(px(10.)));
        let control: AnyElement = if live {
            div()
                .text_size(tokens::text_sm())
                .text_color(tokens::text3())
                .child("Edit only")
                .into_any_element()
        } else {
            menu::dropdown(
                self,
                MenuId::ViewportPerformance,
                chrome::Trigger::new(trigger).tooltip("Tint parts by what they cost"),
                items,
                cx,
            )
            .into_any_element()
        };

        // As wide as the Rendering column, rather than the dock's whole width.
        let mut body = v_flex().child(
            div()
                .max_w(px(300.))
                .child(row("Performance view", control)),
        );
        if let Some(perf) = &self.perf {
            body = body.child(match &perf.report {
                None => note("Measuring…"),
                Some(report) => {
                    let handle = cx.entity();
                    let focus = |target: Ref| {
                        let handle = handle.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| shell.perf_focus(target, cx));
                        }
                    };
                    let worst = report.worst.iter().enumerate().filter_map(|(rank, &part)| {
                        let readout = report.readout(part)?;
                        Some(
                            ranked_row(
                                SharedString::from(format!("perf-worst-{rank}")),
                                rank,
                                &readout.name,
                                perf.metric.format(readout.value),
                                readout.detail.clone(),
                            )
                            .on_click(focus(part)),
                        )
                    });
                    let models = report.models.iter().enumerate().map(|(rank, model)| {
                        let score = match perf.metric {
                            Metric::Physics => model.cost.physics(),
                            _ => model.cost.total(),
                        };
                        let figures = SharedString::from(model.cost.figures());
                        ranked_row(
                            SharedString::from(format!("perf-model-{rank}")),
                            rank,
                            &model.name,
                            perf.metric.format(score),
                            model.cost.leading(),
                        )
                        .tooltip(move |window, cx| {
                            super::super::tooltip::text(figures.clone(), window, cx)
                        })
                        .on_click(focus(model.referent))
                    });
                    let mut lists = v_flex()
                        .gap(px(8.))
                        .pt(px(4.))
                        .child(note(&report.summary))
                        .child(legend(&report.legend));
                    if !report.models.is_empty() {
                        lists = lists
                            .child(heading(format!("COSTLIEST MODELS {}", report.models.len())))
                            .child(list(models));
                    }
                    lists
                        .child(heading(format!("COSTLIEST {}", report.worst.len())))
                        .child(list(worst))
                        .into_any_element()
                }
            });
        }
        super::rows::section("PERFORMANCE", vec![body.into_any_element()])
    }
}

fn heading(text: String) -> Div {
    div()
        .text_size(tokens::text_xxs())
        .font_weight(tokens::WEIGHT_BOLD)
        .text_color(tokens::text3())
        .child(text)
}

fn list(rows: impl Iterator<Item = Stateful<Div>>) -> Div {
    v_flex()
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .children(rows)
}

/// One row of a costliest list: rank, name, value, and what drives it.
fn ranked_row(
    id: SharedString,
    rank: usize,
    name: &str,
    value: String,
    detail: String,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(22.))
        .mx(px(-8.))
        .px(px(8.))
        .gap(px(10.))
        .items_center()
        .rounded(tokens::radius_badge())
        .cursor_pointer()
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover_subtle()))
        .child(
            div()
                .w(px(18.))
                .flex_none()
                .text_color(tokens::text3())
                .child(format!("{}", rank + 1)),
        )
        .child(
            div()
                .w(px(180.))
                .flex_none()
                .truncate()
                .text_color(tokens::text())
                .child(name.to_string()),
        )
        .child(
            div()
                .w(px(150.))
                .flex_none()
                .font_family(tokens::font_mono())
                .text_color(tokens::text())
                .child(value),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(tokens::text3())
                .child(detail),
        )
}

fn note(text: &str) -> AnyElement {
    div()
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(tokens::text2())
        .child(text.to_string())
        .into_any_element()
}

/// The ramp as a strip of swatches, each over the smallest value it
/// stands for.
fn legend(stops: &[([u8; 3], String)]) -> Div {
    h_flex()
        .gap(px(2.))
        .children(stops.iter().map(|([r, g, b], label)| {
            v_flex()
                .w(px(72.))
                .gap(px(2.))
                .child(div().h(px(10.)).rounded(px(2.)).bg(rgb(u32::from(*r) << 16
                    | u32::from(*g) << 8
                    | u32::from(*b))))
                .child(
                    div()
                        .text_size(tokens::text_xxs())
                        .line_height(tokens::line_xxs())
                        .font_family(tokens::font_mono())
                        .text_color(tokens::text3())
                        .truncate()
                        .child(format!("≥ {label}")),
                )
        }))
}
