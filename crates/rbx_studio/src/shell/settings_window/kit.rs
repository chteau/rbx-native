//! What every Settings page is built from: a section title over a card of
//! rows, each row a label and description at the left and its control at
//! the right, and the handful of controls those rows carry. Sizes are the
//! design's, in pixels, like the other windows drawn after it (the launcher,
//! the Argon Diff window); only the window itself follows the UI scale.

use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::Shell;

mod controls;

pub(super) use controls::{
    ghost_icon, readout, secondary_button, segmented, slider, toggle, OnPick,
};

/// Puts one setting back to its default.
pub(super) type Reset = Rc<dyn Fn(&mut Shell, &mut Context<Shell>)>;

/// One setting's row. `reset` is `Some` exactly when the value differs
/// from its default: that is what draws the dot and the reset icon, and
/// what "Reset page" runs.
pub(super) struct Row {
    pub(super) label: &'static str,
    pub(super) description: Option<&'static str>,
    /// Dimmed and inert: on the roadmap.
    pub(super) soon: bool,
    /// Whether the row wears its own `SOON` pill, rather than sharing its
    /// section's.
    pub(super) pill: bool,
    /// A constant's name, set in mono.
    pub(super) mono: bool,
    pub(super) reset: Option<Reset>,
    pub(super) control: AnyElement,
    /// A block under the label and control, inside the same row: a grid of
    /// checkboxes, a list.
    pub(super) below: Option<AnyElement>,
}

impl Row {
    pub(super) fn new(label: &'static str, control: impl IntoElement) -> Self {
        Row {
            label,
            description: None,
            soon: false,
            pill: false,
            mono: false,
            reset: None,
            control: control.into_any_element(),
            below: None,
        }
    }

    pub(super) fn describe(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    /// On the roadmap: dimmed, inert, and marked `SOON`.
    pub(super) fn soon(mut self) -> Self {
        self.soon = true;
        self.pill = true;
        self
    }

    /// On the roadmap, inside a block whose `SOON` pill is on its heading:
    /// dimmed and inert.
    pub(super) fn inert(mut self) -> Self {
        self.soon = true;
        self
    }

    pub(super) fn below(mut self, below: impl IntoElement) -> Self {
        self.below = Some(below.into_any_element());
        self
    }

    pub(super) fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// `reset` when `changed`, so a row only offers it off its default.
    pub(super) fn changed(
        mut self,
        changed: bool,
        reset: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
    ) -> Self {
        if changed {
            self.reset = Some(Rc::new(reset));
        }
        self
    }
}

/// A section: its title, and the card its rows sit in. `soon` pins one pill
/// on the title for a block that is on the roadmap as a whole.
pub(super) struct Section {
    pub(super) title: &'static str,
    pub(super) rows: Vec<Row>,
    /// Anything in the card above the rows: a disclosure, say.
    pub(super) head: Option<AnyElement>,
}

impl Section {
    pub(super) fn new(title: &'static str, rows: Vec<Row>) -> Self {
        Section {
            title,
            rows,
            head: None,
        }
    }
}

pub(super) fn text(size: f32, line: f32) -> Div {
    div().text_size(px(size)).line_height(px(line))
}

pub(super) fn mono(size: f32, line: f32) -> Div {
    text(size, line).font_family(tokens::FONT_FAMILY_MONO)
}

/// A Lucide glyph from the kit's catalogue, by file name.
pub(super) fn icon(name: &'static str, size: f32) -> Icon {
    Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
}

/// The `SOON` pill, with its "On the roadmap" tooltip.
pub(super) fn soon_pill(id: impl Into<ElementId>) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .flex_none()
        .h(px(16.))
        .px(px(5.))
        .items_center()
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(4.))
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(px(9.5))
        .line_height(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text3())
        .child("SOON")
        .tooltip(|window, cx| super::super::tooltip::text("On the roadmap", window, cx))
}

/// A key cap: border2 with a 2 px bottom edge, mono 10.5.
pub(super) fn key_hint(keys: &'static str) -> Div {
    h_flex()
        .flex_none()
        .h(px(18.))
        .px(px(5.))
        .items_center()
        .border_1()
        .border_b_2()
        .border_color(tokens::border2())
        .rounded(px(4.))
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(px(10.5))
        .line_height(px(12.))
        .text_color(tokens::text2())
        .child(keys)
}

/// A section title, 8 px above its card.
fn section_title(title: &'static str) -> Div {
    h_flex().h(px(18.)).items_center().child(
        text(10.5, 14.)
            .font_weight(FontWeight::BOLD)
            .text_color(tokens::text3())
            // .07em of 10.5 px.
            .child(title.to_uppercase()),
    )
}

pub(super) fn card() -> Div {
    v_flex()
        .border_1()
        .border_color(tokens::border())
        .rounded(px(8.))
        .bg(tokens::field_select())
        .overflow_hidden()
}

/// A section's rows, each after the first under a hairline. `shell` is what
/// the reset icons act on.
pub(super) fn section(index: usize, section: Section, shell: &Entity<Shell>) -> Div {
    let rows = section
        .rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| render_row((index, i), row, shell));
    v_flex()
        .gap(px(8.))
        .child(section_title(section.title))
        .child(card().children(section.head).children(rows))
}

fn render_row(id: (usize, usize), row: Row, shell: &Entity<Shell>) -> Stateful<Div> {
    let (label_color, description_color) = if row.soon {
        (tokens::text2(), tokens::text3())
    } else {
        (tokens::text(), tokens::text2())
    };
    let reset = row.reset.clone().map(|reset| {
        let shell = shell.clone();
        ghost_icon(
            ("reset", id.0 * 100 + id.1),
            "rotate-ccw",
            "Reset to default",
        )
        .on_click(move |_, _, cx| shell.update(cx, |shell, cx| reset(shell, cx)))
    });
    let control = h_flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .when(row.soon, |this| this.opacity(0.4))
        .child(row.control);
    div()
        .id(("row", id.0 * 100 + id.1))
        .relative()
        .when(id.1 > 0, |this| {
            this.border_t_1().border_color(tokens::border())
        })
        .hover(|this| this.bg(rgba(0xFFFFFF04)))
        .when(row.reset.is_some(), |this| {
            this.child(
                div()
                    .absolute()
                    .left(px(7.))
                    .top(px(19.))
                    .size(px(5.))
                    .rounded_full()
                    .bg(tokens::check_on()),
            )
        })
        .child(
            h_flex()
                .min_h(px(if row.description.is_some() { 56. } else { 44. }))
                .py(px(10.))
                .px(px(16.))
                .gap(px(16.))
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .items_center()
                                .child(
                                    text(12.5, 17.)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(label_color)
                                        .when(row.mono, |this| {
                                            this.font_family(tokens::FONT_FAMILY_MONO)
                                                .text_size(px(11.5))
                                        })
                                        .child(row.label),
                                )
                                .when(row.pill, |this| {
                                    this.child(soon_pill(("soon", id.0 * 100 + id.1)))
                                }),
                        )
                        .children(row.description.map(|description| {
                            text(11.5, 16.)
                                .text_color(description_color)
                                .child(description)
                        })),
                )
                .children(reset)
                .child(control),
        )
        .children(row.below.map(|below| {
            div()
                .pb(px(14.))
                .px(px(16.))
                .when(row.soon, |this| this.opacity(0.4))
                .child(below)
        }))
}
