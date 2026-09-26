//! The editor's own dropdown menus: the container that floats under a
//! trigger, and the rows inside it.
//!
//! Hand-built rather than `gpui_component`'s stock `PopupMenu` because the
//! design pins the geometry — one 3px radius everywhere, 24px rows, 9px
//! labels — and none of that is reachable from outside that component.
//! What is reused is the stock [`Popover`], which already owns the hard
//! parts: anchoring, outside-click dismissal, and layering above everything
//! else in the window.
//!
//! Menus are *controlled*: which one is open lives on [`Shell::open_menu`],
//! not inside the popover. That is what lets an item close its own menu
//! when clicked, and guarantees two menus can never be open at once.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::Shell;

/// Every menu the shell can open. One value, one menu — `Shell::open_menu`
/// holds at most one, so opening any menu closes whichever was open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuId {
    ExplorerOverflow,
    PropertiesOverflow,
    ViewportOverflow,
    OutputOverflow,
    ArgonOverflow,
    ArgonSyncPriority,
    ArgonDisplayPrompts,
    ArgonLogLevel,
    WallyOverflow,
    ScriptAnalysisOverflow,
    /// The version picker on the n-th Wally search result.
    WallyVersion(usize),
    InsertPart,
    InsertScript,
    InsertGui,
    InsertOptions,
    UiResolution,
    UiInsert,
    UiConstraint,
    ViewportScreen,
}

impl MenuId {
    fn id(self) -> &'static str {
        match self {
            MenuId::ExplorerOverflow => "menu-explorer",
            MenuId::PropertiesOverflow => "menu-properties",
            MenuId::ViewportOverflow => "menu-viewport",
            MenuId::OutputOverflow => "menu-output",
            MenuId::ArgonOverflow => "menu-argon",
            MenuId::ArgonSyncPriority => "menu-argon-sync-priority",
            MenuId::ArgonDisplayPrompts => "menu-argon-display-prompts",
            MenuId::ArgonLogLevel => "menu-argon-log-level",
            MenuId::WallyOverflow => "menu-wally",
            MenuId::ScriptAnalysisOverflow => "menu-script-analysis",
            MenuId::WallyVersion(_) => "menu-wally-version",
            MenuId::InsertPart => "menu-insert-part",
            MenuId::InsertScript => "menu-insert-script",
            MenuId::InsertGui => "menu-insert-gui",
            MenuId::InsertOptions => "menu-insert-options",
            MenuId::UiResolution => "menu-ui-resolution",
            MenuId::UiInsert => "menu-ui-insert",
            MenuId::UiConstraint => "menu-ui-constraint",
            MenuId::ViewportScreen => "menu-viewport-screen",
        }
    }

    /// One popover id per menu; an indexed menu carries its index.
    fn element_id(self) -> ElementId {
        match self {
            MenuId::WallyVersion(index) => ElementId::from((self.id(), index)),
            _ => ElementId::from(self.id()),
        }
    }
}

type Action = Rc<dyn Fn(&mut Shell, &mut Context<Shell>)>;

/// One row. Built with the `with_*` chain rather than a struct literal so a
/// plain item stays a one-liner at the call site.
pub(super) struct Item {
    icon: Option<IconName>,
    label: SharedString,
    enabled: bool,
    checked: bool,
    action: Option<Action>,
}

pub(super) fn item(label: impl Into<SharedString>) -> Item {
    Item {
        icon: None,
        label: label.into(),
        enabled: true,
        checked: false,
        action: None,
    }
}

impl Item {
    pub(super) fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A row with no real handler behind it yet. It still draws — seeing
    /// what Studio offers here, greyed, beats a menu that quietly omits it
    /// (the same rule `menu_bar`'s disabled items follow).
    pub(super) fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub(super) fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub(super) fn on_click(
        mut self,
        action: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
    ) -> Self {
        self.action = Some(Rc::new(action));
        self
    }
}

/// A trigger wired to its menu. `trigger` is whatever the caller wants
/// clicked — a ribbon tile, an overflow "…", a panel header button.
pub(super) fn dropdown(
    shell: &Shell,
    menu: MenuId,
    trigger: super::chrome::Trigger,
    items: Vec<Item>,
    cx: &mut Context<Shell>,
) -> impl IntoElement + 'static {
    dropdown_at(shell, menu, trigger, items, Anchor::TopLeft, cx)
}

