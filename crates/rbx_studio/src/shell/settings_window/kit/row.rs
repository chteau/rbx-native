//! A setting's row and the section it sits in: label and description at
//! the left, the control at the right, the changed-from-default dot and
//! reset between them.

use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::super::Shell;
use super::{card, ghost_icon, key_hint, soon_pill, text};

/// Puts one setting back to its default.
pub(in crate::shell::settings_window) type Reset = Rc<dyn Fn(&mut Shell, &mut Context<Shell>)>;

/// One setting's row. `reset` is `Some` exactly when the value differs
/// from its default: that is what draws the dot and the reset icon, and
/// what "Reset page" runs.
pub(in crate::shell::settings_window) struct Row {
    pub(in crate::shell::settings_window) label: &'static str,
    pub(in crate::shell::settings_window) description: Option<SharedString>,
    /// The description is a path or a name, set in mono.
    pub(in crate::shell::settings_window) description_mono: bool,
    /// Key caps after the description: the shortcut that does the same.
    pub(in crate::shell::settings_window) keys: &'static [&'static str],
    /// A setting that only means something under the one above it.
    pub(in crate::shell::settings_window) indent: bool,
    /// Dimmed and inert: on the roadmap.
    pub(in crate::shell::settings_window) soon: bool,
    /// A tag after the label: which Argon level a value is set at.
    pub(in crate::shell::settings_window) chip: Option<SharedString>,
    /// Whether the row wears its own `SOON` pill, rather than sharing its
    /// section's.
    pub(in crate::shell::settings_window) pill: bool,
    /// A constant's name, set in mono.
    pub(in crate::shell::settings_window) mono: bool,
    /// The control draws its own fade (a still toggle or slider), so the
    /// row doesn't fade it again.
    pub(in crate::shell::settings_window) self_faded: bool,
    pub(in crate::shell::settings_window) reset: Option<Reset>,
    pub(in crate::shell::settings_window) control: AnyElement,
    /// A block under the label and control, inside the same row: a grid of
    /// checkboxes, a list.
    pub(in crate::shell::settings_window) below: Option<AnyElement>,
}

impl Row {
    pub(in crate::shell::settings_window) fn new(
        label: &'static str,
        control: impl IntoElement,
    ) -> Self {
        Row {
            label,
            description: None,
            description_mono: false,
            keys: &[],
            indent: false,
            soon: false,
            chip: None,
            pill: false,
            mono: false,
            self_faded: false,
            reset: None,
            control: control.into_any_element(),
            below: None,
        }
    }

    pub(in crate::shell::settings_window) fn chip(mut self, chip: impl Into<SharedString>) -> Self {
        self.chip = Some(chip.into());
        self
    }

    pub(in crate::shell::settings_window) fn describe(
        mut self,
        description: impl Into<SharedString>,
    ) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(in crate::shell::settings_window) fn keys(mut self, keys: &'static [&'static str]) -> Self {
        self.keys = keys;
        self
    }

    pub(in crate::shell::settings_window) fn describe_mono(
        mut self,
        description: impl Into<SharedString>,
    ) -> Self {
        self.description_mono = true;
        self.describe(description)
    }

    pub(in crate::shell::settings_window) fn indent(mut self) -> Self {
        self.indent = true;
        self
    }

    /// On the roadmap: dimmed, inert, and marked `SOON`.
    pub(in crate::shell::settings_window) fn soon(mut self) -> Self {
        self.soon = true;
        self.pill = true;
        self
    }

    /// On the roadmap, inside a block whose `SOON` pill is on its heading:
    /// dimmed and inert.
    pub(in crate::shell::settings_window) fn inert(mut self) -> Self {
        self.soon = true;
        self
    }

    pub(in crate::shell::settings_window) fn below(mut self, below: impl IntoElement) -> Self {
        self.below = Some(below.into_any_element());
        self
    }

    /// [`Row::soon`] for a control that fades itself.
    pub(in crate::shell::settings_window) fn soon_faded(mut self) -> Self {
        self.self_faded = true;
        self.soon()
    }

    pub(in crate::shell::settings_window) fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// `reset` when `changed`, so a row only offers it off its default.
    pub(in crate::shell::settings_window) fn changed(
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
pub(in crate::shell::settings_window) struct Section {
    pub(in crate::shell::settings_window) title: &'static str,
    pub(in crate::shell::settings_window) rows: Vec<Row>,
    /// Anything in the card above the rows: a disclosure, say.
    pub(in crate::shell::settings_window) head: Option<AnyElement>,
    /// Resets for what the section shows outside its rows, for Reset page.
    pub(in crate::shell::settings_window) resets: Vec<Reset>,
    /// Words a search finds the section's head by, when the head is the
    /// setting (the accent card).
    pub(in crate::shell::settings_window) keywords: &'static [&'static str],
}

