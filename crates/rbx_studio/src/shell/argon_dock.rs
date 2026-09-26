//! The Argon dock: the connection to `argon serve` on the left, the
//! plugin's settings on the right, and, under 900px of dock width, the
//! two stacked. Everything it keeps between frames lives in [`ArgonDock`];
//! what the connection does lives in `shell::argon_sync`, and what the
//! settings mean in `settings::argon`.
//!
//! The dock lays out for its own width, never the window's: a floated or
//! side-docked panel has to fit its own bounds. The width comes from the
//! previous layout pass ([`ArgonDock::width`]) and [`layout_for`] turns it
//! into one decision per frame, so nothing is measured mid-layout.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::InputState;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::argon::{Level, Setting, Value};
use crate::tokens;

use super::layout::Panel;
use super::menu::{self, MenuId};
use super::{chrome, Shell};

mod actions;
mod connection;
mod controls;
mod help;
pub(in crate::shell) mod settings;

pub(super) use connection::detect_argon_version;

/// Below this dock width the two columns stack.
const WIDE_MIN: f32 = 900.;
/// Content width: the stacked layout's 14px of padding on each side.
const STACKED_PADDING: f32 = 14.;
/// Content width at which the settings grid drops to one column.
const TWO_COLUMNS_MIN: f32 = 560.;
/// Content width under which "Restore defaults" is icon-only and card
/// descriptions may take two lines.
const COMPACT_MAX: f32 = 420.;
/// Content width under which the action row wraps onto two rows.
const WRAP_ACTIONS_MAX: f32 = 360.;
/// In the wide layout, the body scrolls as a whole when it is shorter than
/// the connection column needs (18 + 108 + 18).
const CONNECTION_MIN_HEIGHT: f32 = 144.;

/// What one dock width decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Layout {
    /// Two columns side by side with their own padding and scroll area;
    /// otherwise stacked under 14px padding, scrolling as one.
    pub(super) wide: bool,
    /// Cards per grid row.
    pub(super) columns: usize,
    /// "Restore defaults" as a 26×26 icon button.
    pub(super) compact_restore: bool,
    /// Card descriptions may wrap to two lines before the ellipsis.
    pub(super) clamp_descriptions: bool,
    /// The connection's action row wraps: field and "?" first, the button
    /// full width under them.
    pub(super) wrap_actions: bool,
}

/// The layout for a dock `width` pixels wide.
pub(super) fn layout_for(width: f32) -> Layout {
    if width >= WIDE_MIN {
        return Layout {
            wide: true,
            columns: 2,
            compact_restore: false,
            clamp_descriptions: false,
            wrap_actions: false,
        };
    }
    let content = width - 2. * STACKED_PADDING;
    Layout {
        wide: false,
        columns: if content >= TWO_COLUMNS_MIN { 2 } else { 1 },
        compact_restore: content < COMPACT_MAX,
        clamp_descriptions: content < COMPACT_MAX,
        wrap_actions: content < WRAP_ACTIONS_MAX,
    }
}

/// The dock's own state: its inputs, which settings level is being
/// edited, and the bounds the last frame gave it.
pub(super) struct ArgonDock {
    /// The address field's two halves. Together they are what
    /// `Settings::argon_address` remembers (see [`ArgonDock::address`]).
    host: Entity<InputState>,
    port: Entity<InputState>,
    /// The Changes Threshold and Diff Lines Limit steppers.
    threshold: Entity<InputState>,
    diff_limit: Entity<InputState>,
    /// The level the segmented control is editing.
    level: Level,
    /// The dock body's size after the last layout pass.
    width: Rc<Cell<f32>>,
    height: Rc<Cell<f32>>,
    body_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    /// Tracked by the address field's box, so the box can show the
    /// accent hairline while either of its two inputs has focus. Not a
    /// tab stop itself.
    field_focus: FocusHandle,
    /// Where the CLI was found and what version it reports, once a
    /// background task has looked for the binary and asked it (see
    /// [`detect_argon_version`]). `None` until then, and for good when
    /// there is no CLI: the badge is a late fill-in, never a startup stall.
    cli: Option<(std::path::PathBuf, SharedString)>,
    _subscriptions: Vec<Subscription>,
}

