//! The editor's own dropdown menus (§5.4/§5.5): the container that floats
//! under a trigger, and the rows inside it.
//!
//! Hand-built rather than `gpui_component`'s stock `PopupMenu` because the
//! spec pins the geometry — a 16px container radius against 8px item
//! radius, so items visibly float inside their container; 32px rows; 8px
//! padding; 2px between rows — and none of that is reachable from outside
//! that component. What is reused is the stock [`Popover`], which already
//! owns the hard parts: anchoring, outside-click dismissal, and layering
//! above everything else in the window.
//!
//! Menus are *controlled*: which one is open lives on [`Shell::open_menu`],
//! not inside the popover. That is what lets an item close its own menu
//! when clicked, and guarantees two menus can never be open at once.

use std::rc::Rc;

use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::ui_icons;

use super::Shell;

/// Every menu the shell can open. One value, one menu — `Shell::open_menu`
/// holds at most one, so opening any menu closes whichever was open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuId {
    ExplorerOverflow,
    ViewportOverflow,
    OutputOverflow,
    InsertPart,
    InsertScript,
    InsertGui,
}

impl MenuId {
    fn id(self) -> &'static str {
        match self {
            MenuId::ExplorerOverflow => "menu-explorer",
            MenuId::ViewportOverflow => "menu-viewport",
            MenuId::OutputOverflow => "menu-output",
            MenuId::InsertPart => "menu-insert-part",
            MenuId::InsertScript => "menu-insert-script",
            MenuId::InsertGui => "menu-insert-gui",
        }
    }
}

type Action = Rc<dyn Fn(&mut Shell, &mut Context<Shell>)>;

/// One row. Built with the `with_*` chain rather than a struct literal so a
/// plain item stays a one-liner at the call site.
pub(super) struct Item {
    icon: Option<&'static str>,
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
    pub(super) fn icon(mut self, icon: &'static str) -> Self {
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
    let handle = cx.entity();
    let open = shell.open_menu == Some(menu);

    Popover::new(menu.id())
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

/// §5.8 — the menu arrives rather than appearing: a short fade with a 4px
/// settle, on the "liquid" ease-out every other motion in the editor uses.
/// GPUI has no transform, so the spec's accompanying `scale(0.98)` is a
/// translation only (see `UX_GUIDELINES.md` §10).
fn container(shell: Entity<Shell>, items: &[Item]) -> impl IntoElement {
    menu_surface(shell, items).with_animation(
        "menu-open",
        Animation::new(tokens::DURATION_MENU).with_easing(tokens::easing_soft),
        |this, delta| this.opacity(delta).mt(px(-4. + 4. * delta)),
    )
}

fn menu_surface(shell: Entity<Shell>, items: &[Item]) -> Div {
    v_flex()
        .min_w(px(180.))
        .p(tokens::SPACE_2)
        .gap(px(2.))
        .bg(tokens::bg_2())
        .rounded(tokens::RADIUS_LG)
        .border_1()
        .border_color(tokens::border_mid())
        .shadow(tokens::elevation_2())
        .children(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| row(shell.clone(), index, item)),
        )
}

/// One menu row, in every state §5.5 asks for.
///
/// The selection "flash" is the pressed style rather than a fixed 100ms
/// timer: holding the button paints `accent-soft-bg`, releasing runs the
/// action and closes the menu. A real click holds for roughly that long
/// anyway, and tying the flash to the press means it can never outlive the
/// menu it is confirming.
fn row(shell: Entity<Shell>, index: usize, item: &Item) -> impl IntoElement {
    let enabled = item.enabled && item.action.is_some();
    let action = item.action.clone();
    let label = item.label.clone();

    h_flex()
        .id(("menu-item", index))
        .w_full()
        .h(px(32.))
        .flex_none()
        .items_center()
        .gap(tokens::SPACE_2)
        .px(tokens::SPACE_2)
        .rounded(tokens::RADIUS_SM)
        .text_size(tokens::UI_LABEL_SIZE)
        .line_height(tokens::UI_LABEL_LINE_HEIGHT)
        .map(|this| {
            if enabled {
                this.cursor_pointer()
                    .text_color(tokens::text_primary())
                    .hover(|this| this.bg(tokens::bg_3()))
                    .active(|this| this.bg(tokens::accent_soft_bg()))
            } else {
                // `text-disabled` alone, not the spec's further 40% opacity
                // on top of it: the token is already the dim end of the
                // ramp, and dimming it again lands at 1.3:1, which is not
                // "unavailable" so much as "invisible".
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .when_some(item.icon, |this, icon| {
            this.child(ui_icons::icon(icon).size(px(18.)))
        })
        .child(div().flex_1().child(label))
        .when(item.checked, |this| {
            this.child(ui_icons::icon("check").size(px(14.)))
        })
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
