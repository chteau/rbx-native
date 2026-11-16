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
use std::time::Duration;

use gpui_kit::component::{v_flex, Root};
use gpui_kit::*;

use crate::tokens;

use super::layout::{Edge, Landing, Panel};
use super::Shell;

#[cfg(target_os = "linux")]
mod x11;

/// How far in from the editor's left or right edge a torn-out dock's
/// window has to come with that same side, or from its bottom edge with its
/// top, for it to dock on that edge.
const DROP_BAND: f32 = 48.;

/// How long a window has to stay put before a drop is read, see
/// [`PanelWindow::dock_if_dropped`].
const SETTLE_MS: u64 = 150;

/// What a torn-out dock starts at. Wide enough for the Explorer's deeper
/// rows and the Properties name column at the scale the shell is using.
const WINDOW_WIDTH: f32 = 340.;
const WINDOW_HEIGHT: f32 = 480.;

/// What the toolkit's client-side frame takes off a window before its
/// content gets any (`gpui_component::window_border::SHADOW_SIZE`, 20px a
/// side on Linux, not public), for a plugin window sized to its content.
const WINDOW_FRAME: f32 = 40.;

/// One torn-out dock's window.
pub(crate) struct PanelWindow {
    shell: Entity<Shell>,
    panel: Panel,
    /// Whether the title bar may move the window, which
    /// `shell::chrome::panel_topbar` owns and this only has to hold.
    grab: Rc<Cell<bool>>,
    /// The title the platform window was last given.
    title: SharedString,
    /// Keeping where a plugin's window came to rest, see
    /// [`PanelWindow::remember_bounds`].
    remember: Option<Task<()>>,
    /// The size the window last had: a change is a resize, a snap or a
    /// maximize, never a drag, and none of those docks it.
    size: Size<Pixels>,
    /// Whether the size changed since the window last came to rest.
    resized: bool,
    /// The dock check, run once the window has stopped moving, see
    /// [`PanelWindow::dock_if_dropped`].
    settle: Option<Task<()>>,
    _moved: Subscription,
}

impl PanelWindow {
    fn open(shell: Entity<Shell>, panel: Panel, cx: &mut App) -> Option<WindowHandle<Root>> {
        let stowed = shell.update(cx, |shell, _| shell.stowed_panel_windows.remove(&panel));
        let (bounds, min, saved) = match panel {
            Panel::Plugin(dock) => {
                let bars = tokens::topbar_height() + tokens::dock_tabs_height();
                let chrome = size(px(WINDOW_FRAME), px(WINDOW_FRAME) + bars);
                let (saved, fresh, min) = shell.read(cx).plugin_window_bounds(dock, chrome);
                let bounds = saved.unwrap_or_else(|| Bounds::centered(None, fresh, cx));
                (bounds, Some(min), saved)
            }
            _ => {
                let window_size = size(
                    tokens::scaled_width(WINDOW_WIDTH),
                    tokens::scaled_width(WINDOW_HEIGHT),
                );
                (Bounds::centered(None, window_size, cx), None, None)
            }
        };
        let (mut bounds, mut saved) = match stowed {
            Some(stowed) => (stowed, Some(stowed)),
            None => (bounds, saved),
        };
        // A spot in one of the editor's drop bands (saved before the editor
        // was moved, say) would dock the window again the moment it opens.
        if editor_bounds(cx).is_some_and(|editor| drop_edge(editor, bounds).is_some()) {
            bounds = Bounds::centered(None, bounds.size, cx);
            saved = None;
        }
        let title = SharedString::from(panel.title());
        let options = WindowOptions {
            app_id: Some(crate::APP_ID.to_owned()),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: min,
            // Floats above the editor it belongs to, the way a torn-out
            // panel does everywhere: one the compositor can lose behind
            // the main window is one you tore out and then could not find.
            kind: WindowKind::Floating,
            titlebar: Some(TitlebarOptions {
                title: Some(title.clone()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            app_owns_titlebar_drag: true,
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };

        let window = cx
            .open_window(options, move |window, cx| {
                let closing = shell.clone();
                window.on_window_should_close(cx, move |_, cx| {
                    close(&closing, panel, cx);
                    false
                });
                let view = cx.new(|cx| PanelWindow {
                    shell,
                    panel,
                    grab: Rc::new(Cell::new(false)),
                    title,
                    remember: None,
                    size: bounds.size,
                    resized: false,
                    settle: None,
                    _moved: cx.observe_window_bounds(
                        window,
                        |this: &mut PanelWindow, window, cx| {
                            this.remember_bounds(window, cx);
                            this.dock_if_dropped(window, cx)
                        },
                    ),
                });
                cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
            })
            .ok()?;
        #[cfg(target_os = "linux")]
        x11::settle(window, saved.map(|saved| saved.origin), cx);
        #[cfg(not(target_os = "linux"))]
        let _ = saved;
        Some(window)
    }
}

/// The window's own close, its × or the platform's: a native dock goes
/// back where it started rather than disappearing, since a panel you cannot
/// get back is a panel the editor has lost. A plugin's window is its
/// widget, so closing it is the widget's `Enabled` going false, and its
/// toolbar button opens it again where it was left.
fn close(shell: &Entity<Shell>, panel: Panel, cx: &mut App) {
    shell.update(cx, |shell, cx| match panel {
        Panel::Plugin(_) => shell.close_panel(panel, cx),
        _ => shell.set_panel_open(panel, true, cx),
    });
}

impl PanelWindow {
    /// Keeps where a plugin's window was left. Every step of a move lands
    /// here and each keep rewrites the plugin's settings file, so only the
    /// place it comes to rest is written.
    fn remember_bounds(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Panel::Plugin(dock) = self.panel else {
            return;
        };
        let bounds = window.bounds();
        let shell = self.shell.clone();
        self.remember = Some(cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            shell.update(cx, |shell, _| shell.remember_plugin_window(dock, bounds));
        }));
    }

    /// Drag back: GPUI's drag and drop is per window, so a tab can't be
    /// dragged from here into the editor. The window itself can be moved,
    /// though, and once its left or right side comes within [`DROP_BAND`]
    /// of the editor's own, or its top within as much of the editor's
    /// bottom, the dock lands on that edge. The sides rather than the
    /// middle of the window: an editor filling the screen leaves no room
    /// to bring a window's middle to its edge. The middle of the editor is
    /// no drop zone, so a dock can still float over the viewport. The window
    /// has to stay there a moment ([`SETTLE_MS`]) without changing size.
    fn dock_if_dropped(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = window.bounds().size;
        self.resized |= std::mem::replace(&mut self.size, size) != size;
        // Windows reports a snap as a move and then a resize, so the check
        // waits for the window to stop: a snap's first half would otherwise
        // read as a window brought to the editor's edge.
        self.settle = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SETTLE_MS))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if std::mem::take(&mut this.resized) {
                    return;
                }
                let Some(edge) = editor_bounds(cx).and_then(|e| drop_edge(e, window.bounds()))
                else {
                    return;
                };
                let panel = this.panel;
                this.shell.update(cx, |shell, cx| {
                    shell.land_panel(panel, Landing::NewGroup { edge, group: 0 }, cx);
                });
            });
        }));
    }
}

