//! Server & Clients in one window: a tab per peer, the Server first, then
//! Player1..N, where Studio opens a window each. Every tab is a whole
//! editor (`Shell`) on its peer with its own camera, Explorer, Properties
//! and Output, kept while another tab shows, so a switch is instant; only
//! the shown tab is drawn (a viewport that isn't painted stops drawing,
//! see `workspace_view::presence`) and gets the keyboard and mouse. The
//! session, and every peer in it, runs on whatever tab shows.
//!
//! Ctrl+Tab and Ctrl+Shift+Tab cycle the tabs, Ctrl+1..9 jump to one.
//! Closing a player's tab is that player leaving; closing the window ends
//! the session.
//!
//! A tab dragged off the row (or right-click › Open in New Window) pops its
//! peer out into a window of its own: the same editor, moved, so its
//! camera, docks and Output come along and the session never restarts.
//! Dragging that window's tab back onto the row, or closing the window,
//! docks it again. GPUI's drag and drop stops at its window, so both ends
//! are read from where on screen the pointer was let go.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, Icon, Root};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::windows::Home;
use crate::shell::{menu, panel_window, Shell};
use crate::tokens;

/// Which tabs there are, which one shows, and which peers are popped out
/// into windows of their own. The Server's tab is always the first. `S` is
/// each peer's editor; it moves between the row and a window as it is,
/// never rebuilt.
#[derive(Debug, Clone)]
pub(crate) struct Tabs<S> {
    docked: Vec<(Home, S)>,
    active: usize,
    /// Popped out, with the place each had in the row.
    apart: Vec<(Home, S, usize)>,
    /// The popped-out peer whose window has the keyboard.
    focused: Option<Home>,
}

impl<S> Default for Tabs<S> {
    fn default() -> Self {
        Tabs {
            docked: Vec::new(),
            active: 0,
            apart: Vec::new(),
            focused: None,
        }
    }
}

impl<S: Clone> Tabs<S> {
    pub(crate) fn homes(&self) -> Vec<Home> {
        self.docked.iter().map(|(home, _)| *home).collect()
    }

    pub(crate) fn active(&self) -> usize {
        self.active
    }

    /// The editor on tab `index`.
    pub(crate) fn get(&self, index: usize) -> Option<&S> {
        self.docked.get(index).map(|(_, peer)| peer)
    }

    pub(crate) fn shown(&self) -> Option<&S> {
        self.get(self.active)
    }

    pub(crate) fn is_apart(&self, home: Home) -> bool {
        self.apart.iter().any(|(h, ..)| *h == home)
    }

    /// A tab for `home`, at the end (the Server's at the front), not shown.
    pub(crate) fn add(&mut self, home: Home, peer: S) -> usize {
        if home == Home::Server {
            self.docked.insert(0, (home, peer));
            if self.docked.len() > 1 {
                self.active += 1;
            }
            return 0;
        }
        self.docked.push((home, peer));
        self.docked.len() - 1
    }

    /// Takes `home` out, tab or window, and says whether it was the shown
    /// tab. The shown one stays shown, or, when it was this one, the tab
    /// that slid into its place (or the new last) shows.
    pub(crate) fn remove(&mut self, home: Home) -> Option<(S, bool)> {
        if let Some(k) = self.apart.iter().position(|(h, ..)| *h == home) {
            self.focused = self.focused.filter(|h| *h != home);
            return Some((self.apart.remove(k).1, false));
        }
        let at = self.docked.iter().position(|(h, _)| *h == home)?;
        let (_, peer) = self.docked.remove(at);
        let shown = at == self.active;
        if at < self.active || self.active >= self.docked.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some((peer, shown))
    }

    /// Pops `home`'s tab out into a window: the row loses it as it does a
    /// closed one. Never the last tab, which would leave the row's window
    /// empty.
    pub(crate) fn detach(&mut self, home: Home) -> Option<(S, bool)> {
        if self.docked.len() < 2 {
            return None;
        }
        let at = self.docked.iter().position(|(h, _)| *h == home)?;
        let (peer, shown) = self.remove(home)?;
        self.apart.push((home, peer.clone(), at));
        Some((peer, shown))
    }

