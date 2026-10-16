//! File › Customize Shortcuts: real Studio's own window for viewing and
//! rebinding every hotkey, a window of its own rather than a Settings page,
//! as Studio keeps it. Rows come from `crate::shortcuts::ALL`; every change
//! goes through `shortcuts::replace`, so it applies at once everywhere.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;

use crate::launcher::ui;
use crate::shortcuts::{self, Command, Spec, Store, ALL};
use crate::tokens;

use super::Shell;

mod row;

const WIDTH: f32 = 760.;
const HEIGHT: f32 = 640.;
const MIN_WIDTH: f32 = 560.;
const MIN_HEIGHT: f32 = 420.;

/// Opens the window with the editor, for a capture.
pub(crate) const OPEN_VARIABLE: &str = "RBX_STUDIO_SHORTCUTS";

/// The rows a search shows, in [`ALL`]'s order: a case-insensitive match on
/// the command's name, its category's heading, or one of its displayed keys.
fn filtered(store: &Store, query: &str) -> Vec<&'static Spec> {
    let query = query.trim().to_lowercase();
    ALL.iter()
        .filter(|spec| {
            query.is_empty()
                || spec.name.to_lowercase().contains(&query)
                || spec.category.label().to_lowercase().contains(&query)
                || store
                    .bindings(spec.command)
                    .iter()
                    .any(|key| shortcuts::display(key).to_lowercase().contains(&query))
        })
        .collect()
}

/// Which chip is waiting for a key: `slot` is the binding it replaces, or
/// `None` to add one.
#[derive(Clone, Copy, PartialEq)]
struct Recording {
    command: Command,
    slot: Option<usize>,
}

