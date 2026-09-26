//! The Argon page's controls that keep state of their own: a dropdown per
//! choice setting and a field per number setting, each written back from
//! the value its row shows.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::{h_flex, Icon, IndexPath, Sizable as _};
use gpui_kit::*;

use crate::settings::argon::{Level, Setting, Value};
use crate::tokens;

use super::super::SettingsWindow;

type Choices = SearchableVec<SharedString>;

/// `RBX_STUDIO_SETTINGS_ARGON_SCOPE=game|place`: the scope the page opens
/// on, for a capture.
const SCOPE_VARIABLE: &str = "RBX_STUDIO_SETTINGS_ARGON_SCOPE";

/// The page's dropdowns and number fields, which keep state of their own.
pub(in crate::shell::settings_window) struct ArgonControls {
    pub(super) scope: Level,
    pub(super) selects: Vec<(Setting, Entity<SelectState<Choices>>)>,
    pub(super) numbers: Vec<(Setting, Entity<InputState>)>,
}

impl ArgonControls {
    pub(in crate::shell::settings_window) fn new(
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Vec<Subscription>) {
        let mut subscriptions = Vec::new();
        let mut selects = Vec::new();
        let mut numbers = Vec::new();
        for setting in Setting::ALL {
            match setting.default() {
                Value::Choice(_) => {
                    let labels: Vec<SharedString> = setting
                        .choices()
                        .iter()
                        .map(|c| SharedString::from(*c))
                        .collect();
                    let state =
                        cx.new(|cx| SelectState::new(SearchableVec::new(labels), None, window, cx));
                    subscriptions.push(cx.subscribe(
                        &state,
                        move |this, _, event: &SelectEvent<Choices>, cx| {
                            let SelectEvent::Confirm(Some(label)) = event else {
                                return;
                            };
                            if let Some(choice) =
                                setting.choices().iter().find(|c| **c == label.as_ref())
                            {
                                this.argon_write(setting, Value::Choice(choice), cx);
                            }
                        },
                    ));
                    selects.push((setting, state));
                }
                Value::Number(_) => {
                    let input = cx.new(|cx| InputState::new(window, cx));
                    subscriptions.push(cx.subscribe(
                        &input,
                        move |this, input, event: &InputEvent, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            if let Ok(n) = input.read(cx).value().trim().parse::<u32>() {
                                this.argon_write(setting, Value::Number(n), cx);
                            }
                        },
                    ));
                    numbers.push((setting, input));
                }
                Value::Bool(_) => {}
            }
        }
        let scope = match std::env::var(SCOPE_VARIABLE).as_deref() {
            Ok("game") => Level::Game,
            Ok("place") => Level::Place,
            _ => Level::Global,
        };
        let controls = ArgonControls {
            scope,
            selects,
            numbers,
        };
        (controls, subscriptions)
    }
}

impl SettingsWindow {
    pub(super) fn argon_select(&self, setting: Setting) -> impl IntoElement {
        let state = self
            .argon
            .selects
            .iter()
            .find(|(s, _)| *s == setting)
            .map(|(_, state)| state.clone())
            .expect("a select for every choice setting");
        h_flex()
            .w(px(120.))
            .h(px(30.))
            .px(px(10.))
            .items_center()
            .border_1()
            .border_color(tokens::border2())
            .rounded(px(6.))
            .bg(tokens::dock())
            .text_size(px(12.))
            .line_height(px(16.))
            .child(
                Select::new(&state)
                    .appearance(false)
                    .with_size(tokens::field_size())
                    .w_full()
                    .py_0()
                    .px_0()
                    .icon(
                        Icon::new(IconName::ChevronDown)
                            .size(px(12.))
                            .text_color(tokens::text()),
                    )
                    .menu_width(px(120.)),
            )
    }

    pub(super) fn argon_number(&self, setting: Setting) -> Entity<InputState> {
        self.argon
            .numbers
            .iter()
            .find(|(s, _)| *s == setting)
            .map(|(_, input)| input.clone())
            .expect("a field for every number setting")
    }

    /// Puts each dropdown and field back on the value its row shows, after
    /// a scope change or an edit from the dock.
    pub(super) fn sync_argon_controls(
        &self,
        at: &dyn Fn(Setting) -> Value,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (setting, state) in &self.argon.selects {
            let Value::Choice(choice) = at(*setting) else {
                continue;
            };
            let row = setting.choices().iter().position(|c| *c == choice);
            if state.read(cx).selected_index(cx).map(|index| index.row) != row {
                state.update(cx, |state, cx| {
                    state.set_selected_index(row.map(IndexPath::new), window, cx)
                });
            }
        }
        for (setting, input) in &self.argon.numbers {
            let Value::Number(n) = at(*setting) else {
                continue;
            };
            let state = input.read(cx);
            if (window.is_window_active() && state.focus_handle(cx).is_focused(window))
                || state.value().trim().parse::<u32>().ok() == Some(n)
            {
                continue;
            }
            input.update(cx, |state, cx| state.set_value(n.to_string(), window, cx));
        }
    }
}
