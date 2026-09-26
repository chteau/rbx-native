//! The Watch and Call Stack docks, and the debug controls over the Script
//! Editor.
//!
//! Watch has Studio's two tabs: Variables (the paused function's locals
//! and upvalues) and My Watches (expressions typed in, re-evaluated at
//! every pause). Call Stack lists the paused script's Luau frames,
//! innermost first.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_lua::Resume;

use crate::tokens;

use super::super::layout::Panel;
use super::super::{chrome, menu, Shell};
use super::Watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum WatchTab {
    #[default]
    Variables,
    MyWatches,
}

impl Shell {
    pub(in crate::shell) fn watch_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let input = self.watch_input(window, cx);
        if std::mem::take(&mut self.debug.clear_watch_input) {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        let overflow = self.dock_overflow(Panel::Watch, "watch-overflow", cx);

        let tab = self.debug.tab;
        let tabs = h_flex()
            .w_full()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(tokens::border())
            .children(
                [
                    (WatchTab::Variables, "watch-variables", "Variables"),
                    (WatchTab::MyWatches, "watch-mine", "My Watches"),
                ]
                .map(|(this_tab, id, label)| {
                    div()
                        .id(id)
                        .px_2()
                        .rounded(tokens::radius())
                        .text_size(tokens::text_xs())
                        .cursor_pointer()
                        .text_color(if tab == this_tab {
                            tokens::text_strong()
                        } else {
                            tokens::text2()
                        })
                        .when(tab == this_tab, |this| this.bg(tokens::selection()))
                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.debug.tab = this_tab;
                            cx.notify();
                        }))
                        .child(label)
                }),
            );

        let pause = self.debug.run.as_ref().and_then(|run| run.pause.as_ref());
        let rows: Vec<AnyElement> = match tab {
            WatchTab::Variables => match pause {
                None => vec![placeholder(self.debug_running())],
                Some(pause) if pause.variables.is_empty() => {
                    vec![note("No variables in scope").into_any_element()]
                }
                Some(pause) => pause
                    .variables
                    .iter()
                    .map(|variable| {
                        value_row(&variable.name, Some(Ok(variable.value.clone())))
                            .into_any_element()
                    })
                    .collect(),
            },
            WatchTab::MyWatches => self
                .debug
                .watches
                .iter()
                .enumerate()
                .map(|(index, watch)| self.watch_row(index, watch, cx))
                .collect(),
        };

        let body = v_flex()
            .size_full()
            .child(tabs)
            .child(
                div()
                    .id("watch-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(v_flex().w_full().children(rows)),
            )
            .when(tab == WatchTab::MyWatches, |this| {
                this.child(
                    div()
                        .flex_none()
                        .p_1()
                        .border_t_1()
                        .border_color(tokens::border())
                        .child(Input::new(&input).xsmall()),
                )
            });
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    pub(in crate::shell) fn call_stack_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = self.dock_overflow(Panel::CallStack, "call-stack-overflow", cx);
        let run = self.debug.run.as_ref();
        let selected = run.map_or(0, |run| run.frame);
        let pause = run.and_then(|run| run.pause.as_ref());
        let rows: Vec<AnyElement> = match pause {
            None => vec![placeholder(self.debug_running())],
            Some(pause) => pause
                .stack
                .iter()
                .enumerate()
                .map(|(index, frame)| {
                    h_flex()
                        .id(("call-stack-frame", index))
                        .w_full()
                        .cursor_pointer()
                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.select_frame(index, cx);
                        }))
                        .gap_2()
                        .px_2()
                        .py_0p5()
                        .text_size(tokens::text_xs())
                        .when(index == selected, |this| this.bg(tokens::hover()))
                        .child(
                            div()
                                .flex_1()
                                .text_color(tokens::text_strong())
                                .child(frame.function.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(tokens::text2())
                                .child(format!("Line {}", frame.line)),
                        )
                        .into_any_element()
                })
                .collect(),
        };
        let body = div()
            .id("call-stack-list")
            .size_full()
            .overflow_y_scroll()
            .child(v_flex().w_full().children(rows));
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    /// Run, or Resume / the three steps / Stop while a run is live — the
    /// row Studio puts in its mezzanine, here over the editor it drives.
    pub(in crate::shell) fn debug_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let control =
            |id: &'static str, icon: IconName, label: &'static str, resume: Option<Resume>| {
                Button::new(id)
                    .icon(icon)
                    .ghost()
                    .xsmall()
                    .tooltip(label)
                    .accessibility_label(label)
                    .on_click(cx.listener(move |shell, _, _, cx| match resume {
                        Some(resume) => shell.resume_debugging(resume, cx),
                        None => shell.stop_debugging(cx),
                    }))
            };
        let paused = self.debug_paused();
        h_flex()
            .flex_none()
            .gap_0p5()
            .px_1()
            .map(|this| match self.debug_running() {
                false => this.child(
                    Button::new("debug-run")
                        .icon(IconName::BugPlay)
                        .ghost()
                        .xsmall()
                        .tooltip("Debug this script (F5)")
                        .accessibility_label("Debug this script")
                        .on_click(cx.listener(|shell, _, _, cx| shell.start_debugging(cx))),
                ),
                true => this
                    .when(paused, |this| {
                        this.child(control(
                            "debug-resume",
                            IconName::Play,
                            "Resume (F5)",
                            Some(Resume::Continue),
                        ))
                        .child(control(
                            "debug-step-into",
                            IconName::ArrowDownToDot,
                            "Step Into (F11)",
                            Some(Resume::StepInto),
                        ))
                        .child(control(
                            "debug-step-over",
                            IconName::RedoDot,
                            "Step Over (F10)",
                            Some(Resume::StepOver),
                        ))
                        .child(control(
                            "debug-step-out",
                            IconName::ArrowUpFromDot,
                            "Step Out (Shift+F11)",
                            Some(Resume::StepOut),
                        ))
                    })
                    .child(control(
                        "debug-stop",
                        IconName::Square,
                        "Stop (Shift+F5)",
                        None,
                    )),
            })
    }

    fn dock_overflow(
        &self,
        panel: Panel,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let menu_id = match panel {
            Panel::Watch => menu::MenuId::WatchOverflow,
            _ => menu::MenuId::CallStackOverflow,
        };
        menu::dropdown(
            self,
            menu_id,
            chrome::Trigger::new(chrome::dock_options_button(
                id,
                IconName::Ellipsis,
                16.,
                "Dock options",
            )),
            self.move_items(panel),
            cx,
        )
    }

    fn watch_row(&self, index: usize, watch: &Watch, cx: &mut Context<Self>) -> AnyElement {
        value_row(&watch.expression, watch.value.clone())
            .child(
                Button::new(("remove-watch", index))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .accessibility_label("Remove watch")
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        if index < shell.debug.watches.len() {
                            shell.debug.watches.remove(index);
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The "add a watch" field, made the first time the dock is drawn.
    fn watch_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = &self.debug.watch_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a watch expression"));
        let subscription = cx.subscribe(&input, |shell, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let expression = input.read(cx).value().trim().to_owned();
                if !expression.is_empty() {
                    shell.debug.watches.push(Watch {
                        expression,
                        value: None,
                    });
                    shell.request_watch_values();
                }
                // Emptied on the next render: `set_value` needs a `Window`,
                // which a subscription does not get.
                shell.debug.clear_watch_input = true;
                cx.notify();
            }
        });
        self.debug.watch_input = Some(input.clone());
        self.debug.watch_subscription = Some(subscription);
        input
    }
}

/// One name beside its value; an error is shown in place of the value.
fn value_row(name: &str, value: Option<Result<String, String>>) -> Div {
    let (text, colour) = match value {
        None => (String::new(), tokens::text3()),
        Some(Ok(value)) => (value, tokens::text()),
        Some(Err(error)) => (error, tokens::text_error()),
    };
    h_flex()
        .w_full()
        .gap_2()
        .px_2()
        .py_0p5()
        .text_size(tokens::text_xs())
        .child(
            div()
                .w(relative(0.35))
                .flex_none()
                .overflow_hidden()
                .text_color(tokens::text_strong())
                .child(name.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_color(colour)
                .child(text),
        )
}

fn placeholder(running: bool) -> AnyElement {
    note(if running {
        "Running — values show when the script pauses"
    } else {
        "Values show while a debugged script is paused"
    })
    .into_any_element()
}

fn note(text: &'static str) -> Div {
    div()
        .px_2()
        .py_1()
        .text_size(tokens::text_xs())
        .text_color(tokens::text3())
        .child(text)
}
