//! The page's rows: each setting's label, description and control, built
//! from the drafts and wired to change them.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Variant;

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Mutex;

use crate::launcher::ui::{self, Weight};

use super::super::super::settings_window::kit;
use super::super::catalog::{CloudField, LocalKind, LOCAL_FIELDS};
use super::super::model::CloudValue;
use super::super::{tokens, GameSettingsWindow, InputKey, Loading};
use super::id;

/// Enum item names live in the reflection database; the segmented control
/// wants `'static` labels, so each distinct name is leaked once.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut guard = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(found) = set.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

type Pick = Rc<dyn Fn(&mut Window, &mut App)>;

impl GameSettingsWindow {
    pub(super) fn cloud_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let fields: Vec<CloudField> = CloudField::ALL
            .iter()
            .copied()
            .filter(|f| f.tab() == self.tab)
            .collect();
        if fields.is_empty() {
            return Vec::new();
        }
        let state_row = |content: AnyElement| vec![plain_row().child(content).into_any_element()];
        if self.link.is_none() {
            return state_row(
                h_flex()
                    .w_full()
                    .gap(px(12.))
                    .items_center()
                    .child(
                        kit::text(12.5, 18.)
                            .flex_1()
                            .child("This file isn\u{2019}t linked with a Roblox place."),
                    )
                    .child(
                        ui::button(
                            "gs-link",
                            "Link to Roblox Place\u{2026}",
                            Weight::Primary,
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.link_place(cx))),
                    )
                    .into_any_element(),
            );
        }
        match &self.loading {
            Loading::Busy => {
                return state_row(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(ui::spinner("gs-loading", 16.))
                        .child(kit::text(12.5, 18.).child("Loading settings\u{2026}"))
                        .into_any_element(),
                )
            }
            Loading::Failed(message) => {
                return state_row(
                    h_flex()
                        .w_full()
                        .gap(px(12.))
                        .items_center()
                        .child(
                            kit::text(12.5, 18.)
                                .flex_1()
                                .text_color(ui::red())
                                .child(message.clone()),
                        )
                        .child(
                            ui::button("gs-retry", "Try again", Weight::Secondary, true)
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        )
                        .into_any_element(),
                )
            }
            Loading::Idle => {}
        }
        fields
            .into_iter()
            .filter_map(|field| {
                let value = self.cloud.value(field)?;
                let control = self.cloud_control(field, value, cx);
                let error = self.invalid.get(&InputKey::Cloud(field)).cloned();
                Some(row(
                    field.label(),
                    field.description(),
                    (!field.read_only()).then(|| field.scope()),
                    self.cloud.is_edited(field),
                    control,
                    error,
                ))
            })
            .collect()
    }

    fn cloud_control(
        &self,
        field: CloudField,
        value: CloudValue,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = InputKey::Cloud(field);
        match value {
            CloudValue::Bool(on) => kit::toggle(
                id("gs-cloud", format!("{field:?}")),
                on,
                cx.listener(move |this, _, _, cx| this.set_cloud(field, CloudValue::Bool(!on), cx)),
            )
            .into_any_element(),
            CloudValue::Text(_) | CloudValue::Count(_) => self.field_box(key, 260.),
            CloudValue::Price(price) => h_flex()
                .gap(px(10.))
                .items_center()
                .when(price.is_some(), |d| d.child(self.field_box(key, 110.)))
                .child(kit::toggle(
                    id("gs-cloud", format!("{field:?}")),
                    price.is_some(),
                    cx.listener(move |this, _, _, cx| {
                        let next = if price.is_some() { None } else { Some(100) };
                        this.invalid.remove(&key);
                        this.reseed = true;
                        this.set_cloud(field, CloudValue::Price(next), cx);
                    }),
                ))
                .into_any_element(),
            CloudValue::Fixed(text) => kit::text(12.5, 18.)
                .text_color(tokens::text2())
                .child(text)
                .into_any_element(),
        }
    }

    fn field_box(&self, key: InputKey, width: f32) -> AnyElement {
        let Some(input) = self.inputs.get(&key) else {
            return div().into_any_element();
        };
        div()
            .w(px(width))
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(5.))
            .border_1()
            .border_color(tokens::border2())
            .bg(tokens::field_select())
            .child(Input::new(input).appearance(false).text_size(px(12.)))
            .into_any_element()
    }

    pub(super) fn local_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for (i, field) in LOCAL_FIELDS.iter().enumerate() {
            if field.tab != self.tab {
                continue;
            }
            let control = match self.local.value(i).cloned() {
                None => kit::text(12., 16.)
                    .text_color(tokens::text3())
                    .child(format!("No {} in this place", field.class))
                    .into_any_element(),
                Some(value) => match field.kind {
                    LocalKind::Bool => {
                        let on = matches!(value, Variant::Bool(true));
                        kit::toggle(
                            id("gs-local", i),
                            on,
                            cx.listener(move |this, _, _, cx| {
                                this.set_local(i, Variant::Bool(!on), cx)
                            }),
                        )
                        .into_any_element()
                    }
                    LocalKind::Float { .. } => self.field_box(InputKey::Local(i), 110.),
                    LocalKind::Enum(name) => self.enum_control(i, name, &value, cx),
                },
            };
            let error = self.invalid.get(&InputKey::Local(i)).cloned();
            rows.push(row(
                field.label,
                field.description,
                None,
                self.local.is_edited(i),
                control,
                error,
            ));
        }
        rows
    }

    fn enum_control(
        &self,
        index: usize,
        name: &'static str,
        value: &Variant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let items: Vec<(String, u32)> = self
            .shell
            .read(cx)
            .database
            .enum_items(name)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let current = match value {
            Variant::Enum(v) => *v,
            _ => u32::MAX,
        };
        let me = cx.entity();
        if items.len() <= 4 {
            let picks = items
                .iter()
                .map(|(label, v)| {
                    let (me, v) = (me.clone(), *v);
                    let pick: Pick = Rc::new(move |_, cx| {
                        me.update(cx, |this, cx| this.set_local(index, Variant::Enum(v), cx))
                    });
                    (intern(label), v == current, pick)
                })
                .collect();
            return kit::segmented("gs-segmented", 26., picks).into_any_element();
        }
        let position = items.iter().position(|(_, v)| *v == current);
        let shown = position.map_or("Choose", |p| intern(&items[p].0));
        let next = items
            .get(position.map_or(0, |p| (p + 1) % items.len()))
            .map(|(_, v)| *v);
        ui::button(id("gs-enum", index), shown, Weight::Secondary, true)
            .on_click(move |_, _, cx| {
                if let Some(v) = next {
                    me.update(cx, |this, cx| this.set_local(index, Variant::Enum(v), cx));
                }
            })
            .into_any_element()
    }
}

