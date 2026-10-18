//! The dock's table: one section per context, one row per action, one
//! column per device family.

use gpui_kit::assets::IconName;
use gpui_kit::component::select::Select;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use super::super::rows::{checkbox, select_field, text_field};
use super::super::{chrome, tooltip};
use super::Shell;
use crate::input_actions::check::{Issue, IssueKind};
use crate::input_actions::{value_of, Action, Binding, Context as InputContext, Family, Model};
use crate::shell::chrome::ScrollbarY as _;
use crate::tokens;

const ACTION_WIDTH: f32 = 180.;
const TYPE_WIDTH: f32 = 120.;
const CELL_MIN_WIDTH: f32 = 120.;

fn column(label: &'static str) -> Div {
    div()
        .text_size(tokens::text_xs())
        .font_weight(tokens::WEIGHT_BOLD)
        .text_color(tokens::text_muted())
        .child(label)
}

fn cell() -> Div {
    h_flex()
        .flex_1()
        .min_w(px(CELL_MIN_WIDTH))
        .flex_wrap()
        .items_center()
        .gap(tokens::label_gap())
}

fn warning_icon(id: SharedString, messages: Vec<String>) -> Stateful<Div> {
    let tip = SharedString::from(messages.join(" "));
    div()
        .id(id)
        .flex_none()
        .text_color(tokens::warning())
        .child(Icon::new(IconName::TriangleAlert).size(tokens::text_sm()))
        .tooltip(move |window, cx| tooltip::text(tip.clone(), window, cx))
}

fn messages_for(issues: &[Issue], target: Ref) -> Vec<String> {
    issues
        .iter()
        .filter(|issue| issue.target == target)
        .map(|issue| issue.message.clone())
        .collect()
}

