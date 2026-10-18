//! The inspector beside the table: every saved property of the focused
//! context, action or binding, each with the editor its dump type takes.

use gpui_kit::assets::IconName;
use gpui_kit::component::select::Select;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use super::super::rows::{checkbox, select_field, text_field};
use super::super::{chrome, tooltip};
use super::Shell;
use crate::input_actions::check::Issue;
use crate::shell::chrome::ScrollbarY as _;
use crate::tokens;

impl Shell {
    /// `None` when nothing is focused or the focused instance is gone.
    pub(super) fn input_inspector(
        &mut self,
        issues: &[Issue],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let referent = self.input_edits.focused?;
        let class = self.dom.get(referent)?.class().to_owned();
        let name = self.dom.get(referent)?.name().to_owned();

        let name_input = self.input_text(referent, "Name", &name, window, cx);
        self.tab_order
            .register(&name_input.read(cx).focus_handle(cx));
        let name_tab = self.tab_order.next();
        let name_field = text_field(&name_input, name_tab, cx);

        let header = h_flex()
            .w_full()
            .flex_none()
            .items_center()
            .gap(tokens::label_gap())
            .p(tokens::panel_padding())
            .child(
                div()
                    .flex_none()
                    .text_color(tokens::text_muted())
                    .child(SharedString::from(class.clone())),
            )
            .child(div().flex_1().min_w_0().child(name_field))
            .child(
                chrome::icon_button("input-inspector-delete", IconName::Trash, "Delete").on_click(
                    cx.listener(move |shell, _, _, cx| shell.delete_input_instance(referent, cx)),
                ),
            );

        let warnings: Vec<AnyElement> = if self.input_edits.checking {
            issues
                .iter()
                .filter(|issue| issue.target == referent)
                .enumerate()
                .map(|(index, issue)| {
                    h_flex()
                        .id(("input-inspector-issue", index))
                        .w_full()
                        .px(tokens::row_padding())
                        .gap(tokens::label_gap())
                        .items_center()
                        .text_color(tokens::warning())
                        .child(Icon::new(IconName::TriangleAlert).size(px(12.)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(SharedString::from(issue.message.clone())),
                        )
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };

        let properties: Vec<(String, String)> = self
            .database
            .class(&class)
            .map(|descriptor| {
                descriptor
                    .properties
                    .iter()
                    .filter(|property| property.can_save && !property.is_read_only())
                    .map(|property| (property.name.to_string(), property.value_type.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let mut rows = Vec::with_capacity(properties.len());
        for (property, value_type) in properties {
            let value =
                crate::input_actions::value_of(&self.dom, &self.database, referent, &property)
                    .cloned();
            let widget =
                self.input_value_widget(referent, &property, &value_type, value, window, cx);
            let id = SharedString::from(format!("input-prop:{}:{property}", referent.value()));
            let tip = SharedString::from(property.clone());
            rows.push(
                h_flex()
                    .id(id.clone())
                    .w_full()
                    .flex_none()
                    .min_h(tokens::row_height())
                    .items_center()
                    .gap(tokens::label_gap())
                    .px(tokens::row_padding())
                    .child(
                        div()
                            .id(SharedString::from(format!("{id}:name")))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens::text_muted())
                            .tooltip(move |window, cx| tooltip::text(tip.clone(), window, cx))
                            .child(SharedString::from(property)),
                    )
                    .child(div().flex_1().min_w_0().child(widget))
                    .into_any_element(),
            );
        }

        let handle = self.input_edits.inspector_scroll.clone();
        Some(
            v_flex()
                .w(px(300.))
                .flex_none()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .h_full()
                .border_l_1()
                .border_color(tokens::border())
                .child(header)
                .children(warnings)
                .child(
                    div()
                        .id("input-inspector-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&handle)
                        .child(v_flex().w_full().children(rows))
                        .scrollbar_y(&handle),
                )
                .into_any_element(),
        )
    }

    fn input_value_widget(
        &mut self,
        referent: Ref,
        property: &str,
        value_type: &str,
        value: Option<Variant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if value_type == "bool" {
            let on = matches!(value, Some(Variant::Bool(true)));
            let shell = cx.entity().downgrade();
            let property = property.to_owned();
            let id = SharedString::from(format!("input-check:{}:{property}", referent.value()));
            return checkbox(id, on, move |_, _, cx| {
                let _ = shell.update(cx, |shell, cx| {
                    shell.commit_input_value(
                        referent,
                        &property,
                        if on { "false" } else { "true" },
                        cx,
                    )
                });
            })
            .into_any_element();
        }
        if self.database.enum_items(value_type).is_some() {
            let current = match &value {
                Some(Variant::Enum(item)) => self
                    .database
                    .enum_name(value_type, *item)
                    .map(str::to_owned),
                _ => None,
            }
            .or_else(|| {
                self.database
                    .enum_items(value_type)
                    .and_then(|items| items.first().map(|(name, _)| name.clone()))
            })
            .unwrap_or_else(|| "None".to_owned());
            let state = self.input_enum(referent, property, value_type, &current, window, cx);
            let handle = state.read(cx).focus_handle(cx);
            self.tab_order.register(&handle);
            return select_field(&handle, window, cx)
                .child(
                    Select::new(&state)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full()
                        .py_0()
                        .pt(tokens::select_inset()),
                )
                .into_any_element();
        }
        if self.database.class(value_type).is_some() {
            return self.input_reference_widget(referent, property, value_type, &value, cx);
        }
        let seed = value
            .as_ref()
            .and_then(crate::properties::edit::edit_text)
            .unwrap_or_default();
        let input = self.input_text(referent, property, &seed, window, cx);
        self.tab_order.register(&input.read(cx).focus_handle(cx));
        let tab = self.tab_order.next();
        text_field(&input, tab, cx).into_any_element()
    }

    fn input_reference_widget(
        &mut self,
        referent: Ref,
        property: &str,
        value_type: &str,
        value: &Option<Variant>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = match value {
            Some(Variant::Ref(target)) => self.dom.get(*target).map(|t| t.name().to_owned()),
            _ => None,
        };
        let candidate = self.selected().filter(|selected| {
            self.dom
                .get(*selected)
                .is_some_and(|i| self.database.is_subclass_of(i.class(), value_type))
        });
        let key = format!("{}:{property}", referent.value());
        let (use_property, clear_property) = (property.to_owned(), property.to_owned());
        let label = match target {
            Some(name) => div().child(SharedString::from(name)),
            None => div()
                .text_color(tokens::text_muted())
                .child(SharedString::from("None")),
        };
        h_flex()
            .w_full()
            .items_center()
            .gap(tokens::label_gap())
            .child(label.flex_1().min_w_0().truncate())
            .child(
                chrome::button(
                    SharedString::from(format!("input-use:{key}")),
                    "Use selection",
                    false,
                )
                .when(candidate.is_none(), |this| this.opacity(0.5))
                .when_some(candidate, |this, selected| {
                    this.on_click(cx.listener(move |shell, _, _, cx| {
                        shell.commit_input_value(
                            referent,
                            &use_property,
                            &selected.value().to_string(),
                            cx,
                        )
                    }))
                }),
            )
            .child(
                chrome::icon_button(
                    SharedString::from(format!("input-clear:{key}")),
                    IconName::Trash,
                    "Clear",
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.commit_input_value(referent, &clear_property, "nil", cx)
                })),
            )
            .into_any_element()
    }
}
