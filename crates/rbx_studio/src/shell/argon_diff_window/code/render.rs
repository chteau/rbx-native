//! Drawing the code card: the rows as gpui list items, the card around
//! them scrolling sideways, its thumb, and the hunk rows' expander.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::ArgonDiffWindow;
use super::{widest, CodeRow, LineKind, CODE_ROW, COLUMN, GUTTER, GUTTER_NARROW, HUNK_ROW, MARKER};

impl ArgonDiffWindow {
    /// The card around the list: `bg`, a hairline, radius 6, scrolling
    /// sideways as a whole when a line is wider than it.
    pub(in crate::shell::argon_diff_window) fn code_card(
        &mut self,
        rows: Rc<Vec<CodeRow>>,
        unified: bool,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let gutter = if narrow { GUTTER_NARROW } else { GUTTER };
        let left = if unified {
            2. * gutter + MARKER + 4.
        } else {
            gutter + 12.
        };
        // At least the card's own width, so a short file's rows still tint
        // edge to edge; the widest line beyond that scrolls sideways.
        let card = self.width.get()
            - if narrow {
                2. * 14.
            } else {
                super::super::list::LIST_WIDTH + 2. * 20.
            }
            - 2.;
        let width = (left + widest(&rows) as f32 * COLUMN + 16.).max(card);
        let state = self.code_list.clone();
        let this = cx.entity();
        let theme = cx.theme().highlight_theme.clone();
        let colour = move |name: Option<&str>| -> Hsla {
            name.and_then(|name| theme.style(name))
                .and_then(|style| style.color)
                .unwrap_or_else(|| tokens::text().into())
        };
        let list = list(state.clone(), move |index, window, _cx| {
            let Some(row) = rows.get(index) else {
                return div().into_any_element();
            };
            match row {
                CodeRow::Line {
                    old,
                    new,
                    kind,
                    text,
                    runs,
                } => {
                    let (row_bg, cell_bg, number_ink, marker) = match kind {
                        LineKind::Context => (None, None, tokens::text3(), ("", tokens::text3())),
                        LineKind::Added => (
                            Some(tokens::diff_add_soft()),
                            Some(tokens::diff_add_gutter()),
                            tokens::text2(),
                            ("+", tokens::diff_add()),
                        ),
                        LineKind::Removed => (
                            Some(tokens::diff_remove_soft()),
                            Some(tokens::diff_remove_gutter()),
                            tokens::text2(),
                            ("\u{2212}", tokens::text_error()),
                        ),
                    };
                    let number = |value: Option<usize>| {
                        div()
                            .w(px(gutter))
                            .h_full()
                            .flex_none()
                            .pr(px(8.))
                            .text_right()
                            .text_size(px(11.))
                            .text_color(number_ink)
                            .when_some(cell_bg, |this, bg| this.bg(bg))
                            .child(value.map(|n| n.to_string()).unwrap_or_default())
                    };
                    let style = TextStyle {
                        font_family: tokens::FONT_FAMILY_MONO.into(),
                        font_size: px(12.).into(),
                        line_height: px(CODE_ROW).into(),
                        color: tokens::text().into(),
                        font_features: FontFeatures::disable_ligatures(),
                        ..window.text_style()
                    };
                    let highlights: Vec<(Range<usize>, HighlightStyle)> = runs
                        .iter()
                        .map(|(range, name)| {
                            (
                                range.clone(),
                                HighlightStyle {
                                    color: Some(colour(*name)),
                                    ..Default::default()
                                },
                            )
                        })
                        .collect();
                    let code =
                        StyledText::new(text.clone()).with_default_highlights(&style, highlights);
                    h_flex()
                        .h(px(CODE_ROW))
                        .w_full()
                        .items_center()
                        .when_some(row_bg, |this, bg| this.bg(bg))
                        .when(unified, |this| {
                            this.child(number(*old)).child(number(*new)).child(
                                div()
                                    .w(px(MARKER))
                                    .flex_none()
                                    .text_center()
                                    .text_color(marker.1)
                                    .child(marker.0),
                            )
                        })
                        .when(!unified, |this| this.child(number(*old)))
                        .child(
                            div()
                                .flex_none()
                                .pl(px(if unified { 4. } else { 12. }))
                                .whitespace_nowrap()
                                .child(code),
                        )
                        .into_any_element()
                }
                CodeRow::Hunk {
                    key,
                    hidden,
                    header,
                    function,
                } => {
                    let expander = expander(*key, *hidden, left - 4., this.clone());
                    h_flex()
                        .h(px(HUNK_ROW))
                        .w_full()
                        .items_center()
                        .bg(tokens::field_select())
                        .child(expander)
                        .child(
                            h_flex()
                                .pl(px(4.))
                                .whitespace_nowrap()
                                .font_family(tokens::FONT_FAMILY_MONO)
                                .text_size(px(11.))
                                .text_color(tokens::text3())
                                .child(format!("{header}  "))
                                .children(
                                    function
                                        .clone()
                                        .map(|name| div().text_color(tokens::text2()).child(name)),
                                ),
                        )
                        .into_any_element()
                }
                CodeRow::Tail { key, hidden } => h_flex()
                    .h(px(HUNK_ROW))
                    .w_full()
                    .items_center()
                    .bg(tokens::field_select())
                    .child(expander(*key, *hidden, left - 4., this.clone()))
                    .child(
                        div()
                            .pl(px(4.))
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .text_size(px(11.))
                            .text_color(tokens::text3())
                            .child(format!("{hidden} unchanged lines")),
                    )
                    .into_any_element(),
                CodeRow::Limit { more, limit } => h_flex()
                    .h(px(HUNK_ROW))
                    .w_full()
                    .items_center()
                    .bg(tokens::field_select())
                    .child(div().w(px(left)).flex_none())
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text3())
                            .child(format!(
                                "And {more} more lines \u{b7} Diff Lines Limit is {limit}"
                            )),
                    )
                    .into_any_element(),
            }
        });
        div()
            .id("argon-diff-code")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .min_w_full()
            .rounded(tokens::RADIUS_TILE)
            .border_1()
            .border_color(tokens::border())
            .bg(tokens::black())
            .overflow_hidden()
            .child(
                div()
                    .id("argon-diff-code-scroll")
                    .size_full()
                    .flex()
                    .overflow_x_scroll()
                    .child(list.flex_none().w(px(width)).h_full()),
            )
            // The thumb stays in view, as the docks' do: 4 wide, 2 in from
            // the right, a sibling of the scroll area so nothing moves it.
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .bottom(px(4.))
                    .left_0()
                    .right(px(2.))
                    .child(
                        gpui_kit::base::Scrollbar::new(&state)
                            .id("argon-diff-code-thumb")
                            .axis(Axis::Vertical)
                            .mode(gpui_kit::base::ScrollbarMode::Always)
                            .styles(|styles| {
                                styles.thumb(|thumb| {
                                    thumb
                                        .bg(tokens::border2())
                                        .width(px(4.))
                                        .inset(px(0.))
                                        .radius(px(2.))
                                })
                            })
                            .viewport_from_layout(),
                    ),
            )
            .into_any_element()
    }
}

/// The hunk row's left part: as wide as the gutters and the marker, a
/// chevrons-up-down 12 that shows the hidden lines; empty when nothing is
/// hidden.
fn expander(key: usize, hidden: usize, width: f32, this: Entity<ArgonDiffWindow>) -> AnyElement {
    if hidden == 0 {
        return div().w(px(width)).flex_none().into_any_element();
    }
    let label = SharedString::from(format!("Show {hidden} unchanged lines"));
    div()
        .id(("argon-diff-expand", key))
        .w(px(width))
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(tokens::text2())
        .hover(|this| {
            this.bg(tokens::secondary_hover())
                .text_color(tokens::text())
        })
        .tooltip(move |window, cx| crate::shell::tooltip::text(label.clone(), window, cx))
        .on_click(move |_, _, cx| {
            this.update(cx, |window, cx| {
                window.expand_hunk(key);
                cx.notify();
            });
        })
        .child(Icon::new(IconName::ChevronsUpDown).size(px(12.)))
        .into_any_element()
}