impl Shell {
    /// The scrolling table, or the empty state when the place has no
    /// context and no action.
    pub(super) fn input_table(
        &mut self,
        model: &Model,
        issues: &[Issue],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if model.contexts.is_empty() && model.loose.is_empty() {
            return self.input_empty(cx);
        }
        let mut rows: Vec<AnyElement> = vec![self.input_column_header()];
        for context in &model.contexts {
            rows.push(self.input_context_header(context, window, cx));
            if !self.input_edits.collapsed.contains(&context.referent) {
                for action in &context.actions {
                    rows.push(self.input_action_row(action, issues, window, cx));
                }
            }
        }
        if !model.loose.is_empty() {
            rows.push(
                div()
                    .w_full()
                    .flex_none()
                    .px(tokens::row_padding())
                    .py(tokens::row_padding())
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_muted())
                    .child("Not in an InputContext")
                    .into_any_element(),
            );
            for action in &model.loose {
                rows.push(self.input_action_row(action, issues, window, cx));
            }
        }
        let scroll = self.input_edits.table_scroll.clone();
        div()
            .id("input-table")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(v_flex().w_full().children(rows))
            .scrollbar_y(&scroll)
            .into_any_element()
    }

    fn input_column_header(&self) -> AnyElement {
        h_flex()
            .w_full()
            .flex_none()
            .items_center()
            .gap(tokens::label_gap())
            .px(tokens::row_padding())
            .py(tokens::row_padding())
            .border_b_1()
            .border_color(tokens::border())
            .child(column("Action").w(px(ACTION_WIDTH)).flex_none())
            .child(column("Type").w(px(TYPE_WIDTH)).flex_none())
            .children(
                Family::ALL
                    .iter()
                    .map(|family| column(family.label()).flex_1().min_w(px(CELL_MIN_WIDTH))),
            )
            .child(column("Other").flex_1().min_w(px(CELL_MIN_WIDTH)))
            .into_any_element()
    }

    fn input_row(&self, id: SharedString, referent: Ref, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = self.input_edits.focused == Some(referent);
        h_flex()
            .id(id)
            .w_full()
            .flex_none()
            .min_h(tokens::row_height())
            .items_center()
            .gap(tokens::label_gap())
            .px(tokens::row_padding())
            .rounded(tokens::radius_row())
            .map(|this| match selected {
                true => this.bg(tokens::selection()),
                false => this.hover(|this| this.bg(tokens::hover())),
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| shell.select(referent, cx)),
            )
    }

    fn input_field(
        &mut self,
        referent: Ref,
        property: &str,
        seed: &str,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let input = self.input_text(referent, property, seed, window, cx);
        self.tab_order.register(&input.read(cx).focus_handle(cx));
        let index = self.tab_order.next();
        div()
            .w(px(width))
            .flex_none()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(text_field(&input, index, cx))
    }

    fn input_toggle(
        &self,
        referent: Ref,
        property: &'static str,
        checked: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let shell = cx.entity().downgrade();
        checkbox(
            SharedString::from(format!("input-{property}-{}", referent.value())),
            checked,
            move |_, _, cx| {
                let text = if checked { "false" } else { "true" };
                let _ = shell.update(cx, |shell, cx| {
                    shell.commit_input_value(referent, property, text, cx)
                });
            },
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn input_button(
        &mut self,
        id: SharedString,
        icon: IconName,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        chrome::icon_button(id, icon, label)
            .track_focus(&self.tab_order.claim(cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn input_context_header(
        &mut self,
        context: &InputContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let r = context.referent;
        let key = r.value();
        let open = !self.input_edits.collapsed.contains(&r);
        let (icon, label) = match open {
            true => (IconName::ChevronDown, "Collapse"),
            false => (IconName::ChevronRight, "Expand"),
        };
        let chevron = self
            .input_button(format!("input-chevron-{key}").into(), icon, label, cx)
            .on_click(cx.listener(move |shell, _, _, cx| {
                if !shell.input_edits.collapsed.remove(&r) {
                    shell.input_edits.collapsed.insert(r);
                }
                cx.notify();
            }));
        let name = self.input_field(r, "Name", &context.name, ACTION_WIDTH, window, cx);
        let enabled = self.input_toggle(r, "Enabled", context.enabled, cx);
        let sink_checked = context.sink;
        let priority = self.input_field(
            r,
            "Priority",
            &context.priority.to_string(),
            72.,
            window,
            cx,
        );
        let sink = self.input_toggle(r, "Sink", sink_checked, cx);
        let add = self
            .input_button(
                format!("input-add-action-{key}").into(),
                IconName::Plus,
                "Add action",
                cx,
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.add_input_action(r, cx)));
        let delete = self
            .input_button(
                format!("input-delete-{key}").into(),
                IconName::Trash,
                "Delete",
                cx,
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.delete_input_instance(r, cx)));
        let muted = || {
            div()
                .flex_none()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_muted())
        };
        self.input_row(format!("input-context-{key}").into(), r, cx)
            .child(chevron)
            .child(name)
            .child(muted().child("Enabled"))
            .child(enabled)
            .child(muted().child("Priority"))
            .child(priority)
            .child(muted().child("Sink"))
            .child(sink)
            .child(div().flex_1())
            .child(add)
            .child(delete)
            .into_any_element()
    }

    fn input_action_row(
        &mut self,
        action: &Action,
        issues: &[Issue],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let r = action.referent;
        let key = r.value();
        let current = match value_of(&self.dom, &self.database, r, "Type") {
            Some(Variant::Enum(value)) => self
                .database
                .enum_name("InputActionType", *value)
                .unwrap_or("Bool")
                .to_owned(),
            _ => "Bool".to_owned(),
        };
        let name = self.input_field(r, "Name", &action.name, ACTION_WIDTH - 24., window, cx);
        let outside: Vec<String> = issues
            .iter()
            .filter(|i| i.target == r && i.kind == IssueKind::OutsideContext)
            .map(|i| i.message.clone())
            .collect();
        let name_cell = h_flex()
            .w(px(ACTION_WIDTH))
            .flex_none()
            .items_center()
            .gap(tokens::label_gap())
            .child(name)
            .when(!outside.is_empty(), |this| {
                this.child(warning_icon(format!("input-outside-{key}").into(), outside))
            });
        let state = self.input_enum(r, "Type", "InputActionType", &current, window, cx);
        let handle = state.read(cx).focus_handle(cx);
        self.tab_order.register(&handle);
        let type_cell = div()
            .w(px(TYPE_WIDTH))
            .flex_none()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                select_field(&handle, window, cx).child(
                    Select::new(&state)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full()
                        .py_0()
                        .pt(tokens::select_inset()),
                ),
            );
        let mut row = self
            .input_row(format!("input-action-{key}").into(), r, cx)
            .child(name_cell)
            .child(type_cell);
        for (index, family) in Family::ALL.into_iter().enumerate() {
            let missing = issues
                .iter()
                .any(|i| i.target == r && i.kind == IssueKind::MissingFamily(family));
            let chips: Vec<AnyElement> = action
                .bindings
                .iter()
                .filter(|b| b.families.contains(&family))
                .map(|b| self.input_chip(b, index, issues, cx))
                .collect();
            let add = self
                .input_button(
                    format!("input-add-binding-{key}-{index}").into(),
                    IconName::Plus,
                    "Add binding",
                    cx,
                )
                .on_click(
                    cx.listener(move |shell, _, _, cx| shell.add_input_binding(r, family, cx)),
                );
            row = row.child(
                cell()
                    .children(chips)
                    .when(missing, |this| {
                        this.child(
                            div()
                                .text_size(tokens::text_xs())
                                .text_color(tokens::warning())
                                .child("missing"),
                        )
                    })
                    .child(add),
            );
        }
        let others: Vec<AnyElement> = action
            .bindings
            .iter()
            .filter(|b| b.families.is_empty())
            .map(|b| self.input_chip(b, Family::ALL.len(), issues, cx))
            .collect();
        row.child(cell().children(others)).into_any_element()
    }

    fn input_chip(
        &self,
        binding: &Binding,
        column: usize,
        issues: &[Issue],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let b = binding.referent;
        let selected = self.input_edits.focused == Some(b);
        let messages = messages_for(issues, b);
        h_flex()
            .id(SharedString::from(format!(
                "input-chip-{}-{column}",
                b.value()
            )))
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(tokens::row_padding())
            .min_h(tokens::row_height())
            .rounded(tokens::radius_row())
            .border_1()
            .border_color(tokens::border())
            .text_size(tokens::text_sm())
            .text_color(tokens::text_strong())
            .map(|this| match selected {
                true => this.bg(tokens::selection()),
                false => this.hover(|this| this.bg(tokens::hover())),
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| {
                    shell.select(b, cx);
                    cx.stop_propagation();
                }),
            )
            .when(!messages.is_empty(), |this| {
                this.child(warning_icon(
                    format!("input-chip-warn-{}-{column}", b.value()).into(),
                    messages,
                ))
            })
            .child(SharedString::from(binding.summary()))
            .into_any_element()
    }
}