impl Section {
    pub(in crate::shell::settings_window) fn new(title: &'static str, rows: Vec<Row>) -> Self {
        Section {
            title,
            rows,
            head: None,
            resets: Vec::new(),
            keywords: &[],
        }
    }
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

/// A section's rows, each after the first under a hairline. `shell` is what
/// the reset icons act on.
pub(in crate::shell::settings_window) fn section(
    index: usize,
    section: Section,
    shell: &Entity<Shell>,
) -> Div {
    let title = section.title;
    v_flex()
        .gap(px(8.))
        .child(section_title(title))
        .child(section_card(index, section, shell, None))
}

/// A section's card alone; `query` marks where each row matches a search.
pub(in crate::shell::settings_window) fn section_card(
    index: usize,
    section: Section,
    shell: &Entity<Shell>,
    query: Option<&str>,
) -> Div {
    let rows = section
        .rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| render_row((index, i), row, shell, query));
    card().children(section.head).children(rows)
}

/// `text` with every case-insensitive match of `query` marked in the
/// accent on its soft wash.
fn marked(text: SharedString, query: Option<&str>) -> AnyElement {
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return text.into_any_element();
    };
    let lower = text.to_lowercase();
    let needle = query.to_lowercase();
    // Lowercasing can change a string's byte length (outside ASCII), and
    // then the offsets found in one don't fit the other.
    if lower.len() != text.len() {
        return text.into_any_element();
    }
    let style = HighlightStyle {
        color: Some(tokens::check_on().into()),
        background_color: Some(tokens::accent_soft().into()),
        ..Default::default()
    };
    let ranges: Vec<_> = lower
        .match_indices(&needle)
        .map(|(at, found)| (at..at + found.len(), style))
        .collect();
    StyledText::new(text)
        .with_highlights(ranges)
        .into_any_element()
}

fn render_row(
    id: (usize, usize),
    row: Row,
    shell: &Entity<Shell>,
    query: Option<&str>,
) -> Stateful<Div> {
    let label_matched = query.is_some_and(|q| row.label.to_lowercase().contains(&q.to_lowercase()));
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
        .when(row.soon && !row.self_faded, |this| this.opacity(0.4))
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
                .pr(px(16.))
                .pl(px(if row.indent { 40. } else { 16. }))
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
                                        .child(marked(row.label.into(), query)),
                                )
                                .children(row.chip.map(|chip| {
                                    text(10.5, 14.)
                                        .h(px(18.))
                                        .px(px(6.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(4.))
                                        .bg(tokens::accent_soft())
                                        .text_color(tokens::check_on())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(chip)
                                }))
                                .when(row.pill, |this| {
                                    this.child(soon_pill(("soon", id.0 * 100 + id.1)))
                                }),
                        )
                        .children(row.description.map(|description| {
                            let line = text(11.5, 16.)
                                .text_color(description_color)
                                .when(row.description_mono, |this| {
                                    this.font_family(tokens::FONT_FAMILY_MONO)
                                        .text_size(px(11.))
                                })
                                // Marked only where the label didn't
                                // match: one mark per row says why it's here.
                                .child(marked(description, query.filter(|_| !label_matched)));
                            if row.keys.is_empty() {
                                line.into_any_element()
                            } else {
                                // The key caps follow the sentence on its line.
                                h_flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(line)
                                    .children(row.keys.iter().map(|keys| key_hint(keys)))
                                    .into_any_element()
                            }
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
