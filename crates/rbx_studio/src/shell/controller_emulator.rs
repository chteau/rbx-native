//! Test › Controller: Studio's Controller Emulator. A drawn pad you click,
//! drag and drive from the keyboard, and behind it a virtual gamepad
//! (`rbx_play::gamepad`) the Play client sees as real hardware.
//!
//! As in Studio, the pad is plugged in only while this window is open
//! during a Play session: opening it mid-session connects the device,
//! closing it unplugs it. The client must be allowed to see it, so a Play
//! started while the window is open (or with "Prepare every Play" on) adds
//! Sober's gamepad permission and the sandbox's `/dev/input` access — see
//! [`Shell::controller_client_options`].

mod drawing;
mod editor;
mod mapping;

use std::collections::HashSet;

use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_play::gamepad::{Button, Device, PadError, PadState, Side, VirtualPad};
use rbx_play::launch::ClientOptions;

use crate::launcher::ui;
use crate::tokens;

use super::play::Phase;
use super::settings_window::kit;
use super::Shell;
use editor::Editing;
use mapping::{Bindings, Saved};

const WIDTH: f32 = 640.;
const HEIGHT: f32 = 680.;

/// Opens the window with the editor, for a capture.
pub(crate) const OPEN_VARIABLE: &str = "RBX_STUDIO_CONTROLLER_EMULATOR";

/// Sober's own switch for reading gamepads; off by default.
const SOBER_GAMEPAD_KEY: &str = "allow_gamepad_permission";
/// Sober's flatpak grants only the GPU; the pad's event node lives under
/// `/dev/input`, which this opens to the sandbox.
const FLATPAK_INPUT: &str = "--device=input";

pub(crate) struct ControllerEmulator {
    shell: Entity<Shell>,
    saved: Saved,
    bindings: Bindings,
    /// Keys held down in this window that a binding uses.
    held: HashSet<String>,
    /// What clicks and drags on the drawing hold.
    pointer: PadState,
    pad: Option<VirtualPad>,
    /// Why the pad isn't connected, when it should be.
    problem: Option<String>,
    editing: Option<Editing>,
    /// A stick or trigger being dragged on the drawing.
    drag: Option<drawing::Drag>,
    /// Where the drawing's sticks and triggers were last laid out.
    geometry: drawing::Geometry,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Brings the Controller Emulator forward, opening it if it isn't.
    pub(in crate::shell) fn open_controller_emulator(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.controller_window {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = ControllerEmulator::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.controller_window = opened);
        });
    }

    /// Adds what the client needs to see the pad: when the window is open
    /// now (it will connect once Play starts) or the saved switch is on.
    pub(in crate::shell) fn controller_client_options(
        &mut self,
        options: &mut ClientOptions,
        cx: &mut Context<Self>,
    ) {
        let open = self
            .controller_window
            .is_some_and(|window| window.update(cx, |_, _, _| ()).is_ok());
        if open || Saved::load().enabled {
            options
                .sober_config
                .push((SOBER_GAMEPAD_KEY.into(), serde_json::Value::Bool(true)));
            options.flatpak_args.push(FLATPAK_INPUT.into());
        }
    }
}