    /// Docks a popped-out peer back where its tab was, and shows it.
    pub(crate) fn attach(&mut self, home: Home) -> Option<S> {
        let k = self.apart.iter().position(|(h, ..)| *h == home)?;
        let (_, peer, at) = self.apart.remove(k);
        self.focused = self.focused.filter(|h| *h != home);
        // The Server's tab stays the first, whoever left meanwhile.
        let server_first = self.docked.first().is_some_and(|(h, _)| *h == Home::Server);
        let from = usize::from(home != Home::Server && server_first);
        let at = at.clamp(from, self.docked.len());
        self.docked.insert(at, (home, peer.clone()));
        self.active = at;
        Some(peer)
    }

    /// A popped-out peer's window got (`active`) or lost the keyboard.
    pub(crate) fn set_focused(&mut self, home: Home, active: bool) {
        if active && self.is_apart(home) {
            self.focused = Some(home);
        } else if self.focused == Some(home) {
            self.focused = None;
        }
    }

    /// Shows tab `index`; false when there is none or it already shows.
    pub(crate) fn activate(&mut self, index: usize) -> bool {
        if index >= self.docked.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// Ctrl+Tab (`forward`) and Ctrl+Shift+Tab, wrapping around.
    pub(crate) fn cycle(&mut self, forward: bool) -> bool {
        let count = self.docked.len();
        if count < 2 {
            return false;
        }
        let next = if forward {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        self.activate(next)
    }

    /// The player whose client gets the keyboard and mouse: the one whose
    /// own window has the keyboard, else the shown tab's; none on the
    /// Server's.
    pub(crate) fn input_target(&self) -> Option<Ref> {
        let home = match self.focused {
            Some(home) => home,
            None => self.docked.get(self.active)?.0,
        };
        match home {
            Home::Player(player) => Some(player),
            Home::Server => None,
        }
    }

    /// What a key does to the tabs: Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+1..9.
    pub(crate) fn key(&mut self, keystroke: &Keystroke) -> bool {
        let m = &keystroke.modifiers;
        if keystroke.key == "tab" && m.control && !m.alt {
            return self.cycle(!m.shift);
        }
        let digit = keystroke
            .key
            .parse::<usize>()
            .ok()
            .filter(|d| (1..=9).contains(d));
        match digit {
            Some(n) if m.secondary() && !m.shift && !m.alt => self.activate(n - 1),
            _ => false,
        }
    }
}

/// The test window's root: the tabs' editors, the shown one drawn.
pub(crate) struct TestWindow {
    tabs: Tabs<Entity<Shell>>,
    /// This window.
    handle: AnyWindowHandle,
    /// Each popped-out peer's window.
    windows: Vec<(Home, AnyWindowHandle)>,
    /// Where the tab row is in this window, as the shown tab last drew it.
    tab_row: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Collaborators asking to join: (their end, name).
    pub(super) offers: Vec<(u64, String)>,
    /// Players playing from their own editor.
    pub(super) afar: Vec<Ref>,
}

impl TestWindow {
    pub(crate) fn new(home: Home, shell: Entity<Shell>, handle: AnyWindowHandle) -> Self {
        let mut tabs = Tabs::default();
        tabs.add(home, shell);
        TestWindow {
            tabs,
            handle,
            windows: Vec::new(),
            tab_row: Rc::default(),
            offers: Vec::new(),
            afar: Vec::new(),
        }
    }

    pub(crate) fn tabs(&self) -> &Tabs<Entity<Shell>> {
        &self.tabs
    }

    /// Every window the session's peers are in, this one last.
    pub(super) fn windows(&self) -> Vec<AnyWindowHandle> {
        let apart = self.windows.iter().map(|(_, window)| *window);
        apart.chain([self.handle]).collect()
    }

    pub(crate) fn add(&mut self, home: Home, shell: Entity<Shell>, cx: &mut Context<Self>) {
        self.tabs.add(home, shell);
        cx.notify();
    }

    pub(crate) fn remove(&mut self, home: Home, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, shown)) = self.tabs.remove(home) else {
            return;
        };
        self.close_apart(home, cx);
        if shown {
            self.shown_changed(None, window, cx);
        }
        cx.notify();
    }

