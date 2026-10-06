//! The decidable halves of the Explorer's row affordances: which context
//! menu rows are live for a given row and selection, and which rows can be
//! renamed at all, plus how focus is handed back when the menu or a name
//! box closes, under GPUI's headless window. Everything else here needs a
//! live window (see
//! `shell::group`'s own tests for why this codebase's `Shell` methods stop
//! being unit-testable past their `push_history`/`take_changes` pair).

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::give_focus_back;
use super::menu::availability;
use super::rename::renameable;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// A `Workspace` with a `Model` holding one `Part`, plus a loose `Part`
/// beside the model — enough for every row of the menu to have both answers
/// somewhere in it.
fn place() -> (WeakDom, Ref, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Model", Some(workspace));
    let inside = dom.new_instance("Part", "Part", Some(model));
    let loose = dom.new_instance("Part", "Part", Some(workspace));
    (dom, workspace, model, inside, loose)
}

#[test]
fn an_ordinary_row_offers_every_clipboard_action() {
    let (dom, _, _, _, loose) = place();
    let live = availability(&dom, &database(), &[loose], true, loose);
    assert!(live.clipboard);
    assert!(live.rename);
    assert!(live.delete);
    // A lone part has a parent to be wrapped into, but is no `Model`.
    assert!(live.group);
    assert!(!live.ungroup);
    assert!(live.change_class);
}

#[test]
fn a_service_row_greys_what_it_cannot_do() {
    let (dom, workspace, _, _, _) = place();
    let live = availability(&dom, &database(), &[workspace], true, workspace);
    // `clipboard::copyable` drops a service, and so does `group`'s own
    // common-parent rule.
    assert!(!live.clipboard);
    assert!(!live.group);
    assert!(!live.rename);
    // A place whose `Workspace` has been deleted is not a place anything
    // can open again, so Delete is refused for the same reason.
    assert!(!live.delete);
    assert!(!live.change_class);
}

// One convertible instance is enough: the command converts it and says what
// it left alone.
#[test]
fn a_mixed_selection_with_a_service_still_offers_change_class() {
    let (dom, workspace, _, _, loose) = place();
    let live = availability(&dom, &database(), &[workspace, loose], true, workspace);
    assert!(live.change_class);
}

#[test]
fn a_model_row_offers_ungroup() {
    let (dom, _, model, _, _) = place();
    let live = availability(&dom, &database(), &[model], true, model);
    assert!(live.ungroup);
}

#[test]
fn paste_is_live_exactly_while_the_clipboard_holds_something() {
    let (dom, _, _, _, loose) = place();
    let database = database();
    assert!(!availability(&dom, &database, &[loose], true, loose).paste);
    assert!(availability(&dom, &database, &[loose], false, loose).paste);
}

#[test]
fn a_row_whose_instance_is_gone_offers_no_delete() {
    let (mut dom, _, _, inside, loose) = place();
    dom.remove(inside);
    let live = availability(&dom, &database(), &[loose], true, inside);
    assert!(!live.delete);
    assert!(!live.rename);
}

#[test]
fn a_service_cannot_be_renamed_but_an_instance_under_it_can() {
    let (dom, workspace, _, _, loose) = place();
    let database = database();
    assert!(!renameable(&dom, &database, workspace));
    assert!(renameable(&dom, &database, loose));
}

fn key(key: &str, shift: bool, control: bool) -> gpui_kit::Keystroke {
    gpui_kit::Keystroke {
        modifiers: gpui_kit::Modifiers {
            shift,
            control,
            ..Default::default()
        },
        key: key.into(),
        key_char: None,
    }
}

/// Shift+F10 and the Menu key open the row menu; plain F10 is the menu
/// bar's (see `shell::save`), and a modified Menu key is somebody else's.
#[test]
fn shift_f10_and_the_menu_key_open_the_row_menu_and_nothing_else_does() {
    use super::menu::opens_row_menu;
    assert!(opens_row_menu(&key("f10", true, false)));
    assert!(opens_row_menu(&key("menu", false, false)));
    assert!(!opens_row_menu(&key("f10", false, false)));
    assert!(!opens_row_menu(&key("f10", true, true)));
    assert!(!opens_row_menu(&key("menu", true, false)));
    assert!(!opens_row_menu(&key("down", false, false)));
}

/// The shared close path of the row menu, the picker and the name box: focus
/// goes back to the tree when it was still in the closing box or had gone
/// nowhere, and stays put when a click moved it somewhere else on purpose.
#[gpui_kit::test]
fn closing_hands_focus_back_only_when_nothing_else_took_it(cx: &mut gpui_kit::TestAppContext) {
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let tree = cx.focus_handle();
        let name_box = cx.focus_handle();
        let viewport = cx.focus_handle();

        // Enter or Escape: the box still has focus.
        name_box.focus(window, cx);
        give_focus_back(Some(tree.clone()), &name_box, window, cx);
        assert!(tree.is_focused(window));

        // A click into the viewport blurred the box first.
        name_box.focus(window, cx);
        viewport.focus(window, cx);
        give_focus_back(Some(tree.clone()), &name_box, window, cx);
        assert!(viewport.is_focused(window));

        // The box's element left the tree and took focus with it.
        window.blur(cx);
        give_focus_back(Some(tree.clone()), &name_box, window, cx);
        assert!(tree.is_focused(window));
    });
}

/// WCAG 2.5.5: the Delete row is 44 tall; the other rows keep the menu's 24.
#[gpui_kit::test]
fn the_delete_row_lays_out_at_least_44_tall(cx: &mut gpui_kit::TestAppContext) {
    use super::menu::chrome;
    use gpui_kit::assets::IconName;
    crate::probe::assert_primary(cx, "row-menu-delete", 200., |_, _| {
        chrome("delete", IconName::Trash, "Delete", true)
    });
    let cut = crate::probe::size_of(cx, "row-menu-cut", 200., |_, _| {
        chrome("cut", IconName::Scissors, "Cut", true)
    });
    assert!(f32::from(cut.height) < 44.);
}
