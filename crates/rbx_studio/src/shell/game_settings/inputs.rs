//! The window's text boxes: made once, wired to their drafts as the user
//! types, and refilled from the drafts when a load or revert replaces them.

use std::collections::BTreeMap;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::catalog::{CloudField, LocalKind, LOCAL_FIELDS};
use super::model::{self, CloudValue};
use super::{GameSettingsWindow, InputKey};

impl GameSettingsWindow {
    pub(super) fn make_inputs(
        window: &mut Window,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<Self>,
    ) -> BTreeMap<InputKey, Entity<InputState>> {
        let mut keys = vec![
            InputKey::Cloud(CloudField::Name),
            InputKey::Cloud(CloudField::Description),
            InputKey::Cloud(CloudField::PlaceName),
            InputKey::Cloud(CloudField::PlaceDescription),
            InputKey::Cloud(CloudField::ServerSize),
            InputKey::Cloud(CloudField::PrivateServers),
        ];
        for (i, field) in LOCAL_FIELDS.iter().enumerate() {
            if matches!(field.kind, LocalKind::Float { .. }) {
                keys.push(InputKey::Local(i));
            }
        }
        let mut inputs = BTreeMap::new();
        for key in keys {
            let input = cx.new(|cx| InputState::new(window, cx));
            subscriptions.push(
                cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = input.read(cx).value().to_string();
                        this.on_input(key, &text, cx);
                    }
                }),
            );
            inputs.insert(key, input);
        }
        inputs
    }

    pub(super) fn on_input(&mut self, key: InputKey, text: &str, cx: &mut Context<Self>) {
        let result: Result<(), String> = match key {
            InputKey::Cloud(CloudField::ServerSize) => {
                match text
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=700).contains(n))
                {
                    Some(n) => {
                        self.set_cloud(CloudField::ServerSize, CloudValue::Count(n), cx);
                        Ok(())
                    }
                    None => Err("Enter a whole number from 1 to 700.".to_string()),
                }
            }
            InputKey::Cloud(CloudField::PrivateServers) => {
                let trimmed = text.trim();
                let price = if trimmed.is_empty() {
                    Some(None)
                } else {
                    trimmed
                        .parse::<u32>()
                        .ok()
                        .filter(|n| (10..=1_000_000).contains(n))
                        .map(Some)
                };
                match price {
                    Some(p) => {
                        self.set_cloud(CloudField::PrivateServers, CloudValue::Price(p), cx);
                        Ok(())
                    }
                    None => Err("Enter a price from 10 to 1000000 Robux.".to_string()),
                }
            }
            InputKey::Cloud(field) => {
                self.set_cloud(field, CloudValue::Text(text.to_string()), cx);
                Ok(())
            }
            InputKey::Local(i) => {
                let parsed =
                    model::parse_local(&LOCAL_FIELDS[i], &self.shell.read(cx).database, text);
                parsed.map(|value| self.set_local(i, value, cx))
            }
        };
        match result {
            Ok(()) => {
                self.invalid.remove(&key);
            }
            Err(why) => {
                self.invalid.insert(key, why);
            }
        }
        cx.notify();
    }

    /// Fills each input with its draft's value; called from render. Setting
    /// a value emits no change, so the refilled inputs' errors are cleared here.
    pub(super) fn reseed_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reseed = false;
        for (key, input) in &self.inputs {
            let text = match *key {
                InputKey::Cloud(field) => match self.cloud.value(field) {
                    Some(CloudValue::Text(t)) | Some(CloudValue::Fixed(t)) => t,
                    Some(CloudValue::Count(n)) | Some(CloudValue::Price(Some(n))) => n.to_string(),
                    _ => String::new(),
                },
                InputKey::Local(i) => match self.local.value(i) {
                    Some(rbx_dom::Variant::Float32(v)) => format!("{v}"),
                    _ => String::new(),
                },
            };
            self.invalid.remove(key);
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }
}