    /// Shows tab `index` (a press on it).
    pub(crate) fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.tabs.shown().cloned();
        if self.tabs.activate(index) {
            self.shown_changed(old, window, cx);
        }
    }

    /// Pops `home`'s tab out into a window of its own: held by its title
    /// bar under the pointer that dragged it out (`at`, on screen), or just
    /// off this one, and kept on its screen.
    pub(super) fn detach(
        &mut self,
        home: Home,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((shell, shown)) = self.tabs.detach(home) else {
            return;
        };
        if shown {
            self.shown_changed(Some(shell.clone()), window, cx);
        }
        shell.update(cx, |shell, _| {
            if let Some(link) = shell.play.live.local.peer.as_mut() {
                link.detached = true;
            }
        });
        let here = window.bounds();
        let origin = at.map_or(here.origin + point(px(48.), px(48.)), |at| {
            at - point(px(80.), tokens::topbar_height() / 2.)
        });
        let work_area = panel_window::screen_at(origin, cx).map(|(_, work_area)| work_area);
        let bounds = panel_window::keep_on_screen(
            Bounds::new(origin, here.size.map(|side| side * 0.75)),
            work_area,
        );
        let view = cx.entity().downgrade();
        // Opening a window draws it, and its editor reads this view.
        cx.defer(move |cx| open_apart(view, home, shell, bounds, cx));
        cx.notify();
    }

    /// Docks a popped-out peer back into its tab, shown, and closes its
    /// window.
    pub(super) fn attach(&mut self, home: Home, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.tabs.shown().cloned();
        let Some(shell) = self.tabs.attach(home) else {
            return;
        };
        self.close_apart(home, cx);
        let handle = self.handle;
        shell.update(cx, |shell, _| {
            shell.window_handle = handle;
            if let Some(link) = shell.play.live.local.peer.as_mut() {
                link.detached = false;
            }
        });
        self.shown_changed(old, window, cx);
    }

    /// A tab let go of at `at` (on screen) after a drag: one from the row
    /// let go off it pops out; a popped-out one let go over the row docks.
    fn dropped(
        &mut self,
        home: Home,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin = window.bounds().origin;
        let over_row = self
            .tab_row
            .get()
            .is_some_and(|row| Bounds::new(origin + row.origin, row.size).contains(&at));
        match (self.tabs.is_apart(home), over_row) {
            (true, true) => self.attach(home, window, cx),
            (false, false) => self.detach(home, Some(at), window, cx),
            _ => {}
        }
    }

    fn close_apart(&mut self, home: Home, cx: &mut Context<Self>) {
        let Some(k) = self.windows.iter().position(|(h, _)| *h == home) else {
            return;
        };
        let (_, apart) = self.windows.remove(k);
        // Not from inside that window's own close, or a click in it.
        cx.defer(move |cx| {
            let _ = apart.update(cx, |_, window, _| window.remove_window());
        });
    }

    fn key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let old = self.tabs.shown().cloned();
        if !self.tabs.key(keystroke) {
            return false;
        }
        self.shown_changed(old, window, cx);
        true
    }

    /// The tab that was shown lets go of every key and button its player
    /// held (no key stays down on a client nobody is looking at); the new
    /// one takes the focus.
    fn shown_changed(
        &mut self,
        old: Option<Entity<Shell>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.tabs.shown().cloned();
        for (shell, active) in old
            .into_iter()
            .map(|s| (s, false))
            .chain(shown.map(|s| (s, true)))
        {
            shell.update(cx, |shell, cx| {
                shell.local_run_tab_shown(active, window, cx)
            });
        }
        cx.notify();
    }
}

impl Render for TestWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .children(self.tabs.shown().cloned())
    }
}

/// Does `f` to the test window once whatever is being updated now is done
/// (a click in one of its editors, a popped-out window's close).
fn with_test_window(
    view: WeakEntity<TestWindow>,
    cx: &mut App,
    f: impl FnOnce(&mut TestWindow, &mut Window, &mut Context<TestWindow>) + 'static,
) {
    cx.defer(move |cx| {
        let Some(view) = view.upgrade() else {
            return;
        };
        let handle = view.read(cx).handle;
        let _ = handle.update(cx, |_, window, cx| {
            view.update(cx, |t, cx| f(t, window, cx))
        });
    });
}

/// Opens the window `home`'s editor was popped out into, at `bounds`.
fn open_apart(
    view: WeakEntity<TestWindow>,
    home: Home,
    shell: Entity<Shell>,
    bounds: Bounds<Pixels>,
    cx: &mut App,
) {
    let title = shell.read(cx).title.clone();
    let mut options = crate::window_options(&title, cx);
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    let opened = cx.open_window(options, |window, cx| {
        let closing = view.clone();
        window.on_window_should_close(cx, move |_, cx| {
            with_test_window(closing.clone(), cx, move |t, w, cx| t.attach(home, w, cx));
            false
        });
        let apart = cx.new(|cx| ApartWindow::new(view.clone(), home, shell.clone(), window, cx));
        cx.new(|cx| Root::new(crate::window_blur::wrap(apart, cx), window, cx))
    });
    let Ok(handle) = opened else {
        with_test_window(view, cx, move |t, w, cx| t.attach(home, w, cx));
        return;
    };
    let handle = AnyWindowHandle::from(handle);
    shell.update(cx, |shell, _| shell.window_handle = handle);
    let _ = view.update(cx, |t, _| t.windows.push((home, handle)));
    let _ = handle.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.local_run_tab_shown(true, window, cx))
    });
}

