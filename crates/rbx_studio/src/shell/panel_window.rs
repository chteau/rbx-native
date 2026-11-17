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
    /// Where in the window the title bar was last pressed: the pointer
    /// stays at that spot while the window is moved by it.
    grabbed_at: Option<Point<Pixels>>,
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
        let (stowed, torn_at, editor) = shell.update(cx, |shell, _| {
            let torn_at = shell.torn_at.take_if(|(torn, _)| *torn == panel);
            (
                shell.stowed_panel_windows.remove(&panel),
                torn_at.map(|(_, at)| at),
                shell.editor_window,
            )
        });
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
        // A dock torn out opens under the pointer that let go of it, held
        // by its title bar as if it had been dragged there. A saved spot in
        // one of the editor's drop bands is kept as it is: only a window
        // moved by its title bar docks (see `dock_if_dropped`).
        let bounds = match (stowed, torn_at) {
            (Some(stowed), _) => stowed,
            (None, Some(at)) => Bounds::new(at - tear_out_grip(), bounds.size),
            (None, None) => bounds,
        };
        let bounds = keep_on_screen(bounds, work_area(torn_at.unwrap_or(bounds.center()), cx));
        let placed =
            (stowed.is_some() || torn_at.is_some() || saved.is_some()).then_some(bounds.origin);
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
                window.on_window_should_close(cx, move |_, cx| close(&closing, panel, cx));
                let view = cx.new(|cx| PanelWindow {
                    shell,
                    panel,
                    grab: Rc::new(Cell::new(false)),
                    grabbed_at: None,
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
        x11::settle(window, editor, placed, cx);
        #[cfg(not(target_os = "linux"))]
        let _ = (editor, placed);
        Some(window)
    }
}

/// The window's own close, its × or the platform's: a native dock goes
/// back where it started rather than disappearing, since a panel you cannot
/// get back is a panel the editor has lost. A plugin's window is its
/// widget, so closing it is the widget's `Enabled` going false, and its
/// toolbar button opens it again where it was left. One outliving the
/// editor's window is left over from a quit: it just goes, and its widget
/// stays open for the next session, as in Studio.
fn close(shell: &Entity<Shell>, panel: Panel, cx: &mut App) -> bool {
    shell.update(cx, |shell, cx| {
        let editor = shell.editor_window;
        if !editor.is_some_and(|editor| cx.windows().contains(&editor)) {
            return true;
        }
        match panel {
            Panel::Plugin(_) => shell.close_panel(panel, cx),
            _ => shell.set_panel_open(panel, true, cx),
        }
        false
    })
}

