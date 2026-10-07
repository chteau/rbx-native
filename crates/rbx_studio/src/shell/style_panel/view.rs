//! The Stylesheet tab's lines, in the dock's tokens: a sheet is a bold
//! section header, a rule a chevron and its selector inside it, and an open
//! rule's properties name-and-value rows under it. The state and the commit
//! path live in the parent module; this file only draws them.

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::super::chrome;
use super::super::rows::text_field;
use super::super::tooltip;
use super::{Shell, StyleEdits};
use crate::style_editor;
use crate::tokens;

/// How far a line sits in: one chevron-sized step per level, so a rule's
/// chevron lands under its sheet's name and a property's name under its
/// rule's selector.
fn indent(level: usize) -> Pixels {
    tokens::hit_target() * level as f32
}

pub(super) fn flip(set: &mut HashSet<Ref>, referent: Ref) {
    if !set.remove(&referent) {
        set.insert(referent);
    }
}

/// Clicking a button inside a line must not also select the line: the link
/// button reads the selection, and a chevron that selected its rule would
/// open it a moment before toggling it shut again.
fn keep_selection(element: Stateful<Div>) -> Stateful<Div> {
    element.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

impl Shell {
    /// The strip above the list: how many sheets there are, and the
    /// panel's one primary action.
    pub(super) fn style_toolbar(&mut self, sheets: usize, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .flex_none()
            .items_center()
            .gap(tokens::label_gap())
            .px(px(8.))
            .py(tokens::row_padding())
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_muted())
                    .child(match sheets {
                        1 => SharedString::from("1 style sheet"),
                        n => format!("{n} style sheets").into(),
                    }),
            )
            .child(self.new_sheet_button(cx))
            .into_any_element()
    }

    /// What the tab shows before the place has a sheet: what one is for,
    /// and the button that makes one — here instead of in a toolbar.
    pub(super) fn style_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(tokens::label_gap())
            .p(tokens::panel_padding())
            .child(
                div()
                    .text_color(tokens::text_muted())
                    .child(Icon::new(IconName::Paintbrush).size(tokens::text_lg())),
            )
            .child(
                div()
                    .text_size(tokens::text_md())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text_strong())
                    .child("No style sheets yet"),
            )
            .child(
                div()
                    .text_center()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text_muted())
                    .child(
                        "A StyleSheet holds rules that restyle every matching GUI object at once.",
                    ),
            )
            .child(
                div()
                    .pt(tokens::label_gap())
                    .child(self.new_sheet_button(cx)),
            )
            .into_any_element()
    }

    fn new_sheet_button(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        chrome::button("style-new-sheet", "New StyleSheet", false)
            .track_focus(&self.tab_order.claim(cx))
            .on_click(cx.listener(|shell, _, _, cx| {
                let mut sheet = None;
                shell.apply_style_edit(
                    |dom, _| {
                        sheet = Some(style_editor::add_sheet(dom));
                        Ok(())
                    },
                    cx,
                );
                if let Some(sheet) = sheet {
                    shell.select(sheet, cx);
                }
            }))
    }

    /// The last rejected edit, pinned above the list so it stays in view
    /// however far the list is scrolled.
    pub(super) fn style_error(&self) -> Option<AnyElement> {
        let message = self.style_edits.error.clone()?;
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .items_center()
                .gap(tokens::label_gap())
                .px(px(8.))
                .py(tokens::row_padding())
                .bg(tokens::error_soft())
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text_error())
                .child(Icon::new(IconName::CircleAlert).size(tokens::text_sm()))
                .child(div().flex_1().min_w_0().child(SharedString::from(message)))
                .into_any_element(),
        )
    }

    /// One line of the list. A mouse-down selects its instance the way an
    /// Explorer click does, so the Properties panel follows — which is
    /// where a rule's `Name` and anything else this tab omits is edited.
    pub(super) fn style_line(
        &self,
        id: impl Into<ElementId>,
        referent: Ref,
        selected: bool,
        level: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        h_flex()
            .id(id)
            .w_full()
            .flex_none()
            .min_h(tokens::row_height())
            .items_center()
            .gap(tokens::label_gap())
            .pl(indent(level))
            .pr(tokens::row_padding())
            .rounded(tokens::radius_row())
            .map(|this| match selected {
                true => this.bg(tokens::selection()),
                false => this.hover(|this| this.bg(tokens::hover())),
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| shell.select(referent, cx)),
            )
    }

    pub(super) fn style_chevron(
        &mut self,
        id: impl Into<ElementId>,
        open: bool,
        toggle: impl Fn(&mut StyleEdits) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (icon, label) = match open {
            true => (IconName::ChevronDown, "Collapse"),
            false => (IconName::ChevronRight, "Expand"),
        };
        keep_selection(chrome::icon_button(id, icon, label))
            .track_focus(&self.tab_order.claim(cx))
            .on_click(cx.listener(move |shell, _, _, cx| {
                toggle(&mut shell.style_edits);
                cx.notify();
            }))
    }

    /// A text field in the window's Tab order.
    pub(super) fn style_input(
        &mut self,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> Div {
        self.tab_order.register(&input.read(cx).focus_handle(cx));
        text_field(input, self.tab_order.next(), cx)
    }

    /// A sheet's header: its name, where it lives, and the two things
    /// Studio's editor offers on a sheet (`ui/styling/editor.md`) — a new
    /// rule, and a `StyleLink` onto the selected `ScreenGui`. The link stays
    /// visible but disabled until there is one to link to.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn style_sheet_header(
        &mut self,
        sheet: Ref,
        name: &str,
        parent: &str,
        open: bool,
        selected: bool,
        link_target: Option<Ref>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = sheet.value() as usize;
        let chevron = self.style_chevron(
            ("style-sheet-toggle", id),
            open,
            move |edits| flip(&mut edits.closed_sheets, sheet),
            cx,
        );
        let link = match link_target {
            Some(target) => keep_selection(chrome::icon_button(
                ("style-link", id),
                IconName::Link2,
                "Link to the selected ScreenGui",
            ))
            .track_focus(&self.tab_order.claim(cx))
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.apply_style_edit(
                    move |dom, database| {
                        style_editor::add_link(dom, database, target, sheet).map(|_| ())
                    },
                    cx,
                );
            })),
            None => div()
                .id(("style-link", id))
                .flex_none()
                .size(tokens::hit_target())
                .flex()
                .items_center()
                .justify_center()
                .cursor_not_allowed()
                .text_color(tokens::text_disabled())
                .tooltip(|window, cx| {
                    tooltip::text("Select a ScreenGui to link this sheet to it", window, cx)
                })
                .child(Icon::new(IconName::Link2).size(tokens::text_md())),
        };
        let add_rule = keep_selection(chrome::icon_button(
            ("style-add-rule", id),
            IconName::Plus,
            "Add rule",
        ))
        .track_focus(&self.tab_order.claim(cx))
        .on_click(cx.listener(move |shell, _, _, cx| {
            let mut rule = None;
            shell.apply_style_edit(
                |dom, _| {
                    rule = Some(style_editor::add_rule(dom, sheet));
                    Ok(())
                },
                cx,
            );
            // Selecting the new rule opens it (see `follow_selection`),
            // so its selector field is on screen ready to fill in.
            if let Some(rule) = rule {
                shell.select(rule, cx);
            }
        }));

        // Muted glyphs fall below 3:1 on the selection fill, so a selected
        // header draws its buttons in full text.
        let on_selection = |button: Stateful<Div>| match selected {
            true => button.text_color(tokens::text_full()),
            false => button,
        };
        let (chevron, add_rule) = (on_selection(chevron), on_selection(add_rule));
        let link = match link_target {
            Some(_) => on_selection(link),
            None => link,
        };

        self.style_line(("style-sheet", id), sheet, selected, 0, cx)
            .child(chevron)
            .child(
                div()
                    .flex_none()
                    .max_w_1_2()
                    .truncate()
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text_full())
                    .child(SharedString::from(name.to_owned())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_muted())
                    .child(SharedString::from(parent.to_owned())),
            )
            .child(link)
            .child(add_rule)
            .into_any_element()
    }

    /// Under an open sheet with nothing in it.
    pub(super) fn style_sheet_empty(&self) -> AnyElement {
        div()
            .w_full()
            .min_h(tokens::row_height())
            .flex()
            .items_center()
            .pl(indent(1))
            .text_size(tokens::text_sm())
            .text_color(tokens::text_muted())
            .child("No rules yet. Add one with +.")
            .into_any_element()
    }

    /// A derive is the sheet it names and the priority that orders it, both
    /// edited in the Properties panel — the line only says what it pulls in.
    pub(super) fn derive_line(
        &self,
        derive: Ref,
        sheet: &str,
        priority: i32,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.style_line(
            ("style-derive", derive.value() as usize),
            derive,
            selected,
            1,
            cx,
        )
        .text_size(tokens::text_sm())
        .text_color(tokens::text_muted())
        .child(
            div()
                .flex_none()
                .size(tokens::hit_target())
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(IconName::GitBranch).size(tokens::text_sm())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(format!("Derives {sheet}"))),
        )
        .child(
            div()
                .flex_none()
                .child(SharedString::from(format!("Priority {priority}"))),
        )
        .into_any_element()
    }
}
