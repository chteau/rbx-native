//! The Explorer's right-click menu.
//!
//! `studio/explorer.md`: "Right-clicking on an instance opens the options
//! menu, contextually adjusted for the object type. For example,
//! right-clicking a `Model` reveals standard options like Copy and
//! Duplicate, and also options specific to Models like Ungroup. In contrast,
//! right-clicking a service like `Lighting` reveals a more concise menu."
//!
//! Contextual here means *greyed*, not absent — §6's "disabled beats
//! absent": a service's menu still shows Cut and Group so the shape of what
//! an instance can do stays the same wherever you right-click, and each row
//! is enabled by exactly the predicate its own handler returns early on
//! (`clipboard::has_copyable`, `group::has_groupable`/`has_ungroupable`), so
//! a row can never be clickable and do nothing.
//!
//! Rows are built here rather than through [`menu::item`] because Rename,
//! Insert and Change Class need a `Window` the controlled dropdown's action type does not
//! carry; they share [`menu::row_chrome`] so both menus stay one design.
//!
//! The keyboard follows the same WAI-ARIA menu pattern as the dropdowns:
//! Shift+F10 or the Menu key opens it on the selected row (at the pointer,
//! where every Explorer popup goes); open, it holds focus, Up/Down move a
//! highlight and wrap, Home/End jump to the ends, Enter or Space runs the
//! highlighted row, and Escape closes it. However it closes, focus goes
//! back to wherever it was when the menu opened (the tree, or the UI
//! editor's canvas). Disabled rows take the highlight but cannot run, and
//! the pointer moves the same highlight.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::roving::Move;
use super::super::{clipboard, group, keys, menu};
use super::rename::renameable;
use super::Shell;
use crate::change_class;

/// Which of the menu's rows are live. Every field is the guard the row's own
/// handler returns early on, so a row can never be clickable and do nothing
/// — and the whole set is decidable without a window, which is what makes
/// the contextual half of this menu testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Availability {
    /// Cut, Copy and Duplicate, which all act on the same filtered set.
    pub(super) clipboard: bool,
    pub(super) paste: bool,
    pub(super) rename: bool,
    pub(super) group: bool,
    pub(super) ungroup: bool,
    pub(super) delete: bool,
    /// Live while any of the selection can change class; the command
    /// converts those and names the rest (see `shell::change_class`).
    pub(super) change_class: bool,
}

pub(super) fn availability(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
    clipboard_empty: bool,
    target: Ref,
) -> Availability {
    Availability {
        clipboard: clipboard::has_copyable(dom, database, selected),
        paste: !clipboard_empty,
        rename: renameable(dom, database, target),
        group: group::has_groupable(dom, database, selected),
        ungroup: group::has_ungroupable(dom, database, selected),
        delete: keys::removable(dom, database, target),
        change_class: selected
            .iter()
            .any(|&referent| change_class::changeable(dom, database, referent)),
    }
}

/// One open context menu: the row it was opened on, kept so Insert and
/// Rename act on that row rather than on whatever the selection later
/// becomes, and its keyboard focus and highlight.
pub(super) struct RowMenu {
    target: Ref,
    focus: FocusHandle,
    cursor: Option<usize>,
    /// What had focus before the menu took it, given back on close.
    previous: Option<FocusHandle>,
}

impl RowMenu {
    pub(super) fn focus(&mut self, window: &mut Window, cx: &mut App) {
        if self.previous.is_none() {
            self.previous = window.focused(cx);
        }
        self.focus.focus(window, cx);
    }
}

/// Whether `keystroke` asks for the selected row's menu: Shift+F10, or the
/// Menu key on keyboards that have one, the two every desktop shares.
pub(in crate::shell) fn opens_row_menu(keystroke: &Keystroke) -> bool {
    let m = keystroke.modifiers;
    match keystroke.key.as_str() {
        "f10" => m.shift && !m.control && !m.alt && !m.platform,
        "menu" => !m.modified(),
        _ => false,
    }
}