impl PanelWindow {
    /// Keeps where a plugin's window was left. Every step of a move lands
    /// here and each keep rewrites the plugin's settings file, so only the
    /// place it comes to rest is written.
    fn remember_bounds(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Panel::Plugin(dock) = self.panel else {
            return;
        };
        // The restored rect, not a maximized or snapped one: kept, that
        // opened the next tear-out filling the screen.
        let bounds = window.window_bounds().get_bounds();
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
    /// bottom, with the pointer still over the editor, the dock lands on
    /// that edge. The sides rather than the middle of the window: an editor
    /// filling the screen leaves no room to bring a window's middle to its
    /// edge. The middle of the editor is no drop zone, so a dock can still
    /// float over the viewport. The window has to stay there a moment
    /// ([`SETTLE_MS`]) without changing size.
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
                // Never pressed on its title bar, so never moved by it: a
                // window opening at a saved spot, or placed by the window
                // manager, stays a window.
                let Some(grabbed_at) = this.grabbed_at else {
                    return;
                };
                let moved = window.bounds();
                let editor = this.shell.read(cx).editor_window;
                let Some(edge) = editor_bounds(editor, cx)
                    .and_then(|e| drop_edge(e, moved, moved.origin + grabbed_at))
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
fn editor_bounds(editor: Option<AnyWindowHandle>, cx: &mut App) -> Option<Bounds<Pixels>> {
    editor?.update(cx, |_, window, _| window.bounds()).ok()
}

/// Where in a torn-out window the pointer that tore it out ends up: on
/// its title bar, a little in from the left, below the frame the toolkit
/// draws around the window on Linux.
fn tear_out_grip() -> Point<Pixels> {
    let frame = if cfg!(target_os = "linux") {
        px(WINDOW_FRAME / 2.)
    } else {
        px(0.)
    };
    point(frame + px(60.), frame + tokens::topbar_height() / 2.)
}

/// The work area (the screen less its taskbar) of the display under `at`.
fn work_area(at: Point<Pixels>, cx: &App) -> Option<Bounds<Pixels>> {
    cx.displays()
        .into_iter()
        .find(|display| display.bounds().contains(&at))
        .or_else(|| cx.primary_display())
        .map(|display| display.visible_bounds())
}

/// `bounds` shrunk to fit `area` and slid inside it. A tear-out held near
/// the screen's right edge keeps its bar under the pointer; near the
/// bottom it moves up.
fn keep_on_screen(bounds: Bounds<Pixels>, area: Option<Bounds<Pixels>>) -> Bounds<Pixels> {
    let Some(area) = area else {
        return bounds;
    };
    let size = bounds.size.min(&area.size);
    let x = bounds
        .origin
        .x
        .min(area.right() - size.width)
        .max(area.left());
    let y = bounds
        .origin
        .y
        .min(area.bottom() - size.height)
        .max(area.top());
    Bounds::new(point(x, y), size)
}

/// The editor edge a window at `moved` has been brought to (see
/// [`PanelWindow::dock_if_dropped`]), the nearest one if it is at two. Only
/// while the pointer moving it by its title bar is over the editor.
///
/// The pointer, not the middle of the title bar: the bar is grabbed near
/// its title, on the left, so a window taken to the right edge by it hangs
/// mostly past that edge, middle included, and never docked there.
fn drop_edge(
    editor: Bounds<Pixels>,
    moved: Bounds<Pixels>,
    pointer: Point<Pixels>,
) -> Option<Edge> {
    if !editor.contains(&pointer) {
        return None;
    }
    let top = moved.top() + px(12.);
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
            .child(
                div()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, _| {
                            this.grabbed_at = Some(event.position);
                        }),
                    )
                    .child(super::chrome::panel_topbar(
                        title,
                        self.grab.clone(),
                        move |_, window, cx| {
                            if close(&closing, panel, cx) {
                                window.remove_window();
                            }
                        },
                    )),
            )
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
        let main = window.window_handle();
        let handle = cx.entity();
        cx.defer(move |cx| {
            for child in children {
                let _ = child.update(cx, |_, window, _| window.activate_window());
            }
            // Last, so the window the user actually clicked keeps focus.
            handle.update(cx, |_, cx| {
                cx.defer(move |cx| {
                    let _ = main.update(cx, |_, window, _| window.activate_window());
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

    /// New windows only open while the editor's own window is `active`:
    /// Windows makes a floating window owned by the active one, and one
    /// opened while Home was still up at launch went with Home.
    pub(super) fn sync_panel_windows(&mut self, active: bool, cx: &mut Context<Self>) {
        // A window the platform took away under us (its owner closing, say)
        // has to open again rather than count as open.
        let alive = cx.windows();
        self.panel_windows
            .retain(|_, window| alive.contains(&AnyWindowHandle::from(*window)));
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
            let opening = wanted
                .iter()
                .filter(|panel| active && !open.contains(panel));
            for &panel in opening {
                if let Some(window) = PanelWindow::open(handle.clone(), panel, cx) {
                    handle.update(cx, |shell, _| {
                        shell.panel_windows.insert(panel, window);
                    });
                }
            }
            if active {
                // A tear-out whose window did not open is not one for a
                // later Float to open at.
                handle.update(cx, |shell, _| shell.torn_at = None);
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

    /// The Windows retest: torn out at (1650,600) on a 1920x1080 screen,
    /// the window ran past both edges.
    #[::core::prelude::v1::test]
    fn a_tear_out_stays_on_its_screen() {
        let area = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1040.)));
        let torn = Bounds::new(point(px(1566.), px(579.)), size(px(499.), px(502.)));
        let kept = keep_on_screen(torn, Some(area));
        assert_eq!(kept.origin, point(px(1421.), px(538.)));
        assert_eq!(kept.size, torn.size);
        // Wider than the screen: shrunk to it.
        let huge = Bounds::new(point(px(-50.), px(20.)), size(px(2400.), px(400.)));
        let kept = keep_on_screen(huge, Some(area));
        assert_eq!(
            kept,
            Bounds::new(point(px(0.), px(20.)), size(px(1920.), px(400.)))
        );
        // Already on screen: untouched.
        let fine = Bounds::new(point(px(100.), px(100.)), size(px(300.), px(300.)));
        assert_eq!(keep_on_screen(fine, Some(area)), fine);
    }

    #[::core::prelude::v1::test]
    fn a_window_brought_to_an_edge_docks_there_and_one_in_the_middle_floats() {
        let editor = Bounds::new(point(px(0.), px(0.)), size(px(1600.), px(1000.)));
        // Grabbed in the middle of the title bar.
        let edge = |x: f32, y: f32| {
            let moved = Bounds::new(point(px(x), px(y)), size(px(340.), px(480.)));
            drop_edge(editor, moved, moved.origin + point(px(170.), px(12.)))
        };
        assert_eq!(edge(20., 300.), Some(Edge::Left));
        // Pushed past the screen's edge, as with an editor filling it.
        assert_eq!(edge(-120., 300.), Some(Edge::Left));
        assert_eq!(edge(1240., 300.), Some(Edge::Right));
        assert_eq!(edge(600., 960.), Some(Edge::Bottom));
        assert_eq!(edge(600., 300.), None);
        assert_eq!(edge(1500., 300.), None);
    }

    /// The Windows retest: the bar grabbed by its title, near its left end,
    /// and the pointer taken to the editor's right edge leaves most of the
    /// window past that edge. It docks right, as the same move docks left.
    #[::core::prelude::v1::test]
    fn a_window_held_by_its_title_docks_at_the_right_edge_too() {
        let editor = Bounds::new(point(px(28.), px(80.)), size(px(1400.), px(860.)));
        let grab = point(px(60.), px(14.));
        let held_at = |x: f32| {
            let moved = Bounds::new(point(px(x) - grab.x, px(300.)), size(px(499.), px(502.)));
            drop_edge(editor, moved, moved.origin + grab)
        };
        assert_eq!(held_at(1415.), Some(Edge::Right));
        assert_eq!(held_at(40.), Some(Edge::Left));
        // Let go past the editor: the window stays where it was put.
        assert_eq!(held_at(1440.), None);
    }
}
