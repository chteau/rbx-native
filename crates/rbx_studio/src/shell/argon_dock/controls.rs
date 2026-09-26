//! The settings cards' dropdown and stepper, and the subscriptions that
//! turn a stepper's text and buttons into setting changes.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInputEvent, StepAction};
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::argon::{Setting, Value};
use crate::tokens;

use super::super::menu::{self, MenuId};
use super::super::{chrome, Shell};

fn menu_id(setting: Setting) -> MenuId {
    match setting {
        Setting::InitialSyncPriority => MenuId::ArgonSyncPriority,
        Setting::DisplayPrompts => MenuId::ArgonDisplayPrompts,
        _ => MenuId::ArgonLogLevel,
    }
}

impl Shell {
    /// 26px, `panel` on a `border2` hairline: the value and a chevron.
    pub(super) fn dropdown(
        &mut self,
        setting: Setting,
        current: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let items = setting
            .choices()
            .iter()
            .map(|&choice| {
                menu::item(choice)
                    .checked(choice == current)
                    .on_click(move |shell, cx| shell.argon_set(setting, Value::Choice(choice), cx))
            })
            .collect();
        let trigger = h_flex()
            .id(SharedString::from(format!("argon-{}", setting.key())))
            .tab_index(self.tab_order.next())
            .flex_none()
            .h(px(26.))
            .px(px(9.))
            .items_center()
            .gap(px(6.))
            .rounded(tokens::radius())
            .bg(tokens::dock())
            .border_1()
            .border_color(tokens::border2())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text2())
            .cursor_pointer()
            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::field_select())))
            .child(current)
            .child(Icon::new(IconName::ChevronDown).size(px(10.)));
        menu::dropdown(
            self,
            menu_id(setting),
            chrome::Trigger::new(trigger),
            items,
            cx,
        )
    }

    /// 88×26: `panel` on a `border2` hairline, 26px − and + split off by a
    /// `border` hairline, the value centred in mono.
    pub(super) fn stepper(&mut self, setting: Setting, cx: &mut Context<Self>) -> impl IntoElement {
        let state = match setting {
            Setting::DiffLinesLimit => self.argon_ui.diff_limit.clone(),
            _ => self.argon_ui.threshold.clone(),
        };
        self.tab_order.register(&state.read(cx).focus_handle(cx));
        let step = |button: gpui_kit::base::Button, minus: bool| {
            button
                .w(px(26.))
                .h_full()
                .flex_none()
                .text_color(tokens::text2())
                .border_color(tokens::border())
                .map(|this| {
                    if minus {
                        this.border_r_1().rounded_l(px(4.))
                    } else {
                        this.border_l_1().rounded_r(px(4.))
                    }
                })
                .hover(|this| {
                    tokens::hover_fx(this)
                        .bg(tokens::hover())
                        .text_color(tokens::text())
                })
                .child(
                    Icon::new(if minus {
                        IconName::Minus
                    } else {
                        IconName::Plus
                    })
                    .size(px(12.)),
                )
        };
        gpui_kit::base::NumberInput::new(&state)
            .w(px(88.))
            .h(px(26.))
            .flex_none()
            .rounded(tokens::radius())
            .border_1()
            .border_color(tokens::border2())
            .bg(tokens::dock())
            .decrement_button(move |button| step(button, true))
            .increment_button(move |button| step(button, false))
            .input(
                Input::new(&state)
                    .appearance(false)
                    .h_full()
                    .px(px(0.))
                    .text_center()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_md())
                    .text_color(tokens::text()),
            )
    }
}

/// A stepper's text, as typed: a whole number becomes the setting at the
/// level being edited; anything else leaves it alone, the plugin's
/// `filterNumber` keeping digits only (`Settings.luau:198-262`).
pub(super) fn watch_number(
    input: &Entity<InputState>,
    setting: Setting,
    cx: &mut Context<Shell>,
) -> Subscription {
    cx.subscribe(input, move |shell, input, event: &InputEvent, cx| {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        if let Ok(n) = input.read(cx).value().trim().parse::<u32>() {
            shell.argon_set(setting, Value::Number(n), cx);
        }
    })
}

/// The − and + buttons: one step is one, never below zero.
pub(super) fn watch_steps(
    input: &Entity<InputState>,
    setting: Setting,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        move |shell, input, event: &NumberInputEvent, window, cx| {
            let NumberInputEvent::Step(action) = event;
            let current = match shell.argon_shown(setting) {
                Value::Number(n) => n,
                _ => return,
            };
            let next = match action {
                StepAction::Increment => current.saturating_add(1),
                StepAction::Decrement => current.saturating_sub(1),
            };
            shell.argon_set(setting, Value::Number(next), cx);
            input.update(cx, |state, cx| {
                state.set_value(next.to_string(), window, cx);
            });
        },
    )
}
