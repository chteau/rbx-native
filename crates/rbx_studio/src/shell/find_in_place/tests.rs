use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{
    div, AppContext, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, Styled, TestAppContext, Window,
};

/// A Find in Place field inside a root that counts the Ctrl+Z presses
/// reaching it, as `Shell::handle_shell_key` sees them.
struct Field {
    input: Entity<InputState>,
    reached: Rc<Cell<usize>>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let reached = self.reached.clone();
        div()
            .size_full()
            .on_key_down(move |event: &KeyDownEvent, _, _| {
                if event.keystroke.key == "z" {
                    reached.set(reached.get() + 1);
                }
            })
            .child(Input::new(&self.input))
    }
}

#[gpui_kit::test]
fn undo_in_a_field_with_nothing_to_undo_reaches_the_place(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let reached = Rc::new(Cell::new(0));
    let (view, cx) = cx.add_window_view({
        let reached = reached.clone();
        move |window, cx| Field {
            input: cx.new(|cx| InputState::new(window, cx)),
            reached,
        }
    });
    let input = view.read_with(cx, |field, _| field.input.clone());
    cx.update(|window, cx| input.update(cx, |state, cx| state.focus(window, cx)));

    cx.simulate_keystrokes("secondary-z");
    assert_eq!(reached.get(), 1, "an untouched field passes Undo on");

    // Typing is the field's own to take back first, then Undo goes on.
    cx.simulate_input("ab");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(reached.get(), 1);
    for _ in 0..4 {
        if reached.get() == 2 {
            break;
        }
        cx.simulate_keystrokes("secondary-z");
    }
    assert_eq!(reached.get(), 2);
    assert_eq!(
        input.read_with(cx, |state, _| state.value().to_string()),
        ""
    );
}