/// A popped-out peer's window: its editor, drawn whole.
struct ApartWindow {
    tabs: WeakEntity<TestWindow>,
    home: Home,
    shell: Entity<Shell>,
    _subscriptions: [Subscription; 2],
}

impl ApartWindow {
    fn new(
        tabs: WeakEntity<TestWindow>,
        home: Home,
        shell: Entity<Shell>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Its player gets the keyboard and mouse while the window has them,
        // and lets go of every key when it loses them.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            let (home, active) = (this.home, window.is_window_active());
            let _ = this
                .tabs
                .update(cx, |t, _| t.tabs.set_focused(home, active));
            // Docked back already, this window on its way out.
            let link = this.shell.read(cx).play.live.local.peer.as_ref();
            if !link.is_some_and(|link| link.detached) {
                return;
            }
            this.shell.update(cx, |shell, cx| {
                shell.local_run_tab_shown(active, window, cx)
            });
        });
        // What `Shell::new` set up for the window it was made in, the test
        // window, done for this one.
        let focus_lost = cx.on_focus_lost(window, |this, window, cx| {
            let target = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| this.shell.read(cx).root_focus.clone());
            window.focus(&target, cx);
        });
        ApartWindow {
            tabs,
            home,
            shell,
            _subscriptions: [activation, focus_lost],
        }
    }
}

impl Render for ApartWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.shell.clone())
    }
}

/// A peer's tab on its way off the row or back to it: its ghost.
#[derive(Clone)]
struct DraggedPeer {
    icon: IconName,
    label: SharedString,
}

impl Render for DraggedPeer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap(px(7.))
            .px(px(16.))
            .py(px(8.))
            .rounded(tokens::radius_tile())
            .bg(tokens::dock())
            .shadow(tokens::elevation())
            .opacity(0.9)
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .font_weight(tokens::WEIGHT_SEMIBOLD)
            .text_color(tokens::text_full())
            .child(Icon::new(self.icon).size(px(13.)))
            .child(self.label.clone())
    }
}

