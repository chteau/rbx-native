//! The Argon page: the plugin's fifteen settings, the same ones the Argon
//! dock shows, at a scope picked here — Global, or the connected project's
//! Game or Place. At Game and Place a row that doesn't override shows what
//! it inherits, and changing it creates the override.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::argon::{Level, LevelKeys, Setting, Value};
use crate::tokens;

use super::super::argon_dock::settings::{copy, SECTIONS};
use super::super::Shell;
use super::dragger::number;
use super::kit::{self, segmented, text, toggle, Row, Section};
use super::SettingsWindow;

mod controls;

pub(super) use controls::ArgonControls;

/// Whether `level` can hold overrides with `keys`: Global always, Game and
/// Place only while connected to a project that has them.
fn available(level: Level, keys: &LevelKeys) -> bool {
    match level {
        Level::Global => true,
        Level::Game => keys.game.is_some(),
        Level::Place => keys.place.is_some(),
    }
}

fn shown(value: &Value) -> SharedString {
    match value {
        Value::Bool(true) => "On".into(),
        Value::Bool(false) => "Off".into(),
        Value::Choice(choice) => SharedString::from(*choice),
        Value::Number(n) => n.to_string().into(),
    }
}

impl SettingsWindow {
    /// The scope being edited: the one picked, while it can hold overrides.
    fn argon_scope(&self, cx: &App) -> Level {
        let keys = self.shell.read(cx).argon_level_keys();
        let picked = self.argon.scope;
        if available(picked, &keys) {
            picked
        } else {
            Level::Global
        }
    }

    fn argon_write(&mut self, setting: Setting, value: Value, cx: &mut Context<Self>) {
        let scope = self.argon_scope(cx);
        self.shell.update(cx, |shell, cx| {
            shell.argon_set_at(scope, setting, value, cx)
        });
    }

