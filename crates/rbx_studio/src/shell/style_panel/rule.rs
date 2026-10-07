//! An open rule's lines: its header, its priority, its property overrides
//! and the field that adds one.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::super::chrome;
use super::super::tooltip;
use super::view::flip;
use super::{Shell, Target};
use crate::style_editor::{self, PRIORITY_PROPERTY, SELECTOR_PROPERTY};
use crate::tokens;

impl Shell {
    /// A rule: a chevron that shows and hides its properties, and its
    /// selector, always editable. A closed rule says how much it holds.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rule_header(
        &mut self,
        rule: Ref,
        depth: usize,
        selector: &str,
        open: bool,
        selected: bool,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = rule.value() as usize;
        let selector_field = self.style_field(
            format!("selector:{id}"),
            selector,
            "Selector, e.g. .Card",
            Target::Instance {
                referent: rule,
                property: SELECTOR_PROPERTY.to_owned(),
            },
            window,
            cx,
        );
        let chevron = self.style_chevron(
            ("style-rule-toggle", id),
            open,
            move |edits| flip(&mut edits.open_rules, rule),
            cx,
        );
        let field = self.style_input(&selector_field, cx);

        self.style_line(("style-rule", id), rule, selected, depth + 1, cx)
            .child(chevron)
            .child(div().flex_1().min_w_0().child(field))
            .when(!open, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(tokens::text_sm())
                        .text_color(tokens::text_muted())
                        .child(match count {
                            1 => SharedString::from("1 property"),
                            n => format!("{n} properties").into(),
                        }),
                )
            })
            .into_any_element()
    }

    /// A name-and-value line under an open rule, in the Properties panel's
    /// proportions: the name takes what the value leaves, and a trailing
    /// slot keeps every value column the same width whether or not the
    /// line has a remove button.
    #[allow(clippy::too_many_arguments)]
    fn property_line(
        &self,
        id: SharedString,
        rule: Ref,
        depth: usize,
        name: &str,
        value: AnyElement,
        trailing: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tip = SharedString::from(name.to_owned());
        self.style_line(id.clone(), rule, false, depth + 2, cx)
            .child(
                div()
                    .id(SharedString::from(format!("{id}:name")))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(tokens::text_muted())
                    .tooltip(move |window, cx| tooltip::text(tip.clone(), window, cx))
                    .child(SharedString::from(name.to_owned())),
            )
            .child(div().flex_1().min_w_0().child(value))
            .child(trailing.unwrap_or_else(|| {
                div()
                    .flex_none()
                    .size(tokens::hit_target())
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// An open rule's own `Priority`, first among its lines.
    pub(super) fn priority_line(
        &mut self,
        rule: Ref,
        depth: usize,
        priority: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = rule.value() as usize;
        let input = self.style_field(
            format!("priority:{id}"),
            &priority.to_string(),
            "0",
            Target::Instance {
                referent: rule,
                property: PRIORITY_PROPERTY.to_owned(),
            },
            window,
            cx,
        );
        let field = self.style_input(&input, cx).into_any_element();
        self.property_line(
            format!("style-priority:{id}").into(),
            rule,
            depth,
            "Priority",
            field,
            None,
            cx,
        )
    }

    /// One of a rule's property overrides. A value whose type this editor
    /// cannot type back in (see `properties::edit::edit_text`) shows as
    /// text with no field, the way the Properties panel leaves such a row
    /// read-only.
    pub(super) fn property_override(
        &mut self,
        rule: Ref,
        depth: usize,
        name: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("prop:{}:{name}", rule.value());
        let value = match value {
            Some(seed) => {
                let input = self.style_field(
                    key.clone(),
                    seed,
                    "",
                    Target::RuleProperty {
                        rule,
                        name: name.to_owned(),
                    },
                    window,
                    cx,
                );
                self.style_input(&input, cx).into_any_element()
            }
            None => div()
                .truncate()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_disabled())
                .child("Not editable here")
                .into_any_element(),
        };
        let dropped = name.to_owned();
        let remove = chrome::danger_icon_button(
            SharedString::from(format!("style-drop:{key}")),
            IconName::X,
            "Remove property",
        )
        .track_focus(&self.tab_order.claim(cx))
        .on_click(cx.listener(move |shell, _, _, cx| {
            let name = dropped.clone();
            shell.apply_style_edit(
                move |dom, database| style_editor::remove_rule_property(dom, database, rule, &name),
                cx,
            );
        }));
        self.property_line(
            format!("style-{key}").into(),
            rule,
            depth,
            name,
            value,
            Some(remove.into_any_element()),
            cx,
        )
    }

    /// The last line of an open rule: one field taking `Name = value` (see
    /// `Target::NewProperty`), and a button that commits it for anyone who
    /// would rather not press Enter.
    pub(super) fn add_property_line(
        &mut self,
        rule: Ref,
        depth: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = rule.value() as usize;
        let key = SharedString::from(format!("add:{id}"));
        let input = self.style_field(
            key.clone(),
            "",
            "Add property: Name = value",
            Target::NewProperty { rule },
            window,
            cx,
        );
        let field = self.style_input(&input, cx);
        let add = chrome::icon_button(("style-add-property", id), IconName::Plus, "Add property")
            .track_focus(&self.tab_order.claim(cx))
            .on_click(cx.listener(move |shell, _, _, cx| shell.commit_style_field(&key, cx)));
        self.style_line(("style-add-line", id), rule, false, depth + 2, cx)
            .child(div().flex_1().min_w_0().child(field))
            .child(add)
            .into_any_element()
    }
}
