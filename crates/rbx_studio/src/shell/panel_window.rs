//! A dock torn out of the shell, in a window of its own.
//!
//! The window holds no state and owns no copy of anything: it renders the
//! same `Shell` the main window does, through the same `panel_parts` the
//! docked path uses, so a floating Explorer is the Explorer rather than a
//! second one that has to be kept in step. That is the rule the sequence
//! graph window already follows (`crate::sequence_window`), for the same
//! reason, two copies of a panel is two panels disagreeing.
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

use super::layout::{Edge, Landing, Panel};
use super::Shell;

/// How far in from one of the editor's left, right or bottom edges the top
/// of a torn-out dock's window has to come for it to dock on that edge.
const DROP_BAND: f32 = 48.;

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
    _moved: Subscription,
}

impl PanelWindow {
    fn open(shell: Entity<Shell>, panel: Panel, cx: &mut App) -> Option<WindowHandle<Root>> {
        let window_size = size(
            tokens::scaled_width(WINDOW_WIDTH),
            tokens::scaled_width(WINDOW_HEIGHT),
        );
        let options = WindowOptions {
            app_id: Some(crate::APP_ID.to_owned()),
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
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| PanelWindow {
                shell,
                panel,
                grab: Rc::new(Cell::new(false)),
                _moved: cx.observe_window_bounds(window, |this: &mut PanelWindow, window, cx| {
                    this.dock_if_dropped(window, cx)
                }),
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }
}

impl PanelWindow {
    /// Drag back: GPUI's drag and drop is per window, so a tab can't be
    /// dragged from here into the editor. The window itself can be moved,
    /// though, and once the top of it comes within [`DROP_BAND`] of the
    /// editor's left, right or bottom edge the dock lands on that edge.
    /// The middle of the editor is no drop zone, so a dock can still float
    /// over the viewport.
    fn dock_if_dropped(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let moved = window.bounds();
        let top = point(moved.center().x, moved.top() + px(12.));
        let Some(main) = cx.windows().first().copied() else {
            return;
        };
        let Ok(editor) = main.update(cx, |_, window, _| window.bounds()) else {
            return;
        };
        let Some(edge) = drop_edge(editor, top) else {
            return;
        };
        let panel = self.panel;
        self.shell.update(cx, |shell, cx| {
            shell.land_panel(panel, Landing::NewGroup { edge, group: 0 }, cx);
        });
    }
}

/// The editor edge `at` is within [`DROP_BAND`] of, inside the editor, the
/// nearest one if it is near two.
fn drop_edge(editor: Bounds<Pixels>, at: Point<Pixels>) -> Option<Edge> {
    if !editor.contains(&at) {
        return None;
    }
    [
        (Edge::Left, at.x - editor.left()),
        (Edge::Right, editor.right() - at.x),
        (Edge::Bottom, editor.bottom() - at.y),
    ]
    .into_iter()
    .filter(|(_, gap)| *gap < px(DROP_BAND))
    .min_by(|a, b| f32::from(a.1).total_cmp(&f32::from(b.1)))
    .map(|(edge, _)| edge)
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
                        shell.set_panel_open(panel, true, cx);
                    });
                },
            ))
            // The same strip a docked panel wears, minus the tabs: there
            // is only one panel in here, so its name is the window's
            // title and what is left is its own controls.
            .child(super::chrome::dock_strip(
                Vec::new(),
                trailing,
                panel.has_toolbar(),
            ))
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
    /// opens its windows too, a floating panel that only appeared once
    /// you floated it *again* would be a panel the editor lost on restart.
    ///
    /// **Deferred**, and that is not a detail. Opening a window renders it
    /// immediately, and this window's content is the shell itself
    /// ([`PanelWindow::render`] asks it for the panel's parts), so
    /// opening one straight from `Shell::render` re-enters `Shell::update`
    /// and GPUI panics with "cannot update while it is already being
    /// updated". Running it after the frame costs nothing and is the whole
    /// fix.
    /// Brings every torn-out dock up with the window it belongs to.
    ///
    /// `WindowKind::Floating` asks the platform to keep these above the
    /// editor, and on X11 a window manager is free to ignore it, which
    /// leaves a dock you tore out sitting behind the window you tore it
    /// out of. Raising them on the main window's own rising edge is the
    /// part this app can do for itself. Only on the *edge*: raising them
    /// every frame would fight the compositor, and re-activating the main
    /// window afterwards is what keeps the keyboard where the click put
    /// it.
    pub(super) fn raise_panel_windows(&mut self, window: &Window, cx: &mut Context<Self>) {
        let active = window.is_window_active();
        if active == self.window_was_active {
            return;
        }
        self.window_was_active = active;
        if !active || self.panel_windows.is_empty() {
            return;
        }

        let children: Vec<_> = self.panel_windows.values().cloned().collect();
        let handle = cx.entity();
        cx.defer(move |cx| {
            for child in children {
                let _ = child.update(cx, |_, window, _| window.activate_window());
            }
            // Last, so the window the user actually clicked keeps focus.
            handle.update(cx, |_, cx| {
                cx.defer(|cx| {
                    if let Some(main) = cx.windows().first() {
                        let _ = main.update(cx, |_, window, _| window.activate_window());
                    }
                });
            });
        });
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn a_window_brought_to_an_edge_docks_there_and_one_in_the_middle_floats() {
        let editor = Bounds::new(point(px(0.), px(0.)), size(px(1600.), px(1000.)));
        assert_eq!(
            drop_edge(editor, point(px(20.), px(400.))),
            Some(Edge::Left)
        );
        assert_eq!(
            drop_edge(editor, point(px(1590.), px(400.))),
            Some(Edge::Right)
        );
        assert_eq!(
            drop_edge(editor, point(px(800.), px(980.))),
            Some(Edge::Bottom)
        );
        assert_eq!(drop_edge(editor, point(px(800.), px(400.))), None);
        assert_eq!(drop_edge(editor, point(px(1700.), px(400.))), None);
    }
}