pub(super) fn plain_row() -> Div {
    h_flex()
        .px(px(14.))
        .py(px(10.))
        .border_b_1()
        .border_color(tokens::border())
}

pub(super) fn row(
    label: &'static str,
    description: impl IntoElement,
    scope: Option<&'static str>,
    edited: bool,
    control: AnyElement,
    error: Option<String>,
) -> AnyElement {
    v_flex()
        .px(px(14.))
        .py(px(10.))
        .gap(px(4.))
        .border_b_1()
        .border_color(tokens::border())
        .child(
            h_flex()
                .gap(px(16.))
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .gap(px(6.))
                                .items_center()
                                .child(kit::text(12.5, 18.).child(label))
                                .when(edited, |d| {
                                    d.child(
                                        div().size(px(6.)).rounded_full().bg(tokens::accent_line()),
                                    )
                                }),
                        )
                        .child(
                            kit::text(11.5, 16.)
                                .text_color(tokens::text2())
                                .child(description),
                        )
                        .children(
                            scope
                                .map(|s| kit::mono(10.5, 14.).text_color(tokens::text3()).child(s)),
                        ),
                )
                .child(div().flex_none().child(control)),
        )
        .children(error.map(|e| kit::text(11.5, 16.).text_color(ui::red()).child(e)))
        .into_any_element()
}
