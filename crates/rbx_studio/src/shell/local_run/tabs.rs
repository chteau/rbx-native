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

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::windows::Home;
use crate::shell::Shell;
use crate::tokens;

/// Which tabs there are and which one shows. The Server's is always the
/// first.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Tabs {
    homes: Vec<Home>,
    active: usize,
}

impl Tabs {
    pub(crate) fn homes(&self) -> &[Home] {
        &self.homes
    }

    pub(crate) fn active(&self) -> usize {
        self.active
    }

    /// A tab for `home`, at the end (the Server's at the front), not shown.
    pub(crate) fn add(&mut self, home: Home) -> usize {
        if home == Home::Server {
            self.homes.insert(0, home);
            if self.homes.len() > 1 {
                self.active += 1;
            }
            return 0;
        }
        self.homes.push(home);
        self.homes.len() - 1
    }

    /// Takes `home`'s tab out; the shown one stays shown, or, when it was
    /// this one, the tab that slid into its place (or the new last) shows.
    pub(crate) fn remove(&mut self, home: Home) -> Option<usize> {
        let at = self.homes.iter().position(|h| *h == home)?;
        self.homes.remove(at);
        if at < self.active || self.active >= self.homes.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some(at)
    }

    /// Shows tab `index`; false when there is none or it already shows.
    pub(crate) fn activate(&mut self, index: usize) -> bool {
        if index >= self.homes.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// Ctrl+Tab (`forward`) and Ctrl+Shift+Tab, wrapping around.
    pub(crate) fn cycle(&mut self, forward: bool) -> bool {
        let count = self.homes.len();
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

    /// The player whose client gets the keyboard and mouse: the shown
    /// tab's, none on the Server's.
    pub(crate) fn input_target(&self) -> Option<Ref> {
        match self.homes.get(self.active)? {
            Home::Player(player) => Some(*player),
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
    tabs: Tabs,
    shells: Vec<Entity<Shell>>,
    /// Collaborators asking to join: (their end, name).
    pub(super) offers: Vec<(u64, String)>,
    /// Players playing from their own editor.
    pub(super) afar: Vec<Ref>,
}

impl TestWindow {
    pub(crate) fn new(home: Home, shell: Entity<Shell>) -> Self {
        let mut tabs = Tabs::default();
        tabs.add(home);
        TestWindow {
            tabs,
            shells: vec![shell],
            offers: Vec::new(),
            afar: Vec::new(),
        }
    }

    pub(crate) fn tabs(&self) -> &Tabs {
        &self.tabs
    }

    /// The editor on tab `index`.
    pub(crate) fn shell(&self, index: usize) -> Option<&Entity<Shell>> {
        self.shells.get(index)
    }

    pub(crate) fn add(&mut self, home: Home, shell: Entity<Shell>, cx: &mut Context<Self>) {
        let at = self.tabs.add(home);
        self.shells.insert(at, shell);
        cx.notify();
    }

    pub(crate) fn remove(&mut self, home: Home, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.tabs.active();
        let Some(at) = self.tabs.remove(home) else {
            return;
        };
        self.shells.remove(at);
        if at == shown {
            self.shown_changed(None, window, cx);
        }
        cx.notify();
    }

    /// Shows tab `index` (a click on it).
    pub(crate) fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.shells.get(self.tabs.active()).cloned();
        if self.tabs.activate(index) {
            self.shown_changed(old, window, cx);
        }
    }

    fn key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let old = self.shells.get(self.tabs.active()).cloned();
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
        let shown = self.shells.get(self.tabs.active()).cloned();
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
            .children(self.shells.get(self.tabs.active()).cloned())
    }
}

impl Shell {
    /// A test tab's row of tabs, where the editor has its documents: one
    /// per peer, styled as the document tabs, a player's with a close mark,
    /// and "+" for Add Client.
    pub(in crate::shell) fn local_run_peer_tabs(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let link = self.play.live.local.peer.as_ref()?;
        let afar = link.remote;
        let view = link.tabs.upgrade()?;
        let window = view.read(cx);
        // Collaborators asking to join, and who plays from their editor, as
        // the host last said (reading the host here could catch it mid-update).
        let (offers, remote) = (window.offers.clone(), window.afar.clone());
        let active = window.tabs().active();
        let count = window.tabs().homes().len();
        let tabs: Vec<(usize, Home, SharedString)> = window
            .tabs()
            .homes()
            .iter()
            .enumerate()
            .map(|(i, home)| {
                // This editor is mid-render: its own title, not a read.
                let title = match window.shell(i) {
                    Some(s) if s.entity_id() == cx.entity_id() => self.title.clone(),
                    Some(s) => s.read(cx).title.clone(),
                    None => SharedString::default(),
                };
                (i, *home, title)
            })
            .collect();
        let tabs: Vec<_> = tabs
            .into_iter()
            .map(|(index, home, label)| {
                let from_afar = matches!(home, Home::Player(p) if remote.contains(&p));
                peer_tab(
                    &view,
                    index,
                    home,
                    label,
                    index == active,
                    count,
                    from_afar,
                    cx,
                )
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
                .when(!afar, |this| {
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
                .into_any_element(),
        )
    }
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

/// One peer's tab, as `chrome::document_tab` draws a document's; a player
/// playing from their own editor (`from_afar`) has the globe.
#[allow(clippy::too_many_arguments)]
fn peer_tab(
    view: &Entity<TestWindow>,
    index: usize,
    home: Home,
    label: SharedString,
    active: bool,
    count: usize,
    from_afar: bool,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
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
        // Deferred: showing a tab updates this editor, which the click is
        // still inside.
        .on_click(move |_, window, cx| {
            let view = view.clone();
            window.defer(cx, move |window, cx| {
                let _ = view.update(cx, |tabs, cx| tabs.activate(index, window, cx));
            });
        })
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

    fn players(n: usize) -> (Tabs, Vec<rbx_dom::Ref>) {
        let mut dom = WeakDom::new();
        let refs: Vec<_> = (0..n)
            .map(|i| dom.new_instance("Player", &format!("Player{}", i + 1), None))
            .collect();
        let mut tabs = Tabs::default();
        tabs.add(Home::Server);
        for r in &refs {
            tabs.add(Home::Player(*r));
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
        late.add(Home::Player(p[0]));
        late.add(Home::Server);
        assert_eq!(late.homes(), [Home::Server, Home::Player(p[0])]);
        assert_eq!(late.input_target(), Some(p[0]));
        // Add Client: a tab at the end, not shown.
        tabs.activate(1);
        let mut dom = WeakDom::new();
        let third = dom.new_instance("Player", "Player3", None);
        assert_eq!(tabs.add(Home::Player(third)), 3);
        assert_eq!(tabs.active(), 1);
    }

    #[test]
    fn removing_a_tab_keeps_the_shown_one_or_shows_its_neighbour() {
        let (mut tabs, p) = players(3);
        tabs.activate(3);
        // A tab before the shown one: the same player still shows.
        assert_eq!(tabs.remove(Home::Player(p[0])), Some(1));
        assert_eq!(tabs.input_target(), Some(p[2]));
        // The shown last tab: the new last shows.
        tabs.remove(Home::Player(p[2]));
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
        assert_eq!(Tabs::default().input_target(), None);
    }
}