impl ArgonDock {
    pub(super) fn new(address: &str, window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let (host, port) = connection::split_address(address);
        let host = cx.new(|cx| InputState::new(window, cx).default_value(host));
        let port = cx.new(|cx| InputState::new(window, cx).default_value(port));
        let threshold = cx.new(|cx| InputState::new(window, cx).default_value("5"));
        let diff_limit = cx.new(|cx| InputState::new(window, cx).default_value("3000"));
        let subscriptions = vec![
            controls::watch_number(&threshold, Setting::ChangesThreshold, cx),
            controls::watch_number(&diff_limit, Setting::DiffLinesLimit, cx),
            controls::watch_steps(&threshold, Setting::ChangesThreshold, window, cx),
            controls::watch_steps(&diff_limit, Setting::DiffLinesLimit, window, cx),
        ];
        cx.spawn(async move |shell, cx| {
            let cli = cx.background_spawn(async { detect_argon_version() }).await;
            let _ = shell.update(cx, |shell, cx| {
                shell.argon_ui.cli = cli;
                cx.notify();
            });
        })
        .detach();
        ArgonDock {
            host,
            port,
            threshold,
            diff_limit,
            level: Level::Global,
            width: Rc::new(Cell::new(WIDE_MIN)),
            height: Rc::new(Cell::new(CONNECTION_MIN_HEIGHT)),
            body_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            field_focus: cx.focus_handle().tab_stop(false),
            cli: None,
            _subscriptions: subscriptions,
        }
    }

    /// The address as `host:port`, the shape the connection parses and the
    /// settings file stores.
    pub(super) fn address(&self, cx: &App) -> String {
        format!(
            "{}:{}",
            self.host.read(cx).value().trim(),
            self.port.read(cx).value().trim()
        )
    }

    /// Puts `host:port` into the two fields.
    pub(super) fn set_address(&self, address: &str, window: &mut Window, cx: &mut App) {
        let (host, port) = connection::split_address(address);
        self.host
            .update(cx, |state, cx| state.set_value(host, window, cx));
        self.port
            .update(cx, |state, cx| state.set_value(port, window, cx));
    }
}

