//! One command's row: its name, a chip per binding, add and reset.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::launcher::ui;
use crate::shortcuts::{self, Command, Spec};
use crate::tokens;

use super::ShortcutsWindow;

fn chip(id: ElementId, active: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .gap_1()
        .px_2()
        .h(px(22.))
        .rounded(px(4.))
        .border_1()
        .border_color(if active {
            tokens::accent_line()
        } else {
            tokens::border()
        })
        .bg(if active {
            tokens::accent_soft()
        } else {
            tokens::field_select()
        })
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|style| style.bg(tokens::secondary_hover()))
}

const RECORDING: &str = "Press a key…";

/// `recording` is `Some(slot)` while this row waits for a key (`None` slot:
/// adding); `conflicts` names what the last change's key also runs.
pub(super) fn row(
    spec: &'static Spec,
    keys: Vec<Keystroke>,
    is_default: bool,
    recording: Option<Option<usize>>,
    conflicts: Option<&[Command]>,
    cx: &mut Context<ShortcutsWindow>,
) -> AnyElement {
    let command = spec.command;
    let mut chips = h_flex().flex_wrap().gap_1();
    for (slot, key) in keys.iter().enumerate() {
        let active = recording == Some(Some(slot));
        let label: SharedString = if active {
            RECORDING.into()
        } else {
            shortcuts::display(key).into()
        };
        let remove = div()
            .id(ElementId::Name(format!("remove-{}-{slot}", spec.id).into()))
            .px_0p5()
            .text_color(tokens::text3())
            .hover(|style| style.text_color(tokens::text()))
            .child("×")
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.remove(command, slot, cx);
            }));
        chips = chips.child(
            chip(
                ElementId::Name(format!("key-{}-{slot}", spec.id).into()),
                active,
            )
            .child(label)
            .child(remove)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.start_recording(command, Some(slot), window, cx)
            })),
        );
    }
    let adding = recording == Some(None);
    chips = chips.child(
        chip(ElementId::Name(format!("add-{}", spec.id).into()), adding)
            .text_color(if adding {
                tokens::text()
            } else {
                tokens::text2()
            })
            .child(if adding { RECORDING } else { "+" })
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.start_recording(command, None, window, cx)
            })),
    );
    let line = h_flex()
        .gap_3()
        .py_1p5()
        .items_center()
        .child(div().w(px(220.)).flex_none().truncate().child(spec.name))
        .child(div().flex_1().child(chips))
        .when(!is_default, |line| {
            line.child(
                ui::button(
                    ElementId::Name(format!("reset-{}", spec.id).into()),
                    "Reset",
                    ui::Weight::Ghost,
                    true,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.reset(command, cx))),
            )
        });
    let warning = conflicts.filter(|c| !c.is_empty()).map(|others| {
        let names: Vec<String> = others
            .iter()
            .map(|other| {
                let spec = other.spec();
                format!("{} ({})", spec.name, spec.category.label())
            })
            .collect();
        div()
            .pl(px(232.))
            .pb_1()
            .text_size(px(12.))
            .text_color(tokens::text_error())
            .child(format!("Also runs: {}", names.join(", ")))
    });
    v_flex()
        .border_b_1()
        .border_color(tokens::border2())
        .child(line)
        .children(warning)
        .into_any_element()
}