/// [`dropdown`] with the menu's `anchor` corner on the trigger: a trigger
/// at a window's right edge opens its menu leftwards, one near the bottom
/// upwards.
pub(super) fn dropdown_at(
    shell: &Shell,
    menu: MenuId,
    trigger: super::chrome::Trigger,
    items: Vec<Item>,
    anchor: Anchor,
    cx: &mut Context<Shell>,
) -> impl IntoElement + 'static {
    let handle = cx.entity();
    let open = shell.open_menu == Some(menu);

    Popover::new(menu.element_id())
        .anchor(anchor)
        .open(open)
        // The popover draws no chrome of its own: §5.4's container *is* the
        // chrome, and two stacked backgrounds would double the border.
        .appearance(false)
        .on_open_change({
            let handle = handle.clone();
            move |open, _, cx| {
                let open = *open;
                handle.update(cx, |shell, cx| {
                    shell.open_menu = open.then_some(menu);
                    cx.notify();
                });
            }
        })
        .trigger(trigger)
        .content(move |_, _, _| container(handle.clone(), &items))
}

/// The menu arrives rather than appearing: a short fade with a 4px settle,
/// on the ease-out every other motion in the editor uses. GPUI has no
/// element transform, so this is a translation only (see
/// `UX_GUIDELINES.md`'s deviation list).
fn container(shell: Entity<Shell>, items: &[Item]) -> impl IntoElement {
    menu_surface(shell, items).with_animation(
        "menu-open",
        Animation::new(tokens::DURATION_MENU).with_easing(tokens::easing_soft),
        |this, delta| this.opacity(delta).mt(px(-4. + 4. * delta)),
    )
}

fn menu_surface(shell: Entity<Shell>, items: &[Item]) -> Div {
    surface().children(
        items
            .iter()
            .enumerate()
            .map(|(index, item)| row(shell.clone(), index, item)),
    )
}

/// §9's menu container, without its rows: `chrome`, the one radius, 4px of
/// padding and the elevation its hairline carries. Shared with the
/// Explorer's context menu, which builds its own rows because its actions
/// need a `Window` the controlled dropdown's [`Action`] cannot carry.
pub(super) fn surface() -> Div {
    v_flex()
        .min_w(px(180.))
        .p(px(4.))
        .gap(px(1.))
        .bg(tokens::chrome())
        .rounded(tokens::RADIUS)
        .shadow(tokens::elevation())
}

/// One menu row, in every state it has.
///
/// The selection "flash" is the pressed style rather than a fixed 100ms
/// timer: holding the button paints `accent-soft-bg`, releasing runs the
/// action and closes the menu. A real click holds for roughly that long
/// anyway, and tying the flash to the press means it can never outlive the
/// menu it is confirming.
fn row(shell: Entity<Shell>, index: usize, item: &Item) -> impl IntoElement {
    let enabled = item.enabled && item.action.is_some();
    let action = item.action.clone();

    row_chrome(
        ("menu-item", index),
        item.icon,
        item.label.clone(),
        enabled,
        item.checked,
    )
    .when(enabled, |this| {
        this.on_click(move |_, _, cx| {
            let action = action.clone();
            shell.update(cx, |shell, cx| {
                shell.open_menu = None;
                if let Some(action) = action {
                    action(shell, cx);
                }
                cx.notify();
            });
        })
    })
}

/// One menu row's chrome, without a handler on it. Shared with the
/// Explorer's context menu (see [`surface`]).
pub(super) fn row_chrome(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    label: SharedString,
    enabled: bool,
    checked: bool,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .w_full()
        .h(px(24.))
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .rounded(tokens::RADIUS)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .map(|this| {
            if enabled {
                this.cursor_pointer()
                    .text_color(tokens::text_strong())
                    .hover(|this| this.bg(tokens::hover()))
                    .active(|this| this.bg(tokens::selection()))
            } else {
                // `text-disabled` alone, not the spec's further 40% opacity
                // on top of it: the token is already the dim end of the
                // ramp, and dimming it again lands at 1.3:1, which is not
                // "unavailable" so much as "invisible".
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .when_some(icon, |this, icon| this.child(Icon::new(icon).size(px(12.))))
        .child(div().flex_1().child(label))
        .when(checked, |this| {
            this.child(Icon::new(IconName::Check).size(px(10.)))
        })
}