impl Shell {
    /// The dock's trailing control and its body.
    pub(super) fn argon_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::ArgonOverflow,
            chrome::Trigger::new(chrome::dock_options_button(
                "argon-overflow",
                IconName::Ellipsis,
                16.,
                "Argon dock options",
            )),
            self.move_items(Panel::Argon),
            cx,
        );

        let layout = layout_for(self.argon_ui.width.get());
        let short = self.argon_ui.height.get() < CONNECTION_MIN_HEIGHT;
        let body = if layout.wide {
            self.wide_body(layout, short, cx)
        } else {
            self.stacked_body(layout, cx)
        };
        let width = self.argon_ui.width.clone();
        let height = self.argon_ui.height.clone();
        let shell = cx.entity_id();
        let measured = div()
            .size_full()
            .flex()
            .flex_col()
            // The size this frame gave the body is what the next frame lays
            // out for. When it changed, ask for that next frame now, so a
            // resize settles in one extra pass rather than waiting for the
            // next unrelated redraw.
            .on_children_prepainted(move |bounds, window, cx| {
                if let Some(bounds) = bounds.first() {
                    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    if (w - width.get()).abs() >= 0.5 || (h - height.get()).abs() >= 0.5 {
                        width.set(w);
                        height.set(h);
                        cx.notify(shell);
                        // A floated panel is its own window and doesn't
                        // redraw on the shell's notify; ask it directly, once
                        // this draw is over (a refresh mid-draw is dropped).
                        let handle = window.window_handle();
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, _| window.refresh());
                        });
                    }
                }
            })
            .child(body);

        (
            Some(overflow.into_any_element()),
            Some(measured.into_any_element()),
        )
    }

    /// Two columns: the connection at 440px, a hairline, the settings
    /// filling the rest with their own scroll area. When the body is too
    /// short for the connection column, the whole body scrolls instead.
    fn wide_body(&mut self, layout: Layout, short: bool, cx: &mut Context<Self>) -> AnyElement {
        let connection = self.argon_connection(layout, cx).w(px(440.)).flex_none();
        let settings = self.argon_settings_column(layout, !short, cx).h_full();
        h_flex()
            .id("argon-body-scroll")
            .size_full()
            .items_stretch()
            .px(px(20.))
            .py(px(18.))
            .gap(px(24.))
            .bg(tokens::dock())
            .when(short, |this| {
                this.overflow_y_scroll()
                    .track_scroll(&self.argon_ui.body_scroll)
                    .vertical_scrollbar(&self.argon_ui.body_scroll)
            })
            .child(connection)
            .child(
                div()
                    .w(px(1.))
                    .flex_none()
                    .self_stretch()
                    .bg(tokens::border()),
            )
            .child(settings)
            .into_any_element()
    }

    /// One column under 14px of padding, scrolling as a whole: the
    /// connection, an 18px gap, a hairline, 18px, the settings.
    fn stacked_body(&mut self, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let connection = self.argon_connection(layout, cx).w_full();
        let settings = self.argon_settings_column(layout, false, cx).w_full();
        v_flex()
            .id("argon-body-scroll")
            .size_full()
            .p(px(STACKED_PADDING))
            .gap(px(18.))
            .bg(tokens::dock())
            .overflow_y_scroll()
            .track_scroll(&self.argon_ui.body_scroll)
            .vertical_scrollbar(&self.argon_ui.body_scroll)
            .child(connection)
            .child(div().h(px(1.)).w_full().flex_none().bg(tokens::border()))
            .child(settings)
            .into_any_element()
    }

    /// The level the segmented control edits.
    pub(super) fn argon_level(&self) -> Level {
        self.argon_ui.level
    }

    pub(super) fn set_argon_level(
        &mut self,
        level: Level,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.argon_ui.level = level;
        self.sync_argon_fields(window, cx);
        cx.notify();
    }

    /// Writes one setting at the level being edited, then saves.
    pub(super) fn argon_set(&mut self, setting: Setting, value: Value, cx: &mut Context<Self>) {
        let keys = self.argon_level_keys();
        if self
            .argon_settings
            .set(setting, value, self.argon_ui.level, &keys)
        {
            self.save_settings();
        }
        cx.notify();
    }

    /// Clears every override at the level being edited (`Config.luau:162-174`).
    pub(super) fn argon_restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keys = self.argon_level_keys();
        if self
            .argon_settings
            .restore_defaults(self.argon_ui.level, &keys)
        {
            self.save_settings();
        }
        self.sync_argon_fields(window, cx);
        cx.notify();
    }

    /// What a card shows for `setting` at the level being edited: its own
    /// override, else the default — the plugin's binding
    /// (`Settings.luau:379`, `:391`).
    pub(super) fn argon_shown(&self, setting: Setting) -> Value {
        let keys = self.argon_level_keys();
        self.argon_settings
            .exact(setting, self.argon_ui.level, &keys)
            .unwrap_or_else(|| setting.default())
    }

    /// Puts the two steppers' text in step with what they show, after a
    /// level change or Restore defaults (their own edits already are).
    ///
    /// Also run every render, since Settings can write either value at the
    /// level this dock is showing; a field being typed in is left alone.
    pub(in crate::shell) fn sync_argon_fields(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (setting, field) in [
            (Setting::ChangesThreshold, self.argon_ui.threshold.clone()),
            (Setting::DiffLinesLimit, self.argon_ui.diff_limit.clone()),
        ] {
            if window.is_window_active() && field.read(cx).focus_handle(cx).is_focused(window) {
                continue;
            }
            if let Value::Number(n) = self.argon_shown(setting) {
                let text = n.to_string();
                field.update(cx, |state, cx| {
                    if state.value().as_ref() != text {
                        state.set_value(text, window, cx);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