impl Shell {
    /// A right-click on an Explorer row. Selects the row first unless it is
    /// already part of the selection — right-clicking one of three selected
    /// parts has to keep all three, or Copy would silently copy one.
    pub(in crate::shell) fn open_row_menu(
        &mut self,
        target: Ref,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.selected_all().contains(&target) {
            self.select(target, cx);
        }
        self.explorer_edit.pointer = position;
        self.explorer_edit.picker = None;
        self.explorer_edit.renaming = None;
        // A menu reopened over another keeps what the first one took
        // focus from, rather than the first menu itself.
        let previous = self
            .explorer_edit
            .menu
            .take()
            .and_then(|menu| menu.previous);
        self.explorer_edit.menu = Some(RowMenu {
            target,
            focus: cx.focus_handle(),
            cursor: None,
            previous,
        });
        self.explorer_edit.focus_menu = true;
        cx.notify();
    }

    /// Shift+F10 or the Menu key in the tree: the selected row's menu, at
    /// the pointer like every Explorer popup, starting on its first row.
    /// False with nothing selected, so the key falls through.
    pub(in crate::shell) fn open_row_menu_from_keyboard(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.selected() else {
            return false;
        };
        self.open_row_menu(target, self.explorer_edit.pointer, cx);
        if let Some(menu) = &mut self.explorer_edit.menu {
            menu.cursor = Some(0);
        }
        true
    }

    /// Closes the row menu and, if focus is still in it, hands focus back to
    /// what had it when the menu opened, as a popover does.
    fn close_row_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.explorer_edit.menu.take() {
            if let Some(previous) = menu
                .previous
                .filter(|_| menu.focus.contains_focused(window, cx))
            {
                previous.focus(window, cx);
            }
        }
        cx.notify();
    }

    /// The open menu's keys. `actions` is each row's handler where the row
    /// can run, in row order.
    fn row_menu_key(
        &mut self,
        keystroke: &Keystroke,
        actions: &[Option<RowAction>],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = &mut self.explorer_edit.menu else {
            return false;
        };
        if let Some(movement) = Move::of(keystroke, true) {
            menu.cursor = movement.from(menu.cursor, actions.len());
            cx.notify();
            return true;
        }
        match keystroke.key.as_str() {
            "escape" => {
                self.close_row_menu(window, cx);
                true
            }
            "enter" | "space" => {
                // Nothing highlighted (a right-click open): just close, as
                // the dropdowns do. A disabled row runs nothing.
                let Some(index) = menu.cursor else {
                    self.close_row_menu(window, cx);
                    return true;
                };
                if let Some(action) = actions.get(index).cloned().flatten() {
                    // Closed and focus handed back first, as a click does:
                    // Rename and Insert open something of their own, which
                    // then takes focus from it.
                    self.close_row_menu(window, cx);
                    action(self, window, cx);
                    cx.notify();
                }
                true
            }
            _ => false,
        }
    }

    pub(super) fn row_menu_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.explorer_edit.menu.as_ref()?;
        let (target, focus, cursor) = (open.target, open.focus.clone(), open.cursor);
        let mut live = availability(
            &self.dom,
            &self.database,
            self.selected_all(),
            self.clipboard_is_empty(),
            target,
        );
        // On the UI editor's canvas a group is a `Frame`, and a `Frame`
        // comes apart again (see `ui_editor::arrange`).
        let canvas = self.ui_canvas_active();
        live.ungroup |= canvas && self.ui_can_ungroup();

        let rows = [
            row(
                "cut",
                IconName::Scissors,
                "Cut",
                live.clipboard,
                |shell, _, cx| shell.cut_selected(cx),
            ),
            row(
                "copy",
                IconName::Copy,
                "Copy",
                live.clipboard,
                |shell, _, cx| shell.copy_selected(cx),
            ),
            row(
                "duplicate",
                IconName::CopyPlus,
                "Duplicate",
                live.clipboard,
                |shell, _, cx| shell.duplicate_selected(cx),
            ),
            row(
                "paste-into",
                IconName::ClipboardPaste,
                "Paste Into",
                live.paste,
                |shell, _, cx| shell.paste_into_selected(cx),
            ),
            row(
                "rename",
                IconName::Pencil,
                "Rename",
                live.rename,
                move |shell, window, cx| shell.begin_rename(target, window, cx),
            ),
            row(
                "insert",
                IconName::Plus,
                "Insert Object…",
                true,
                move |shell, window, cx| shell.open_insert_picker(target, window, cx),
            ),
            row(
                "change-class",
                IconName::Replace,
                "Change Class…",
                live.change_class,
                |shell, window, cx| {
                    let targets = shell.selected_all().to_vec();
                    shell.open_change_class_picker(targets, window, cx);
                },
            ),
            row(
                "group",
                IconName::Package,
                if canvas {
                    "Group in a Frame"
                } else {
                    "Group as Model"
                },
                live.group,
                |shell, _, cx| shell.group_selected(cx),
            ),
            row(
                "ungroup",
                IconName::PackageOpen,
                "Ungroup",
                live.ungroup,
                |shell, _, cx| shell.ungroup_selected(cx),
            ),
            row(
                "delete",
                IconName::Trash,
                "Delete",
                live.delete,
                |shell, _, cx| shell.delete_selected(cx),
            ),
        ];
        let actions: Vec<Option<RowAction>> = rows
            .iter()
            .map(|row| row.enabled.then(|| row.action.clone()))
            .collect();
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| row.build(index, cursor == Some(index), cx))
            .collect::<Vec<_>>();

        let surface = menu::surface()
            .id("explorer-row-menu")
            .track_focus(&focus)
            .on_key_down(cx.listener(move |shell, event: &KeyDownEvent, window, cx| {
                if shell.row_menu_key(&event.keystroke, &actions, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .occlude()
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, window, cx| {
                shell.close_row_menu(window, cx);
            }))
            .children(rows);

        Some(
            deferred(
                anchored()
                    .position(self.popup_anchor())
                    .snap_to_window_with_margin(px(8.))
                    .child(surface),
            )
            .into_any_element(),
        )
    }
}

