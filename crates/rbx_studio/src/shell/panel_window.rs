//! A dock torn out of the shell, in a window of its own.
//!
//! The window holds no state and owns no copy of anything: it renders the
//! same `Shell` the main window does, through the same `panel_parts` the
//! docked path uses, so a floating Explorer is the Explorer rather than a
//! second one that has to be kept in step. That is the rule the sequence
//! graph window already follows (`crate::sequence_window`), for the same
//! reason — two copies of a panel is two panels disagreeing.
//!
//! Which panels are torn out is [`super::layout::Layout`]'s to say. This
//! module only keeps the open windows matching it: [`Shell::sync_panel_windows`]
//! opens one for a panel that has just been floated and closes the one
//! belonging to a panel that has just been docked again.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::{v_flex, Root};
use gpui_kit::*;

use crate::tokens;

use super::layout::Panel;
use super::Shell;

/// What a torn-out dock starts at. Wide enough for the Explorer's deeper
/// rows and the Properties name column at the scale the shell is using.
const WINDOW_WIDTH: f32 = 340.;
const WINDOW_HEIGHT: f32 = 480.;

/// One torn-out dock's window.
pub(crate) struct PanelWindow {
    shell: Entity<Shell>,
    panel: Panel,
    /// Whether the title bar may move the window, which
    /// `shell::chrome::panel_topbar` owns and this only has to hold.
    grab: Rc<Cell<bool>>,
}

impl PanelWindow {
    fn open(shell: Entity<Shell>, panel: Panel, cx: &mut App) -> Option<WindowHandle<Root>> {
        let window_size = size(
            tokens::scaled_width(WINDOW_WIDTH),
            tokens::scaled_width(WINDOW_HEIGHT),
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                window_size,
                cx,
            ))),
            // Floats above the editor it belongs to, the way a torn-out
            // panel does everywhere: one the compositor can lose behind
            // the main window is one you tore out and then could not find.
            kind: WindowKind::Floating,
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(panel.key())),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            app_owns_titlebar_drag: true,
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            let view = cx.new(|_| PanelWindow {
                shell,
                panel,
                grab: Rc::new(Cell::new(false)),
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }
}

impl Render for PanelWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.panel;
        let shell = self.shell.clone();

        // Rendered by asking the shell for the same parts the docked path
        // asks for. `update` rather than `read`: building the Properties
        // rows creates this frame's widgets, which is a mutation however
        // read-only the result looks.
        let (trailing, content) =
            shell.update(cx, |shell, cx| shell.floating_parts(panel, window, cx));

        // Closing a torn-out dock puts it back where it started rather
        // than making it disappear: a panel you cannot get back is a panel
        // the editor has lost, and there is no "reopen" anywhere.
        let closing = self.shell.clone();
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .text_color(tokens::text_strong())
            .child(super::chrome::panel_topbar(
                SharedString::from(panel.key()),
                self.grab.clone(),
                move |_, _, cx| {
                    closing.update(cx, |shell, cx| {
                        shell.dock_panel(panel, panel.home(), None, cx);
                    });
                },
            ))
            // The same strip a docked panel wears, minus the tabs: there
            // is only one panel in here, so its name is the window's
            // title and what is left is its own controls.
            .child(super::chrome::dock_strip(Vec::new(), trailing))
            .children(content)
    }
}

impl Shell {
    /// Opens a window for every panel that has just been torn out, and
    /// closes the one belonging to every panel that has just been put
    /// back.
    ///
    /// Driven from the shell's own render rather than from the transforms
    /// that move a panel, so that a layout restored from the settings file
    /// opens its windows too — a floating panel that only appeared once
    /// you floated it *again* would be a panel the editor lost on restart.
    ///
    /// **Deferred**, and that is not a detail. Opening a window renders it
    /// immediately, and this window's content is the shell itself
    /// ([`PanelWindow::render`] asks it for the panel's parts) — so
    /// opening one straight from `Shell::render` re-enters `Shell::update`
    /// and GPUI panics with "cannot update while it is already being
    /// updated". Running it after the frame costs nothing and is the whole
    /// fix.
    pub(super) fn sync_panel_windows(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<Panel> = self.layout.floating().to_vec();
        let open: Vec<Panel> = self.panel_windows.keys().copied().collect();
        if wanted.len() == open.len() && wanted.iter().all(|panel| open.contains(panel)) {
            return;
        }

        let handle = cx.entity();
        cx.defer(move |cx| {
            for panel in Panel::ALL {
                let (wanted, open) = handle.read_with(cx, |shell, _| {
                    (
                        shell.layout.floating().contains(&panel),
                        shell.panel_windows.contains_key(&panel),
                    )
                });
                if wanted == open {
                    continue;
                }

                if wanted {
                    if let Some(window) = PanelWindow::open(handle.clone(), panel, cx) {
                        handle.update(cx, |shell, _| {
                            shell.panel_windows.insert(panel, window);
                        });
                    }
                } else {
                    let window = handle.update(cx, |shell, _| shell.panel_windows.remove(&panel));
                    if let Some(window) = window {
                        // The window is gone either way; a close that
                        // fails is one the platform had already taken.
                        let _ = window.update(cx, |_, window, _| window.remove_window());
                    }
                }
            }
        });
    }
}