    /// "Restore defaults" at Global, "Clear overrides" at Game and Place;
    /// inert while the scope holds nothing.
    pub(super) fn argon_header(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let scope = self.argon_scope(cx);
        let shell = self.shell.read(cx);
        let keys = shell.argon_level_keys();
        let any = Setting::ALL
            .iter()
            .any(|setting| shell.argon_settings.exact(*setting, scope, &keys).is_some());
        let label = if scope == Level::Global {
            "Restore defaults"
        } else {
            "Clear overrides"
        };
        let shell = self.shell.clone();
        kit::header_button(
            "argon-restore",
            label,
            any.then_some(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.argon_restore_at(scope, cx))
            }),
        )
    }

    /// Scope, the three levels, and what the picked one means.
    pub(super) fn argon_scope_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let scope = self.argon_scope(cx);
        let (keys, project) = {
            let shell = self.shell.read(cx);
            (shell.argon_level_keys(), shell.argon_project_name())
        };
        let hint = match scope {
            Level::Global if keys.place.is_none() => {
                "Every place. Connect to an Argon project to override one game or place.".to_owned()
            }
            Level::Global => "Every place, unless a game or place overrides it.".to_owned(),
            Level::Game => format!(
                "Every place in {project}\u{2019}s game. Overrides show a dot; reset one to fall back."
            ),
            Level::Place => {
                format!("Only {project}. Overrides show a dot; reset one to fall back.")
            }
        };
        let window = cx.entity().downgrade();
        let items = [Level::Global, Level::Game, Level::Place]
            .into_iter()
            .filter(|level| available(*level, &keys))
            .map(|level| {
                let window = window.clone();
                let pick: kit::OnPick = std::rc::Rc::new(move |_, cx| {
                    let _ = window.update(cx, |this, cx| {
                        this.argon.scope = level;
                        cx.notify();
                    });
                });
                (level.label(), level == scope, pick)
            })
            .collect();
        h_flex()
            .gap(px(12.))
            .py(px(12.))
            .px(px(14.))
            .items_center()
            .border_1()
            .border_color(tokens::border())
            .rounded(px(8.))
            .bg(tokens::field_select())
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Scope"),
            )
            .child(segmented("argon-scope", 26., items))
            .child(
                text(11.5, 16.)
                    .flex_1()
                    .text_color(tokens::text2())
                    .child(hint),
            )
            .into_any_element()
    }

    pub(super) fn argon_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let scope = self.argon_scope(cx);
        let (settings, keys, address) = {
            let shell = self.shell.read(cx);
            (
                shell.argon_settings.clone(),
                shell.argon_level_keys(),
                shell.argon_saved_address.clone(),
            )
        };
        // What each row shows: its own override at this scope, else what
        // it inherits.
        let at = |setting: Setting| {
            settings
                .exact(setting, scope, &keys)
                .unwrap_or_else(|| settings.inherited(setting, scope, &keys).1)
        };
        self.sync_argon_controls(&at, window, cx);
        let chip = match scope {
            Level::Place => Some("This place"),
            Level::Game => Some("This game"),
            Level::Global => None,
        };
        let focused: Vec<(Setting, bool)> = self
            .argon
            .numbers
            .iter()
            .map(|(setting, input)| (*setting, input.read(cx).focus_handle(cx).is_focused(window)))
            .collect();

        SECTIONS
            .iter()
            .map(|(title, section)| {
                let mut rows: Vec<Row> = section
                    .iter()
                    .map(|setting| {
                        let setting = *setting;
                        let value = at(setting);
                        let own = settings.exact(setting, scope, &keys).is_some();
                        let control: AnyElement = match &value {
                            Value::Bool(on) => {
                                let on = *on;
                                toggle(
                                    SharedString::from(format!("argon-{}", setting.key())),
                                    on,
                                    self.set(move |shell, cx| {
                                        shell.argon_set_at(scope, setting, Value::Bool(!on), cx)
                                    }),
                                )
                                .into_any_element()
                            }
                            Value::Choice(_) => self.argon_select(setting).into_any_element(),
                            Value::Number(_) => {
                                let focused = focused.iter().any(|(s, f)| *s == setting && *f);
                                let input = self.argon_number(setting);
                                number(&input, "", focused).into_any_element()
                            }
                        };
                        let (name, description) = copy(setting);
                        let inherited = (scope != Level::Global && !own).then(|| {
                            let (from, value) = settings.inherited(setting, scope, &keys);
                            format!("{} \u{b7} {}", from.label(), shown(&value))
                        });
                        let mut row = Row::new(
                            name,
                            h_flex()
                                .gap(px(8.))
                                .items_center()
                                .children(inherited.map(|line| {
                                    div()
                                        .text_size(px(11.))
                                        .text_color(tokens::text3())
                                        .child(line)
                                }))
                                .child(control),
                        )
                        .describe(description)
                        .changed(own, move |shell: &mut Shell, cx| {
                            shell.argon_clear_at(scope, setting, cx)
                        });
                        if own {
                            if let Some(chip) = chip {
                                row = row.chip(chip);
                            }
                        }
                        row
                    })
                    .collect();
                if *title == "CONNECTION" {
                    rows.push(self.argon_address_row(&address, scope));
                }
                Section::new(title, rows)
            })
            .collect()
    }

    /// The address is not one of the plugin's levelled settings: it is the
    /// last one Connect succeeded with, the dock's own. Shown, not edited —
    /// the dock's field is where an address is typed and tried.
    fn argon_address_row(&self, address: &str, scope: Level) -> Row {
        let address = if address.is_empty() {
            "localhost:8000".to_owned()
        } else {
            address.to_owned()
        };
        Row::new(
            "Server address",
            h_flex()
                .gap(px(8.))
                .items_center()
                .when(scope != Level::Global, |this| {
                    this.child(
                        div()
                            .text_size(px(11.))
                            .text_color(tokens::text3())
                            .child(format!("Global \u{b7} {address}")),
                    )
                })
                .child(
                    h_flex()
                        .w(px(180.))
                        .h(px(30.))
                        .px(px(10.))
                        .items_center()
                        .border_1()
                        .border_color(tokens::border2())
                        .rounded(px(6.))
                        .bg(tokens::dock())
                        .child(kit::mono(11.5, 16.).child(address)),
                ),
        )
        .describe("The last address that connected.")
    }
}