/// Where the editor's own window is.
fn editor_bounds(cx: &mut App) -> Option<Bounds<Pixels>> {
    let main = cx.windows().first().copied()?;
    main.update(cx, |_, window, _| window.bounds()).ok()
}

/// The editor edge a window at `moved` has been brought to (see
/// [`PanelWindow::dock_if_dropped`]), the nearest one if it is at two. Only
/// while the top of the window is over the editor.
fn drop_edge(editor: Bounds<Pixels>, moved: Bounds<Pixels>) -> Option<Edge> {
    let top = moved.top() + px(12.);
    if !editor.contains(&point(moved.center().x, top)) {
        return None;
    }
    [
        (Edge::Left, moved.left() - editor.left()),
        (Edge::Right, editor.right() - moved.right()),
        (Edge::Bottom, editor.bottom() - top),
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

        // A plugin can retitle its widget while it is open.
        let title = SharedString::from(panel.title());
        if title != self.title {
            window.set_window_title(&title);
            self.title = title.clone();
        }

        let closing = self.shell.clone();
        v_flex()
            .size_full()
            .bg(tokens::dock())
            .text_color(tokens::text_strong())
            .child(super::chrome::panel_topbar(
                title,
                self.grab.clone(),
                move |_, _, cx| close(&closing, panel, cx),
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

    /// The floating panels that get a window. A plugin dock whose widget
    /// is shut keeps its place in the layout but not its window, which
    /// comes back where it was when the widget opens again.
    ///
    /// None while one of the editor's dialogs is up: the dialog is drawn
    /// inside the editor's window, and every platform keeps a floating
    /// window above the one it belongs to, so a torn-out dock would sit
    /// on top of the question it has to wait for. They are put away until
    /// it is answered and come back where they were.
    fn windowed_panels(&self) -> Vec<Panel> {
        if self.dialog_up {
            return Vec::new();
        }
        let shut = self.shut_plugin_docks();
        let floating = self.layout.floating().iter().copied();
        floating.filter(|panel| !shut.contains(panel)).collect()
    }

    pub(super) fn sync_panel_windows(&mut self, cx: &mut Context<Self>) {
        let wanted = self.windowed_panels();
        let open: Vec<Panel> = self.panel_windows.keys().copied().collect();
        if wanted.len() == open.len() && wanted.iter().all(|panel| open.contains(panel)) {
            return;
        }

        let handle = cx.entity();
        cx.defer(move |cx| {
            let (wanted, open) = handle.read_with(cx, |shell, _| {
                let open: Vec<Panel> = shell.panel_windows.keys().copied().collect();
                (shell.windowed_panels(), open)
            });
            for &panel in wanted.iter().filter(|panel| !open.contains(panel)) {
                if let Some(window) = PanelWindow::open(handle.clone(), panel, cx) {
                    handle.update(cx, |shell, _| {
                        shell.panel_windows.insert(panel, window);
                    });
                }
            }
            for panel in open.into_iter().filter(|panel| !wanted.contains(panel)) {
                let window = handle.update(cx, |shell, _| shell.panel_windows.remove(&panel));
                let Some(window) = window else {
                    continue;
                };
                // The window is gone either way; a close that fails is
                // one the platform had already taken.
                let Ok(bounds) = window.update(cx, |_, window, _| {
                    let bounds = window.bounds();
                    window.remove_window();
                    bounds
                }) else {
                    continue;
                };
                handle.update(cx, |shell, _| {
                    if shell.dialog_up {
                        shell.stowed_panel_windows.insert(panel, bounds);
                    }
                });
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
        let at = |x: f32, y: f32| Bounds::new(point(px(x), px(y)), size(px(340.), px(480.)));
        assert_eq!(drop_edge(editor, at(20., 300.)), Some(Edge::Left));
        // Pushed past the screen's edge, as with an editor filling it.
        assert_eq!(drop_edge(editor, at(-120., 300.)), Some(Edge::Left));
        assert_eq!(drop_edge(editor, at(1240., 300.)), Some(Edge::Right));
        assert_eq!(drop_edge(editor, at(600., 960.)), Some(Edge::Bottom));
        assert_eq!(drop_edge(editor, at(600., 300.)), None);
        assert_eq!(drop_edge(editor, at(1500., 300.)), None);
    }
}
