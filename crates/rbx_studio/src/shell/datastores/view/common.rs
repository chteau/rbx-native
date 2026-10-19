//! Pieces the section views share: failure panels, list states, rows,
//! search and JSON fields, and the value tree.

use std::collections::BTreeSet;

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::json_tree::{self, Kind};
use super::super::state::{Load, Problem, TreeTarget, ValueView};
use super::super::{fetch, DataStoresWindow};

pub(super) fn problem_panel(problem: &Problem, cx: &mut Context<DataStoresWindow>) -> AnyElement {
    let (title, detail, scope) = match problem {
        Problem::MissingScope { scope } => (
            "The API key can\u{2019}t do this yet".to_string(),
            "Add the permission on the Creator Dashboard, or grant this experience to the key."
                .to_string(),
            Some(scope.clone()),
        ),
        Problem::NoKey => (
            "No Open Cloud key".to_string(),
            "Add an API key to browse and edit data stores.".to_string(),
            None,
        ),
        Problem::RateLimited => (
            "Roblox is rate limiting this experience".to_string(),
            "Wait a moment, then try again.".to_string(),
            None,
        ),
        other => (
            "Couldn\u{2019}t load this".to_string(),
            fetch::describe(other),
            None,
        ),
    };
    let keyed = matches!(problem, Problem::MissingScope { .. } | Problem::NoKey);
    v_flex()
        .p(px(16.))
        .gap(px(8.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .text_color(tokens::warning())
                .child(ui::icon("triangle-alert", 16.))
                .child(div().font_weight(FontWeight::BOLD).child(title)),
        )
        .child(div().text_color(tokens::text2()).child(detail))
        .children(scope.map(|scope| {
            div()
                .font_family(tokens::font_mono())
                .text_size(px(12.))
                .child(scope)
        }))
        .when(keyed, |d| {
            d.child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        ui::button("ds-problem-manage", "Manage key", Weight::Secondary, true)
                            .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                    )
                    .child(
                        ui::button(
                            "ds-problem-dashboard",
                            "Open Creator Dashboard",
                            Weight::Ghost,
                            true,
                        )
                        .on_click(
                            cx.listener(|_, _, _, cx| {
                                cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL)
                            }),
                        ),
                    ),
            )
        })
        .into_any_element()
}

pub(super) fn list_state(
    load: &Load,
    has_items: bool,
    empty_text: &str,
    cx: &mut Context<DataStoresWindow>,
) -> Option<AnyElement> {
    match load {
        Load::Busy if !has_items => Some(
            h_flex()
                .p(px(16.))
                .gap(px(8.))
                .items_center()
                .text_color(tokens::text3())
                .child(ui::spinner("ds-list-spin", 14.))
                .child("Loading\u{2026}")
                .into_any_element(),
        ),
        Load::Failed(problem) => Some(problem_panel(problem, cx)),
        Load::Idle if !has_items => Some(
            div()
                .p(px(16.))
                .text_color(tokens::text3())
                .child(empty_text.to_string())
                .into_any_element(),
        ),
        _ => None,
    }
}

pub(super) fn load_more(
    id: &'static str,
    busy: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    if busy {
        return h_flex()
            .p(px(10.))
            .justify_center()
            .child(ui::spinner("ds-more-spin", 14.))
            .into_any_element();
    }
    h_flex()
        .p(px(8.))
        .justify_center()
        .child(ui::button(id, "Load more", Weight::Ghost, true).on_click(on_click))
        .into_any_element()
}

pub(super) fn column_label(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(11.))
        .text_color(tokens::text3())
        .child(text.into())
}

pub(super) fn list_row(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .h(px(28.))
        .flex_none()
        .px(px(12.))
        .gap(px(8.))
        .items_center()
        .cursor_pointer()
        .when(selected, |d| {
            d.bg(tokens::accent_soft()).text_color(tokens::check_on())
        })
        .when(!selected, |d| d.hover(|h| h.bg(tokens::hover())))
}

