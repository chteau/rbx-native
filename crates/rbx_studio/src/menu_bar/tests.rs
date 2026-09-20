use gpui_kit::{Action as _, OwnedMenuItem};

use super::{menus, MenuPlaceholder};

/// Every item in the bar either does something or is visibly greyed out.
/// The two halves are one invariant, not two: an enabled item wired to
/// [`MenuPlaceholder`] would look live and do nothing, and a disabled item
/// wired to a real command would hide a feature the editor actually has.
#[test]
fn a_placeholder_item_is_exactly_the_set_of_disabled_items() {
    let placeholder = MenuPlaceholder.name();
    for menu in menus() {
        for item in menu.items {
            let OwnedMenuItem::Action {
                name,
                action,
                disabled,
                ..
            } = item
            else {
                continue;
            };
            assert_eq!(
                action.name() == placeholder,
                disabled,
                "{} ⟩ {name}",
                menu.name
            );
        }
    }
}

/// The four titles the rest of this module's doc comment describes, in the
/// order Studio itself puts them — a reordering here changes what F10 lands
/// on, so it is worth being deliberate about.
#[test]
fn the_bar_holds_the_four_titles_in_order() {
    let names: Vec<String> = menus().iter().map(|menu| menu.name.to_string()).collect();
    assert_eq!(names, ["File", "Edit", "Model", "View"]);
}

/// A menu whose first entry is a separator draws a stray rule at the top,
/// and `PopupMenu::separator` silently drops a leading one, so a menu that
/// starts with one would quietly lose it rather than fail.
#[test]
fn no_menu_opens_or_closes_on_a_separator() {
    for menu in menus() {
        let separator =
            |item: Option<&OwnedMenuItem>| matches!(item, Some(OwnedMenuItem::Separator));
        assert!(
            !separator(menu.items.first()),
            "{} starts on a rule",
            menu.name
        );
        assert!(
            !separator(menu.items.last()),
            "{} ends on a rule",
            menu.name
        );
    }
}