pub(crate) struct ShortcutsWindow {
    shell: Entity<Shell>,
    filter: Entity<InputState>,
    recording: Option<Recording>,
    /// The commands the last change's key also runs, shown under its row.
    conflict: Option<(Command, Vec<Command>)>,
    /// The window's own focus, so a recorded key reaches it rather than the
    /// search field.
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Brings Customize Shortcuts forward, opening it if it isn't.
    pub(crate) fn open_shortcuts(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.shortcuts_window {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`, which is
        // still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = ShortcutsWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.shortcuts_window = opened);
        });
    }
}

impl ShortcutsWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Customize Shortcuts".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| ShortcutsWindow::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts"));
        let subscriptions = vec![cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        ShortcutsWindow {
            shell,
            filter,
            recording: None,
            conflict: None,
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn start_recording(
        &mut self,
        command: Command,
        slot: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        self.recording = Some(Recording { command, slot });
        cx.notify();
    }

    /// Writes `store` to the app and has the editor redraw its hints.
    fn commit(&mut self, store: Store, cx: &mut Context<Self>) {
        shortcuts::replace(store, cx);
        self.shell.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn remove(&mut self, command: Command, slot: usize, cx: &mut Context<Self>) {
        let mut store = shortcuts::store();
        let mut keys = store.bindings(command);
        if slot < keys.len() {
            keys.remove(slot);
        }
        store.set(command, keys);
        self.recording = None;
        self.conflict = None;
        self.commit(store, cx);
    }

    fn reset(&mut self, command: Command, cx: &mut Context<Self>) {
        let mut store = shortcuts::store();
        store.reset(command);
        self.recording = None;
        self.conflict = None;
        self.commit(store, cx);
    }

    fn reset_all(&mut self, cx: &mut Context<Self>) {
        let mut store = shortcuts::store();
        store.reset_all();
        self.recording = None;
        self.conflict = None;
        self.commit(store, cx);
    }

    /// While recording, Escape cancels and the next bindable key becomes
    /// the binding; a clash is still applied, as Studio allows, and named.
    fn handle_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(recording) = self.recording else {
            return false;
        };
        let m = keystroke.modifiers;
        let bare = !(m.control || m.alt || m.shift || m.platform || m.function);
        if bare && keystroke.key == "escape" {
            self.recording = None;
            cx.notify();
            return true;
        }
        if !shortcuts::bindable(keystroke) {
            return false;
        }
        let key = Keystroke {
            key: keystroke.key.clone(),
            modifiers: keystroke.modifiers,
            key_char: None,
        };
        let command = recording.command;
        let mut store = shortcuts::store();
        let mut keys = store.bindings(command);
        match recording.slot {
            Some(slot) if slot < keys.len() => keys[slot] = key.clone(),
            _ => {
                if !keys.iter().any(|k| shortcuts::same_key(k, &key)) {
                    keys.push(key.clone());
                }
            }
        }
        store.set(command, keys);
        let conflicts = store.conflicts(command, &key);
        self.recording = None;
        self.conflict = (!conflicts.is_empty()).then_some((command, conflicts));
        self.commit(store, cx);
        true
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_none()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(tokens::border())
            .child(div().flex_1().child(Input::new(&self.filter)))
            .child(
                ui::button(
                    "shortcuts-reset-all",
                    "Reset all",
                    ui::Weight::Secondary,
                    true,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reset_all(cx))),
            )
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = shortcuts::store();
        let query = self.filter.read(cx).value().to_string();
        let specs = filtered(&store, &query);
        let mut list = v_flex().px_4().pb_4();
        let mut heading = None;
        for spec in &specs {
            if heading != Some(spec.category) {
                heading = Some(spec.category);
                list = list.child(
                    div()
                        .pt_4()
                        .pb_1()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text2())
                        .child(spec.category.label()),
                );
            }
            let recording = self
                .recording
                .filter(|r| r.command == spec.command)
                .map(|r| r.slot);
            let conflicts = self
                .conflict
                .as_ref()
                .filter(|(command, _)| *command == spec.command)
                .map(|(_, others)| others.as_slice());
            list = list.child(row::row(
                spec,
                store.bindings(spec.command),
                store.is_default(spec.command),
                recording,
                conflicts,
                cx,
            ));
        }
        if specs.is_empty() {
            list = list.child(
                div()
                    .pt_6()
                    .text_color(tokens::text3())
                    .child("No shortcut matches the search."),
            );
        }
        div()
            .id("shortcuts-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(list)
    }
}

impl Render for ShortcutsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("shortcuts-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens::dock())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.handle_key(&event.keystroke, cx) {
                    cx.stop_propagation();
                }
            }))
            .font_family(tokens::font_ui())
            .text_size(px(13.))
            .text_color(tokens::text())
            .child(super::chrome::window_topbar(
                "Customize Shortcuts".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(self.header(cx))
            .child(self.list(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::filtered;
    use crate::shortcuts::{Category, Command, Store, ALL};

    #[test]
    fn empty_query_lists_everything() {
        assert_eq!(filtered(&Store::default(), "  ").len(), ALL.len());
    }

    #[test]
    fn matches_name_case_insensitively() {
        let rows = filtered(&Store::default(), "save to");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, Command::Save);
    }

    #[test]
    fn matches_category_label() {
        let rows = filtered(&Store::default(), "debugger");
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|spec| spec.category == Category::Debug));
    }

    #[test]
    fn matches_displayed_key() {
        let rows = filtered(
            &Store::default(),
            &crate::shortcuts::display(&gpui_kit::Keystroke::parse("shift-f11").unwrap()),
        );
        assert!(rows.iter().any(|spec| spec.command == Command::StepOut));
    }

    #[test]
    fn follows_the_store_not_the_defaults() {
        let mut store = Store::default();
        store.set(Command::Save, Vec::new());
        assert!(!filtered(&store, "ctrl+s")
            .iter()
            .any(|spec| spec.command == Command::Save));
    }

    #[test]
    fn nothing_matches_gibberish() {
        assert!(filtered(&Store::default(), "zzqxv").is_empty());
    }
}