impl ControllerEmulator {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: false,
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Controller Emulator".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| ControllerEmulator::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = Saved::load();
        let bindings = saved.bindings(saved.device);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        // Play starting and stopping is what plugs the pad in and out.
        let subscriptions = vec![cx.observe(&shell, |this, _, cx| this.sync_connection(cx))];
        let mut this = ControllerEmulator {
            shell,
            saved,
            bindings,
            held: HashSet::new(),
            pointer: PadState::default(),
            pad: None,
            problem: None,
            editing: None,
            drag: None,
            geometry: drawing::Geometry::default(),
            focus,
            _subscriptions: subscriptions,
        };
        this.sync_connection(cx);
        this
    }

    fn device(&self) -> Device {
        self.saved.device
    }

    fn playing(&self, cx: &App) -> bool {
        self.shell.read(cx).play.phase != Phase::Idle
    }

    /// Connects the pad while Play runs, disconnects it otherwise, and
    /// swaps it when the device changed.
    fn sync_connection(&mut self, cx: &mut Context<Self>) {
        let before = (self.pad.is_some(), self.problem.clone());
        let want = self.playing(cx) && self.device().unavailable().is_none();
        if self
            .pad
            .as_ref()
            .is_some_and(|pad| pad.device() != self.device())
            || !want
        {
            self.pad = None;
        }
        if want && self.pad.is_none() && self.problem.is_none() {
            match VirtualPad::create(self.device()) {
                Ok(pad) => {
                    self.pad = Some(pad);
                    self.push();
                }
                Err(error) => self.problem = Some(error.to_string()),
            }
        }
        if !want {
            self.problem = None;
        }
        // The Shell notifies on every change of its own; redraw only when
        // the connection did change.
        if before != (self.pad.is_some(), self.problem.clone()) {
            cx.notify();
        }
    }

    /// What the pad reports now: the drawing's state and the keyboard's.
    fn output(&self) -> PadState {
        mapping::merge(
            &self.pointer,
            &mapping::state_for(&self.held, &self.bindings),
        )
    }

    fn push(&mut self) {
        let output = self.output();
        if let Some(pad) = &mut self.pad {
            if let Err(error) = pad.send(output) {
                self.problem = Some(PadError::Io(error).to_string());
                self.pad = None;
            }
        }
    }

    fn select_device(&mut self, device: Device, cx: &mut Context<Self>) {
        self.saved.device = device;
        self.saved.save();
        self.bindings = self.saved.bindings(device);
        self.held.clear();
        self.pointer = PadState::default();
        self.editing = None;
        self.problem = None;
        self.sync_connection(cx);
    }

    fn toggle_enabled(&mut self, cx: &mut Context<Self>) {
        self.saved.enabled = !self.saved.enabled;
        self.saved.save();
        cx.notify();
    }

    pub(super) fn pointer_button(&mut self, button: Button, down: bool, cx: &mut Context<Self>) {
        self.pointer.set(button, down);
        self.push();
        cx.notify();
    }

    pub(super) fn pointer_stick(&mut self, side: Side, x: f32, y: f32, cx: &mut Context<Self>) {
        self.pointer.set_stick(side, x, y);
        self.push();
        cx.notify();
    }

    pub(super) fn pointer_trigger(&mut self, side: Side, value: f32, cx: &mut Context<Self>) {
        self.pointer.set_trigger(side, value);
        self.push();
        cx.notify();
    }

    /// Commits the mapping editor's draft for the current device.
    fn save_bindings(&mut self, bindings: Bindings, cx: &mut Context<Self>) {
        self.saved.set_bindings(self.device(), bindings.clone());
        self.saved.save();
        self.bindings = bindings;
        self.editing = None;
        cx.notify();
    }

    fn key_down(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if let Some(editing) = &mut self.editing {
            let handled = editing.record_key(key, self.saved.device);
            cx.notify();
            return handled;
        }
        if !self.bindings.values().any(|bound| bound == key) {
            return false;
        }
        if self.held.insert(key.to_owned()) {
            self.push();
            cx.notify();
        }
        true
    }

    fn key_up(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.held.remove(key) {
            self.push();
            cx.notify();
        }
    }

    fn status(&self, cx: &App) -> (SharedString, Rgba) {
        if let Some(reason) = self.device().unavailable() {
            return (format!("Not available: {reason}.").into(), tokens::text3());
        }
        if let Some(problem) = &self.problem {
            return (problem.clone().into(), tokens::text_error());
        }
        match &self.pad {
            Some(pad) => {
                let node = pad
                    .event_node()
                    .map(|node| format!(" as {}", node.display()))
                    .unwrap_or_default();
                (format!("Connected{node}.").into(), tokens::text2())
            }
            None if self.playing(cx) => ("Connecting…".into(), tokens::text2()),
            None => (
                "Not connected: the pad plugs in while a Play session runs.".into(),
                tokens::text3(),
            ),
        }
    }

    fn device_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_wrap()
            .gap(px(6.))
            .children(Device::ALL.into_iter().map(|device| {
                let selected = device == self.device();
                let pill = ui::pill(device.label(), None, selected);
                let id = SharedString::from(format!("controller-device-{}", device.key()));
                let item = div().id(ElementId::Name(id)).child(pill);
                match device.unavailable() {
                    Some(reason) => item.opacity(0.5).tooltip(move |window, cx| {
                        super::tooltip::text(format!("{} — {reason}", device.label()), window, cx)
                    }),
                    None => item.cursor_pointer().on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.select_device(device, cx),
                    )),
                }
            }))
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.saved.enabled;
        h_flex()
            .flex_none()
            .gap_3()
            .px_4()
            .py_3()
            .items_center()
            .border_t_1()
            .border_color(tokens::border())
            .child(kit::toggle(
                "controller-prepare-every-play",
                enabled,
                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_enabled(cx)),
            ))
            .child(
                div()
                    .flex_1()
                    .child("Prepare every Play for gamepads, window open or not"),
            )
            .when(self.device().unavailable().is_none(), |this| {
                this.child(
                    ui::button(
                        "controller-edit-mappings",
                        "Edit mappings",
                        ui::Weight::Secondary,
                        true,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.editing = Some(Editing::new(this.bindings.clone()));
                        cx.notify();
                    })),
                )
            })
    }
}

impl Render for ControllerEmulator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (status, status_color) = self.status(cx);
        let device = self.device();
        let output = self.output();
        let body = match &self.editing {
            Some(editing) => editor::editor(device, editing, &self.geometry, cx).into_any_element(),
            None if device.unavailable().is_some() => div().flex_1().into_any_element(),
            None => drawing::controller(device, &output, None, None, &self.geometry, cx)
                .into_any_element(),
        };
        v_flex()
            .id("controller-emulator")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_size(px(13.))
            .text_color(tokens::text())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.key_down(&event.keystroke.key, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_up(
                cx.listener(|this, event: &KeyUpEvent, _, cx| {
                    this.key_up(&event.keystroke.key, cx)
                }),
            )
            // Drags are followed window-wide, so a stick keeps moving when
            // the pointer leaves its circle.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                drawing::drag_move(this, event.position, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| drawing::drag_end(this, cx)),
            )
            .child(super::chrome::window_topbar(
                "Controller Emulator".into(),
                false,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                v_flex()
                    .flex_none()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(tokens::border())
                    .child(self.device_picker(cx))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(status_color)
                            .child(status),
                    ),
            )
            .child(v_flex().flex_1().min_h_0().child(body))
            .child(self.footer(cx))
    }
}