/// What one row does when clicked. A `Window` as well as a `Context`,
/// unlike the controlled dropdown's own action: Rename and Insert both open
/// something that needs one.
type RowAction = std::rc::Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

/// One row of the menu, before it has a `Context` to bind its handler
/// through — the array above has to be built as data first so every row can
/// be laid out in one `map`.
struct Row {
    id: &'static str,
    icon: IconName,
    label: &'static str,
    enabled: bool,
    action: RowAction,
}

fn row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    enabled: bool,
    action: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) -> Row {
    Row {
        id,
        icon,
        label,
        enabled,
        action: std::rc::Rc::new(action),
    }
}

impl Row {
    fn build(self, index: usize, highlighted: bool, cx: &mut Context<Shell>) -> AnyElement {
        let Row {
            id,
            icon,
            label,
            enabled,
            action,
        } = self;
        menu::row_chrome(
            SharedString::from(format!("row-menu-{id}")),
            Some(icon),
            label.into(),
            enabled,
            false,
        )
        // The keyboard's highlight wears the hover's surface, and the
        // pointer moves it, as in the dropdowns (`shell::menu`).
        .when(highlighted, |this| this.bg(crate::tokens::hover()))
        .on_hover(cx.listener(move |shell, hovered: &bool, _, cx| {
            if let Some(menu) = shell.explorer_edit.menu.as_mut().filter(|_| *hovered) {
                menu.cursor = Some(index);
                cx.notify();
            }
        }))
        .when(enabled, |this| {
            this.on_click(cx.listener(move |shell, _, window, cx| {
                // Closed before the action runs, not after: Rename and
                // Insert both open something of their own, and clearing
                // the menu afterwards would take that with it.
                shell.close_row_menu(window, cx);
                action(shell, window, cx);
                cx.notify();
            }))
        })
        .into_any_element()
    }
}
