use gpui_kit::SharedString;
use rbx_dom::Ref;

use super::DraggedInstances;

fn name() -> SharedString {
    SharedString::from("Part")
}

#[test]
fn a_row_outside_the_selection_drags_only_itself() {
    let selected = [Ref::new(1), Ref::new(2)];
    let dragged = DraggedInstances::new(&selected, Ref::new(3), &name());

    assert_eq!(dragged.references, vec![Ref::new(3)]);
    assert_eq!(dragged.label, name());
}

#[test]
fn a_row_inside_the_selection_drags_the_whole_selection() {
    let selected = [Ref::new(1), Ref::new(2), Ref::new(3)];
    let dragged = DraggedInstances::new(&selected, Ref::new(2), &name());

    assert_eq!(dragged.references, selected.to_vec());
    assert_eq!(dragged.label, SharedString::from("3 instances"));
}

#[test]
fn a_lone_selected_row_is_labelled_by_name_not_by_count() {
    let selected = [Ref::new(7)];
    let dragged = DraggedInstances::new(&selected, Ref::new(7), &name());

    assert_eq!(dragged.references, vec![Ref::new(7)]);
    assert_eq!(dragged.label, name());
}

#[test]
fn nothing_selected_still_drags_the_pressed_row() {
    let dragged = DraggedInstances::new(&[], Ref::new(5), &name());

    assert_eq!(dragged.references, vec![Ref::new(5)]);
}