pub(super) fn search_field(input: &Entity<InputState>, glyph: &'static str) -> AnyElement {
    h_flex()
        .h(tokens::input_height())
        .mx(px(8.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded(tokens::radius())
        .border_1()
        .border_color(tokens::border())
        .bg(tokens::field_select())
        .child(
            div()
                .text_color(tokens::text3())
                .child(ui::icon(glyph, 13.)),
        )
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h(tokens::input_height())
                .flex_1(),
        )
        .into_any_element()
}

pub(super) fn json_area(input: &Entity<TextareaState>) -> AnyElement {
    div()
        .w_full()
        .h(px(150.))
        .p(px(6.))
        .rounded(tokens::radius())
        .border_1()
        .border_color(tokens::border())
        .bg(tokens::field_select())
        .font_family(tokens::font_mono())
        .text_size(px(12.))
        .child(Textarea::new(input))
        .into_any_element()
}

pub(super) fn value_view(
    value: &Value,
    collapsed: &BTreeSet<String>,
    view: ValueView,
    target: TreeTarget,
    cx: &mut Context<DataStoresWindow>,
) -> AnyElement {
    let tag = match target {
        TreeTarget::Entry => "entry",
        TreeTarget::MapItem => "map",
    };
    if view == ValueView::Raw {
        return div()
            .id(SharedString::from(format!("ds-raw-{tag}")))
            .max_h(px(480.))
            .overflow_y_scroll()
            .p(px(8.))
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .font_family(tokens::font_mono())
            .text_size(px(12.))
            .child(json_tree::pretty(value))
            .into_any_element();
    }
    let mut column = v_flex()
        .p(px(4.))
        .rounded(tokens::radius())
        .bg(tokens::field_select())
        .font_family(tokens::font_mono())
        .text_size(px(12.));
    for (i, row) in json_tree::rows(value, collapsed).into_iter().enumerate() {
        let color = match row.kind {
            Kind::String => tokens::diff_add(),
            Kind::Number => tokens::check_on(),
            Kind::Bool => tokens::warning(),
            Kind::Object | Kind::Array | Kind::Null => tokens::text3(),
        };
        let path = row.path.clone();
        let expandable = row.expandable;
        let chevron = if row.collapsed {
            "chevron-right"
        } else {
            "chevron-down"
        };
        column = column.child(
            h_flex()
                .id(SharedString::from(format!("ds-tree-{tag}-{i}")))
                .h(px(22.))
                .pl(px(row.depth as f32 * 14.))
                .gap(px(4.))
                .items_center()
                .child(
                    div()
                        .w(px(12.))
                        .flex_none()
                        .text_color(tokens::text3())
                        .when(expandable, |d| d.child(ui::icon(chevron, 12.))),
                )
                .when(!row.label.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_none()
                            .text_color(tokens::text2())
                            .child(format!("{}:", row.label)),
                    )
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(color)
                        .child(row.preview.clone()),
                )
                .when(expandable, |d| {
                    d.cursor_pointer().on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_tree(target, path.clone(), cx)
                        }),
                    )
                }),
        );
    }
    column.into_any_element()
}

/// "2026-10-19T21:50:03Z" as "2026-10-19 21:50 UTC"; Roblox always answers in UTC.
pub(super) fn time_label(rfc3339: Option<&str>) -> String {
    let Some(text) = rfc3339 else {
        return "\u{2014}".to_string();
    };
    match (text.get(..10), text.get(10..11), text.get(11..16)) {
        (Some(date), Some("T"), Some(time)) => format!("{date} {time} UTC"),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::time_label;

    #[test]
    fn time_label_formats_utc() {
        assert_eq!(
            time_label(Some("2026-10-19T21:50:03.123Z")),
            "2026-10-19 21:50 UTC"
        );
        assert_eq!(time_label(None), "\u{2014}");
        assert_eq!(time_label(Some("soon")), "soon");
    }
}