impl Shell {
    /// A test tab's row of tabs, where the editor has its documents: one
    /// per peer, styled as the document tabs, a player's with a close mark,
    /// and "+" for Add Client. A popped-out peer's window has its own tab
    /// alone.
    pub(in crate::shell) fn local_run_peer_tabs(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let link = self.play.live.local.peer.as_ref()?;
        let (afar, detached, menu_at) = (link.remote, link.detached, link.tab_menu);
        let view = link.tabs.upgrade()?;
        let window = view.read(cx);
        // Collaborators asking to join, and who plays from their editor, as
        // the host last said (reading the host here could catch it mid-update).
        let (offers, remote) = (window.offers.clone(), window.afar.clone());
        let row = window.tab_row.clone();
        let (active, tabs): (usize, Vec<(usize, Home, SharedString)>) = if detached {
            (0, vec![(0, link.home, self.title.clone())])
        } else {
            let tabs = window
                .tabs()
                .homes()
                .into_iter()
                .enumerate()
                .map(|(i, home)| {
                    // This editor is mid-render: its own title, not a read.
                    let title = match window.tabs().get(i) {
                        Some(s) if s.entity_id() == cx.entity_id() => self.title.clone(),
                        Some(s) => s.read(cx).title.clone(),
                        None => SharedString::default(),
                    };
                    (i, home, title)
                })
                .collect();
            (window.tabs().active(), tabs)
        };
        let count = tabs.len();
        let menu = menu_at.and_then(|(home, at)| {
            let label = tabs.iter().find(|(_, h, _)| *h == home)?.2.clone();
            Some(peer_tab_menu(
                &view,
                home,
                label,
                at,
                detached,
                count > 1,
                cx,
            ))
        });
        let tabs: Vec<_> = tabs
            .into_iter()
            .map(|(index, home, label)| {
                let from_afar = matches!(home, Home::Player(p) if remote.contains(&p));
                let tab = PeerTab {
                    index,
                    home,
                    label,
                    active: index == active,
                    count,
                    from_afar,
                    detached,
                };
                peer_tab(&view, tab, cx)
            })
            .collect();
        let offers: Vec<_> = offers
            .into_iter()
            .enumerate()
            .map(|(i, (from, name))| offer_chip(i, from, name, cx))
            .collect();
        Some(
            h_flex()
                .id("peer-tabs")
                .relative()
                .w_full()
                .h(tokens::tabs_height())
                .flex_none()
                .items_end()
                .gap(px(2.))
                .pt(px(8.))
                .px(px(12.))
                .when_some(tokens::on_ground(tokens::black()), |this, bg| this.bg(bg))
                .border_b_1()
                .border_color(tokens::border())
                .role(Role::TabList)
                .aria_label("Server and players")
                .children(tabs)
                .children(offers)
                // A collaborator's own window shows their player alone.
                .when(!afar && !detached, |this| {
                    this.child(
                        h_flex()
                            .id("peer-tab-add")
                            .flex_none()
                            .w(tokens::tab_add_width())
                            .h(tokens::tabs_height() - px(8.))
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(tokens::text3())
                            .hover(|this| this.text_color(tokens::text()))
                            .tooltip(|window, cx| {
                                crate::shell::tooltip::text(
                                    "Add a client to the current session.",
                                    window,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|shell, _, _, cx| shell.local_run_add_client(cx)))
                            .child(Icon::new(IconName::Plus).size(px(10.))),
                    )
                })
                // Where a popped-out tab docks when it is let go.
                .when(!detached, |this| {
                    this.child(
                        canvas(move |bounds, _, _| row.set(Some(bounds)), |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                    )
                })
                .children(menu)
                .into_any_element(),
        )
    }

    fn begin_peer_drag(&mut self, home: Home, cx: &mut Context<Self>) {
        if let Some(link) = self.play.live.local.peer.as_mut() {
            link.dragging = Some(home);
            link.tab_menu = None;
        }
        cx.notify();
    }

    /// A peer's tab let go after a drag, at `position` in this window: off
    /// the row it pops out; a popped-out one let go over the test window's
    /// row, outside its own window (which may be in front of that row),
    /// docks back.
    pub(in crate::shell) fn end_peer_drag(
        &mut self,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(link) = self.play.live.local.peer.as_mut() else {
            return;
        };
        let Some(home) = link.dragging.take() else {
            return;
        };
        let size = window.viewport_size();
        let inside = position.x >= px(0.)
            && position.y >= px(0.)
            && position.x <= size.width
            && position.y <= size.height;
        if link.detached && inside {
            return;
        }
        let at = window.bounds().origin + position;
        with_test_window(link.tabs.clone(), cx, move |t, w, cx| {
            t.dropped(home, at, w, cx)
        });
    }

    fn set_peer_tab_menu(&mut self, menu: Option<(Home, Point<Pixels>)>, cx: &mut Context<Self>) {
        if let Some(link) = self.play.live.local.peer.as_mut() {
            link.tab_menu = menu;
        }
        cx.notify();
    }

    /// A popped-out peer's window closing from its own title bar: back to
    /// its tab. False in any other window.
    pub(in crate::shell) fn local_run_dock_back(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(link) = self.play.live.local.peer.as_ref().filter(|l| l.detached) else {
            return false;
        };
        let home = link.home;
        with_test_window(link.tabs.clone(), cx, move |t, w, cx| t.attach(home, w, cx));
        true
    }
}

/// A peer tab's right-click menu: pop it out (or dock it back), and close
/// a player's.
fn peer_tab_menu(
    view: &Entity<TestWindow>,
    home: Home,
    label: SharedString,
    at: Point<Pixels>,
    detached: bool,
    can_detach: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let move_view = view.downgrade();
    let moving = if detached {
        menu::row_chrome(
            "peer-tab-menu-dock",
            Some(IconName::AppWindow),
            "Dock Back as a Tab".into(),
            true,
            false,
        )
        .on_click(cx.listener(move |shell, _, _, cx| {
            shell.set_peer_tab_menu(None, cx);
            with_test_window(move_view.clone(), cx, move |t, w, cx| t.attach(home, w, cx));
        }))
    } else {
        menu::row_chrome(
            "peer-tab-menu-open",
            Some(IconName::ExternalLink),
            "Open in New Window".into(),
            can_detach,
            false,
        )
        .when(can_detach, |this| {
            this.on_click(cx.listener(move |shell, _, _, cx| {
                shell.set_peer_tab_menu(None, cx);
                with_test_window(move_view.clone(), cx, move |t, w, cx| {
                    t.detach(home, None, w, cx)
                });
            }))
        })
    };
    let close = match home {
        Home::Player(player) => Some(
            menu::row_chrome(
                "peer-tab-menu-close",
                Some(IconName::X),
                format!("Close {label}").into(),
                true,
                false,
            )
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.set_peer_tab_menu(None, cx);
                shell.local_run_close_tab(player, cx);
            })),
        ),
        Home::Server => None,
    };
    let surface = menu::surface()
        .id("peer-tab-menu")
        .occlude()
        .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
            shell.set_peer_tab_menu(None, cx);
        }))
        .child(moving)
        .children(close);
    deferred(
        anchored()
            .position(at)
            .snap_to_window_with_margin(px(8.))
            .child(surface),
    )
    .with_priority(1)
    .into_any_element()
}

