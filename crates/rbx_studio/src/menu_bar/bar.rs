//! The bar itself: the four titles, which one the keyboard is on, and the
//! dropdown the open one shows.
//!
//! This exists because `gpui_component`'s ready-made `AppMenuBar` keeps its
//! current title in a private field and offers no way to set it, so nothing
//! outside it can put the keyboard *into* the bar. Everything below the
//! titles is still the toolkit's: each dropdown is a stock `PopupMenu`, which
//! already owns Up/Down through the items, Enter to activate and Escape to
//! close, and which deliberately propagates Left/Right back up to whoever
//! owns the bar so an open menu can be walked out of sideways.
//!
//! The bar is **not** a Tab stop. Tab walks the editor's regions (see
//! `shell::roving`); the desktop convention for a menu bar is F10 or a bare
//! Alt tap instead, and putting it in both places would only add a stop
//! everyone has to pass through to reach the ribbon.

use gpui_kit::base::actions::{Cancel, Confirm, SelectDown, SelectLeft, SelectRight};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{h_flex, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::alt_tap::AltTap;
use super::popup;

/// The key context the bar's own bindings live in. Deliberately not the
/// toolkit's own `AppMenuBar`, which this replaces: reusing that name would
/// silently inherit whatever that component binds, now and later.
pub(crate) const CONTEXT: &str = "RbxMenuBar";

/// Binds the keys the bar answers, scoped to [`CONTEXT`] so they only mean
/// anything while focus is inside the bar or the menu it has open.
pub(crate) fn install_key_bindings(cx: &mut App) {
    // Real Studio's own shortcut for its Settings dialog. Global, so it
    // works from anywhere in the editor, and so the File menu shows it.
    cx.bind_keys([KeyBinding::new("alt-s", super::MenuStudioSettings, None)]);
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm { secondary: false }, Some(CONTEXT)),
        KeyBinding::new("space", Confirm { secondary: false }, Some(CONTEXT)),
    ]);
}

/// Where Left/Right lands, given `len` titles. Wraps, for the same reason
/// `shell::roving::Move::apply` does: a four-item strip running off its end
/// and stopping reads as the key having failed rather than as a boundary.
fn next_title(current: usize, len: usize, back: bool) -> usize {
    let last = len.saturating_sub(1);
    match (current, back) {
        (0, true) => last,
        (current, true) => current - 1,
        (current, false) if current >= last => 0,
        (current, false) => current + 1,
    }
}

/// Read once at startup by `Shell::new`; documented on
/// [`MenuBar::apply_debug_entry`].
pub(crate) const MENU_VARIABLE: &str = "RBX_STUDIO_MENU";

pub(crate) struct MenuBar {
    menus: Vec<OwnedMenu>,
    /// One focus handle per title, created once and kept: a title holding
    /// focus *is* "the keyboard is in the menu bar", so these are also what
    /// [`MenuBar::entered`] reads.
    handles: Vec<FocusHandle>,
    /// Which title the keyboard last moved to. Only meaningful while one of
    /// [`MenuBar::handles`] holds focus or a menu is open.
    current: usize,
    /// Which title's dropdown is showing.
    open: Option<usize>,
    /// Rebuilt each time a menu opens rather than kept per title: a dropped
    /// `PopupMenu` is the only way to be sure no stale selection survives
    /// into the next opening.
    popup: Option<Entity<PopupMenu>>,
    /// Where focus was when F10 (or a bare Alt) reached into the bar, so
    /// Escape can put it back exactly there.
    restore: Option<FocusHandle>,
    alt: AltTap,
    _dismissed: Option<Subscription>,
}

