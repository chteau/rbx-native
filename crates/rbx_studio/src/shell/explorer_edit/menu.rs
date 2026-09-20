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
//! Rows are built here rather than through [`menu::item`] because Rename and
//! Insert need a `Window` the controlled dropdown's action type does not
//! carry; they share [`menu::row_chrome`] so both menus stay one design.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::{clipboard, group, menu};
use super::rename::renameable;
use super::Shell;

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
        delete: dom.get(target).is_some(),
    }
}

/// One open context menu: the row it was opened on, kept so Insert and
/// Rename act on that row rather than on whatever the selection later
/// becomes.
pub(super) struct RowMenu {
    target: Ref,
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
        self.explorer_edit.menu = Some(RowMenu { target });
        cx.notify();
    }

    pub(super) fn row_menu_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let target = self.explorer_edit.menu.as_ref()?.target;
        let live = availability(
            &self.dom,
            &self.database,
            self.selected_all(),
            self.clipboard_is_empty(),
            target,
        );

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
                "group",
                IconName::Package,
                "Group as Model",
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
        ]
        .map(|row| row.build(cx));

        let surface = menu::surface()
            .id("explorer-row-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.explorer_edit.menu = None;
                cx.notify();
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
type RowAction = Box<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

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
        action: Box::new(action),
    }
}

impl Row {
    fn build(self, cx: &mut Context<Shell>) -> AnyElement {
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
        .when(enabled, |this| {
            this.on_click(cx.listener(move |shell, _, window, cx| {
                // Closed before the action runs, not after: Rename and
                // Insert both open something of their own, and clearing
                // the menu afterwards would take that with it.
                shell.explorer_edit.menu = None;
                action(shell, window, cx);
                cx.notify();
            }))
        })
        .into_any_element()
    }
}