/// A collaborator asking to join the session: their name, Accept and ✕.
fn offer_chip(index: usize, from: u64, name: String, cx: &mut Context<Shell>) -> Stateful<Div> {
    let label = SharedString::from(format!("{name} wants to join as a player"));
    h_flex()
        .id(("peer-offer", index))
        .flex_none()
        .items_center()
        .gap(px(8.))
        .ml(px(6.))
        .mb(px(4.))
        .pl(px(10.))
        .pr(px(4.))
        .py(px(4.))
        .rounded(tokens::radius_tile())
        .border_1()
        .border_color(tokens::accent_line())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(tokens::text())
        .role(Role::Group)
        .aria_label(label.clone())
        .child(Icon::new(IconName::UserPlus).size(px(12.)))
        .child(div().truncate().child(label))
        .child(
            div()
                .id(("peer-offer-accept", index))
                .px(px(8.))
                .py(px(2.))
                .rounded(tokens::radius_tile())
                .cursor_pointer()
                .role(Role::Button)
                .text_color(tokens::text_full())
                .bg(tokens::accent_line())
                .hover(tokens::hover_fx)
                .on_click(
                    cx.listener(move |shell, _, _, cx| shell.local_run_accept_remote(from, cx)),
                )
                .child("Accept"),
        )
        .child(
            h_flex()
                .id(("peer-offer-decline", index))
                .size(px(18.))
                .items_center()
                .justify_center()
                .rounded(tokens::radius_tile())
                .cursor_pointer()
                .role(Role::Button)
                .aria_label(SharedString::from(format!("Turn {name} down")))
                .text_color(tokens::text_label())
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .on_click(
                    cx.listener(move |shell, _, _, cx| shell.local_run_decline_remote(from, cx)),
                )
                .child(Icon::new(IconName::X).size(px(8.))),
        )
}

/// One peer's tab, as the row draws it.
struct PeerTab {
    index: usize,
    home: Home,
    label: SharedString,
    active: bool,
    count: usize,
    /// A player playing from their own editor: the globe.
    from_afar: bool,
    /// The tab of a popped-out peer's own window.
    detached: bool,
}