impl MenuBar {
    pub(crate) fn new(menus: Vec<OwnedMenu>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| MenuBar {
            handles: menus.iter().map(|_| cx.focus_handle()).collect(),
            menus,
            current: 0,
            open: None,
            popup: None,
            restore: None,
            alt: AltTap::default(),
            _dismissed: None,
        })
    }

    /// F10, and the completed bare Alt tap, both land here: a toggle, because
    /// the key that reaches into the bar is also the one that gets back out
    /// of it without hunting for Escape.
    pub(crate) fn toggle_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.entered(window) {
            self.leave(window, cx);
        } else {
            self.enter(window, cx);
        }
    }

    /// One modifier change, on its way to [`AltTap`]. Completing a tap enters
    /// the bar exactly as F10 does.
    pub(crate) fn modifiers_changed(
        &mut self,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.alt.modifiers_changed(modifiers) {
            self.toggle_entry(window, cx);
        }
    }

    /// `RBX_STUDIO_MENU=1` reaches into the bar exactly as F10 does, and
    /// `RBX_STUDIO_MENU=<title>` (File/Edit/Model/View) goes on to open that
    /// menu — a debugging aid for a screenshot, since nothing else on this
    /// machine can send a keystroke to the editor on its behalf (see
    /// `agents/AGENTS.md`'s safety rules).
    pub(crate) fn apply_debug_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(MENU_VARIABLE) else {
            return;
        };
        self.toggle_entry(window, cx);
        if let Some(index) = self
            .menus
            .iter()
            .position(|menu| menu.name.as_ref() == spec)
        {
            self.open_menu(index, cx);
        }
    }

    /// Anything that is not a modifier change, arriving while Alt is held —
    /// see [`AltTap`] for why a tap has to be cancellable at all.
    pub(crate) fn interrupt_alt_tap(&mut self) {
        self.alt.interrupt();
    }

    fn entered(&self, window: &Window) -> bool {
        self.open.is_some() || self.handles.iter().any(|handle| handle.is_focused(window))
    }

    /// Enters at the first title rather than wherever the bar was left: a
    /// menu bar people reach for blind is only predictable if it always
    /// starts in the same place.
    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore = window.focused(cx);
        self.current = 0;
        self.focus_current(window, cx);
    }

    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        if let Some(restore) = self.restore.take() {
            restore.focus(window, cx);
        }
        cx.notify();
    }

    fn focus_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.handles.get(self.current) {
            handle.clone().focus(window, cx);
        }
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        self.popup = None;
        self._dismissed = None;
        cx.notify();
    }

    /// Opens `index`'s dropdown, replacing whatever was open. The popup takes
    /// focus itself once built (see [`MenuBar::render`]), which is what hands
    /// Up/Down and Enter over to it.
    fn open_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close(cx);
        self.current = index;
        self.open = Some(index);
        cx.notify();
    }

    fn toggle_menu(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == Some(index) {
            self.close(cx);
            self.focus_current(window, cx);
        } else {
            self.open_menu(index, cx);
        }
    }

    /// True while a title — not the open dropdown — holds focus. The keys
    /// that mean "open this menu" only mean that there; inside the dropdown
    /// the same keys belong to the items.
    fn title_focused(&self, window: &Window) -> bool {
        self.handles
            .get(self.current)
            .is_some_and(|handle| handle.is_focused(window))
    }

    fn step(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = next_title(self.current, self.menus.len(), back);
        // Walking sideways with a menu open opens the next one rather than
        // just moving the highlight — every desktop menu bar does this, and
        // it is why `PopupMenu` propagates Left/Right up here at all.
        if self.open.is_some() {
            self.open_menu(next, cx);
        } else {
            self.current = next;
            self.focus_current(window, cx);
        }
    }

    fn on_select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.step(true, window, cx);
    }

    fn on_select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.step(false, window, cx);
    }

    fn on_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let current = self.current;
        self.open_menu(current, cx);
    }

    fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if !self.title_focused(window) {
            return;
        }
        let current = self.current;
        self.toggle_menu(current, window, cx);
    }

    /// Escape leaves in two steps, the way a menu bar is expected to: the
    /// open dropdown first (focus stays on its title), the bar itself second.
    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_some() {
            self.close(cx);
            self.focus_current(window, cx);
        } else {
            self.leave(window, cx);
        }
    }

    /// A dropdown dismissing itself — an item clicked, a click outside, its
    /// own Escape. Focus goes back to the title only if the keyboard was what
    /// put it here; a mouse user is not chasing focus around.
    fn on_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keyboard = self.restore.is_some();
        self.close(cx);
        if keyboard {
            self.focus_current(window, cx);
        }
    }

    fn build_popup(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menus.get(index) else {
            return;
        };
        // The same handle `AppMenuBar` used to hand its popup: an action a
        // menu item fires dispatches from where focus was before the bar took
        // it, not from the bar.
        let popup = popup::dropdown(&menu.items, self.restore.clone(), window, cx);
        self._dismissed = Some(cx.subscribe_in(
            &popup,
            window,
            |bar, _, _: &DismissEvent, window, cx| bar.on_dismiss(window, cx),
        ));
        self.popup = Some(popup);
    }

    /// One title, plus the dropdown hanging off it while it is the open one.
    ///
    /// The focus handle is on the wrapper rather than on the `Button`: the
    /// toolkit's button makes its own handle internally, which nothing
    /// outside it can focus. `tab_stop(false)` on both is what keeps the bar
    /// out of GPUI's own Tab sequence — and it has to go on the *handle*,
    /// since an element carrying `track_focus` ignores the element-level
    /// setting (see `shell::roving::Roving::item`).
    fn title(&self, index: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let name = self.menus[index].name.clone();
        let open = self.open == Some(index);
        let handle = self.handles[index].clone().tab_stop(false);
        // The ring is conditioned on `restore` rather than left to GPUI's
        // `focus_visible`, which paints only for focus GPUI itself moved with
        // a key. Focus gets here by this module calling `.focus()`, which does
        // not count — and a focused control with no ring is WCAG 2.4.7. There
        // is no ambiguity to resolve either way: `restore` is set only by
        // F10/Alt, so it *is* "the keyboard put us here".
        let keyboard = self.restore.is_some() && self.handles[index].is_focused(window);

        div()
            .id(("menu-title", index))
            .relative()
            .track_focus(&handle)
            .rounded(tokens::radius())
            .when(keyboard, |this| {
                this.shadow(tokens::focus_ring(tokens::menu_bar()))
            })
            .child(
                Button::new(("menu-title-button", index))
                    .small()
                    .py_0p5()
                    .compact()
                    .ghost()
                    .tab_stop(false)
                    .label(name)
                    .selected(open)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |bar, _, window, cx| {
                            // Stop propagation to avoid dragging the window.
                            window.prevent_default();
                            cx.stop_propagation();
                            bar.toggle_menu(index, window, cx);
                        }),
                    ),
            )
            .on_hover(cx.listener(move |bar, hovered: &bool, _, cx| {
                // Hover only *moves* an already-open bar, the way every
                // desktop menu bar behaves; it never opens one.
                if *hovered && bar.open.is_some() && bar.open != Some(index) {
                    bar.open_menu(index, cx);
                }
            }))
            .when(open, |this| {
                this.children(self.popup.clone().map(|popup| {
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .snap_to_window_with_margin(px(8.))
                            .child(div().size_full().occlude().top_1().child(popup)),
                    )
                }))
            })
            .into_any_element()
    }
}

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Built here rather than when the menu opened, because a `PopupMenu`
        // needs a `Window` to exist at all and the keyboard path that opens
        // one does not always have a render pass under it.
        if let Some(index) = self.open {
            if self.popup.is_none() {
                self.build_popup(index, window, cx);
                if let Some(popup) = self.popup.clone() {
                    popup.read(cx).focus_handle(cx).focus(window, cx);
                }
            }
        }

        let titles: Vec<AnyElement> = (0..self.menus.len())
            .map(|index| self.title(index, window, cx))
            .collect();

        h_flex()
            .id("menu-bar")
            .role(Role::MenuBar)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_cancel))
            .size_full()
            .gap_x_1()
            .children(titles)
    }
}

#[cfg(test)]
#[path = "bar/tests.rs"]
mod tests;
