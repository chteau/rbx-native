//! The Preset menu and Save: picking an entry stages it; Save names the
//! staged values and applies them.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Choice;
use crate::launcher::ui;
use crate::shell::Shell;
use crate::tokens;

impl Shell {
    pub(super) fn network_pick(
        &mut self,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.play.network.stage(choice);
        if let Some(popover) = &mut self.play.network.popover {
            popover.menu_open = false;
        }
        self.network_sync_inputs(window, cx);
    }

    pub(super) fn network_start_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = match &self.play.network.shown().choice {
            Choice::Saved(name) => name.clone(),
            _ => String::new(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        input.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |shell, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => shell.network_finish_save(window, cx),
                InputEvent::Change => {
                    if let Some(popover) = &mut shell.play.network.popover {
                        popover.save_error = None;
                    }
                    cx.notify();
                }
                _ => {}
            },
        );
        if let Some(popover) = &mut self.play.network.popover {
            popover.saving = Some(input);
            popover._subscriptions.push(subscription);
        }
        cx.notify();
    }

    pub(super) fn network_finish_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self
            .play
            .network
            .popover
            .as_ref()
            .and_then(|p| p.saving.clone())
        else {
            return;
        };
        let name = input.read(cx).value().to_string();
        match self.play.network.save(&name) {
            Ok(()) => {
                if let Some(popover) = &mut self.play.network.popover {
                    popover.saving = None;
                }
                self.save_settings();
                self.network_apply(cx);
                self.network_sync_inputs(window, cx);
            }
            Err(error) => {
                if let Some(popover) = &mut self.play.network.popover {
                    popover.save_error = Some(error);
                }
                cx.notify();
            }
        }
    }

    /// The Preset list: Off, the built-ins, then saved presets.
    pub(super) fn network_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let network = &self.play.network;
        let current = network.shown().choice.clone();
        let mut entries = vec![(Choice::Off, "Off (no simulation)".to_owned())];
        entries.extend(
            rbx_play::netsim::PRESETS
                .iter()
                .enumerate()
                .map(|(i, preset)| (Choice::BuiltIn(i), preset.name.to_owned())),
        );
        entries.extend(
            network
                .saved
                .iter()
                .map(|preset| (Choice::Saved(preset.name.clone()), preset.name.clone())),
        );
        v_flex()
            .p(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel())
            .children(entries.into_iter().enumerate().map(|(i, (choice, label))| {
                let selected = choice == current;
                h_flex()
                    .id(("network-preset-entry", i))
                    .h(px(26.))
                    .px(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(selected, |this| this.bg(tokens::accent_soft()))
                    .hover(|this| this.bg(ui::wash()))
                    .child(ui::text(12., 16.).flex_1().truncate().child(label))
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.network_pick(choice.clone(), window, cx)
                    }))
            }))
    }
}