/// One peer's tab, as `chrome::document_tab` draws a document's. Pressed,
/// it shows; dragged off the row, it pops out (or, popped out, back in);
/// right-clicked, its menu.
fn peer_tab(view: &Entity<TestWindow>, tab: PeerTab, cx: &mut Context<Shell>) -> Stateful<Div> {
    let PeerTab {
        index,
        home,
        label,
        active,
        count,
        from_afar,
        detached,
    } = tab;
    let player = match home {
        Home::Player(player) => Some(player),
        Home::Server => None,
    };
    let icon = match (player, from_afar) {
        (Some(_), true) => IconName::Globe,
        (Some(_), false) => IconName::PersonStanding,
        (None, _) => IconName::Server,
    };
    let view = view.downgrade();
    let shell = cx.entity();
    let ghost = DraggedPeer {
        icon,
        label: label.clone(),
    };
    h_flex()
        .id(("peer-tab", index))
        .flex_none()
        .items_center()
        .gap(px(7.))
        .pl(px(16.))
        .pr(if player.is_some() { px(8.) } else { px(16.) })
        .py(px(8.))
        .rounded_t(tokens::radius_tile())
        .relative()
        .cursor_pointer()
        .role(Role::Tab)
        .aria_label(label.clone())
        .aria_selected(active)
        .aria_position_in_set(index + 1)
        .aria_size_of_set(count)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(if active {
            tokens::text_full()
        } else {
            tokens::text_tab_idle(tokens::text_placeholder())
        })
        .when(active, |this| {
            this.bg(tokens::dock())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .bottom(px(-1.))
                        .h(px(2.))
                        .bg(tokens::tab_active_bar()),
                )
        })
        .when(!active, |this| {
            this.hover(|this| tokens::hover_fx(this).bg(tokens::hover_subtle()))
        })
        // On the press, not the click: the tab is also a drag handle, and
        // a press that goes on to move never clicks. Deferred: showing a
        // tab updates this editor, which the press is still inside.
        .when(!detached, |this| {
            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let view = view.clone();
                window.defer(cx, move |window, cx| {
                    let _ = view.update(cx, |tabs, cx| tabs.activate(index, window, cx));
                });
            })
        })
        // Popping out the only tab would leave the row's window empty.
        .when(detached || count > 1, |this| {
            this.on_drag(ghost, move |ghost, _, _, cx| {
                shell.update(cx, |shell, cx| shell.begin_peer_drag(home, cx));
                cx.new(|_| ghost.clone())
            })
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                shell.set_peer_tab_menu(Some((home, event.position)), cx);
            }),
        )
        .when_some(player, |this, player| {
            this.on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |shell, _, _, cx| shell.local_run_close_tab(player, cx)),
            )
        })
        .child(Icon::new(icon).size(px(13.)))
        .child(div().truncate().child(label.clone()))
        .when_some(player, |this, player| {
            let close = SharedString::from(format!("Close {label}"));
            this.child(
                h_flex()
                    .id(("peer-tab-close", index))
                    .flex_none()
                    .size(px(16.))
                    .items_center()
                    .justify_center()
                    .role(Role::Button)
                    .aria_label(close.clone())
                    .text_color(tokens::text_label())
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .tooltip(move |window, cx| {
                        crate::shell::tooltip::text(close.clone(), window, cx)
                    })
                    // Neither shows the tab nor starts dragging it.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        cx.stop_propagation();
                        shell.local_run_close_tab(player, cx);
                    }))
                    .child(Icon::new(IconName::X).size(px(8.))),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{Home, Tabs};
    use gpui_kit::Keystroke;
    use rbx_dom::WeakDom;

    /// The Server and `n` players, each editor standing in as its name.
    fn players(n: usize) -> (Tabs<&'static str>, Vec<rbx_dom::Ref>) {
        const NAMES: [&str; 4] = ["Player1", "Player2", "Player3", "Player4"];
        let mut dom = WeakDom::new();
        let refs: Vec<_> = (0..n)
            .map(|i| dom.new_instance("Player", NAMES[i], None))
            .collect();
        let mut tabs = Tabs::default();
        tabs.add(Home::Server, "Server");
        for (r, name) in refs.iter().zip(NAMES) {
            tabs.add(Home::Player(*r), name);
        }
        (tabs, refs)
    }

    fn key(spelling: &str) -> Keystroke {
        Keystroke::parse(spelling).expect("a keystroke")
    }

    #[test]
    fn the_server_tab_comes_first_and_players_follow_in_order() {
        let (mut tabs, p) = players(2);
        assert_eq!(
            tabs.homes(),
            [Home::Server, Home::Player(p[0]), Home::Player(p[1])]
        );
        assert_eq!(tabs.active(), 0);
        // A late Server (opened after a player) still goes first, and what
        // was shown stays shown.
        let mut late = Tabs::default();
        late.add(Home::Player(p[0]), ());
        late.add(Home::Server, ());
        assert_eq!(late.homes(), [Home::Server, Home::Player(p[0])]);
        assert_eq!(late.input_target(), Some(p[0]));
        // Add Client: a tab at the end, not shown.
        tabs.activate(1);
        let mut dom = WeakDom::new();
        let third = dom.new_instance("Player", "Player3", None);
        assert_eq!(tabs.add(Home::Player(third), "Player3"), 3);
        assert_eq!(tabs.active(), 1);
    }

    #[test]
    fn removing_a_tab_keeps_the_shown_one_or_shows_its_neighbour() {
        let (mut tabs, p) = players(3);
        tabs.activate(3);
        // A tab before the shown one: the same player still shows.
        assert_eq!(tabs.remove(Home::Player(p[0])), Some(("Player1", false)));
        assert_eq!(tabs.input_target(), Some(p[2]));
        // The shown last tab: the new last shows.
        assert_eq!(tabs.remove(Home::Player(p[2])), Some(("Player3", true)));
        assert_eq!(tabs.input_target(), Some(p[1]));
        // The shown middle tab: the one that slid into its place.
        let (mut tabs, p) = players(3);
        tabs.activate(2);
        tabs.remove(Home::Player(p[1]));
        assert_eq!(tabs.input_target(), Some(p[2]));
        assert_eq!(tabs.remove(Home::Player(p[1])), None);
    }

    #[test]
    fn ctrl_tab_cycles_and_ctrl_digits_jump() {
        let (mut tabs, p) = players(3);
        assert!(tabs.key(&key("ctrl-tab")));
        assert_eq!(tabs.active(), 1);
        assert!(tabs.key(&key("ctrl-shift-tab")));
        assert!(tabs.key(&key("ctrl-shift-tab")));
        assert_eq!(tabs.active(), 3, "wraps to the last");
        assert!(tabs.key(&key("ctrl-tab")));
        assert_eq!(tabs.active(), 0, "and back to the Server");
        assert!(tabs.key(&key("secondary-3")));
        assert_eq!(tabs.input_target(), Some(p[1]));
        assert!(!tabs.key(&key("secondary-9")), "no ninth tab");
        assert!(!tabs.key(&key("secondary-3")), "already shown");
        assert!(!tabs.key(&key("tab")), "a plain Tab is the game's");
        assert!(!tabs.key(&key("w")));
        assert_eq!(tabs.active(), 2);
    }

    #[test]
    fn only_the_shown_player_tab_takes_input() {
        let (mut tabs, p) = players(2);
        assert_eq!(tabs.input_target(), None, "the Server tab drives no client");
        tabs.activate(2);
        assert_eq!(tabs.input_target(), Some(p[1]));
        tabs.activate(1);
        assert_eq!(tabs.input_target(), Some(p[0]));
        assert_eq!(Tabs::<()>::default().input_target(), None);
    }

    #[test]
    fn a_popped_out_peer_keeps_its_editor_and_docks_back_where_it_was() {
        let (mut tabs, p) = players(2);
        tabs.activate(1);
        // The shown Player1 pops out: the very editor leaves the row, and
        // the tab that slid into its place shows.
        assert_eq!(tabs.detach(Home::Player(p[0])), Some(("Player1", true)));
        assert_eq!(tabs.homes(), [Home::Server, Home::Player(p[1])]);
        assert_eq!(tabs.shown(), Some(&"Player2"));
        assert!(tabs.is_apart(Home::Player(p[0])));
        // Back: the same editor, in its old place, shown.
        assert_eq!(tabs.attach(Home::Player(p[0])), Some("Player1"));
        assert_eq!(
            tabs.homes(),
            [Home::Server, Home::Player(p[0]), Home::Player(p[1])]
        );
        assert_eq!(tabs.shown(), Some(&"Player1"));
        assert!(!tabs.is_apart(Home::Player(p[0])));
        assert_eq!(tabs.attach(Home::Player(p[0])), None, "already docked");
    }

    #[test]
    fn the_last_tab_never_pops_out() {
        let (mut tabs, p) = players(1);
        assert!(tabs.detach(Home::Server).is_some());
        assert_eq!(tabs.detach(Home::Player(p[0])), None);
        assert_eq!(tabs.homes(), [Home::Player(p[0])]);
        // The Server docks back in front of the player, whatever its
        // place was.
        assert_eq!(tabs.attach(Home::Server), Some("Server"));
        assert_eq!(tabs.homes(), [Home::Server, Home::Player(p[0])]);
    }

    #[test]
    fn a_popped_out_window_with_the_keyboard_takes_its_player_s_input() {
        let (mut tabs, p) = players(2);
        tabs.detach(Home::Player(p[1]));
        assert_eq!(tabs.input_target(), None, "the Server tab shows");
        tabs.set_focused(Home::Player(p[1]), true);
        assert_eq!(tabs.input_target(), Some(p[1]));
        // A docked peer's window never counts as popped out.
        tabs.set_focused(Home::Player(p[0]), true);
        assert_eq!(tabs.input_target(), Some(p[1]));
        tabs.set_focused(Home::Player(p[1]), false);
        assert_eq!(tabs.input_target(), None);
        // A player who leaves from their own window takes it along.
        tabs.set_focused(Home::Player(p[1]), true);
        assert_eq!(tabs.remove(Home::Player(p[1])), Some(("Player2", false)));
        assert_eq!(tabs.input_target(), None);
        assert!(!tabs.is_apart(Home::Player(p[1])));
    }
}
