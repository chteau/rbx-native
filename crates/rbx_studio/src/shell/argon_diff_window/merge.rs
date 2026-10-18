//! The diff window's merge mode (`Provider::Merge`): the selected
//! instance's conflicts, each with its base, ours and theirs value and a
//! pick per side, a script's source hunks side by side, and a footer that
//! applies the merge.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::merge::{Resolution, Target};
use rbx_dom::Variant;

use super::header::button;
use super::values::{chip, class_icon, format_value, unknown, Cell, Side};
use super::ArgonDiffWindow;
use crate::class_icons::IconPack;
use crate::shell::argon_sync::DiffNode;
use crate::shell::place_merge::{target_label, MergeSession};
use crate::tokens;

/// What one column of a card shows.
enum Shown {
    Value(Cell),
    Text(String),
    Absent,
}

/// One conflict, already formatted so drawing needs no borrow.
struct Card {
    id: usize,
    label: String,
    source: bool,
    existence: bool,
    columns: [Shown; 3],
    chosen: Option<usize>,
}

const COLUMNS: [(&str, &str); 3] = [
    ("Base", "Use base"),
    ("Ours", "Use ours"),
    ("Theirs", "Use theirs"),
];

impl ArgonDiffWindow {
    /// The detail pane in merge mode for `node` (a row of `session.nodes`).
    pub(super) fn merge_detail(
        &mut self,
        session: Rc<RefCell<MergeSession>>,
        node: Option<&DiffNode>,
        pack: IconPack,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(node) = node else {
            return v_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .justify_center()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .text_color(tokens::text3())
                .child("No conflict selected");
        };
        let cards = cards(&session.borrow(), node, self.shell.read(cx));
        let unresolved = cards.iter().filter(|c| c.chosen.is_none()).count();
        let ids: Vec<usize> = cards.iter().map(|c| c.id).collect();
        let mark = session.clone();
        let header = h_flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .pt(px(16.))
            .pb(px(14.))
            .px(px(if narrow { 14. } else { 20. }))
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .flex_none()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::radius_tile())
                    .bg(tokens::field_select())
                    .child(class_icon(&node.class, pack, 16.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(tokens::WEIGHT_BOLD)
                            .text_color(tokens::text())
                            .child(node.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(tokens::font_mono())
                            .text_size(tokens::text_badge())
                            .line_height(tokens::line_badge())
                            .text_color(tokens::text3())
                            .child(node.full_path()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text2())
                    .child(format!(
                        "{} conflicts \u{b7} {unresolved} unresolved",
                        cards.len()
                    )),
            )
            .child(
                small_button(("merge-mark-resolved", 0usize), "Mark resolved", false).on_click(
                    cx.listener(move |_, _, _, cx| {
                        let mut session = mark.borrow_mut();
                        for &id in &ids {
                            let open = session
                                .merge
                                .conflicts()
                                .get(id)
                                .is_some_and(|c| c.resolution.is_none());
                            if open {
                                let _ = session.merge.resolve(id, Resolution::Ours);
                            }
                        }
                        cx.notify();
                    }),
                ),
            );
        let body = v_flex()
            .id("merge-detail-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(12.))
            .p(px(if narrow { 14. } else { 20. }))
            .children(cards.into_iter().map(|card| card_view(card, &session, cx)));
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(header)
            .child(body)
    }

    /// The footer in merge mode: unresolved count, Cancel, Apply.
    pub(super) fn merge_footer(
        &mut self,
        session: Rc<RefCell<MergeSession>>,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let unresolved = session.borrow().merge.unresolved();
        let note = if unresolved == 0 {
            "All conflicts resolved.".to_string()
        } else {
            format!("{unresolved} unresolved \u{2014} unresolved conflicts keep your version.")
        };
        let shell = self.shell.clone();
        h_flex()
            .h(px(57.))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(if narrow { 14. } else { 16. }))
            .border_t_1()
            .border_color(tokens::border())
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text3())
                    .child(note),
            )
            .child(div().flex_1())
            .child(
                button("merge-cancel", "Cancel", false)
                    .on_click(cx.listener(|_, _, window: &mut Window, _| window.remove_window())),
            )
            .child(button("merge-apply", "Apply", true).on_click(cx.listener(
                move |_, _, window: &mut Window, cx| {
                    let shell = shell.clone();
                    let session = session.clone();
                    let app: &mut App = cx;
                    app.defer(move |cx| {
                        shell.update(cx, |s, cx| s.apply_place_merge(&session.borrow(), cx));
                    });
                    window.remove_window();
                },
            )))
    }
}

fn cards(session: &MergeSession, node: &DiffNode, shell: &crate::shell::Shell) -> Vec<Card> {
    let Some(ids) = session.by_node.get(node.id) else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|&id| session.merge.conflicts().get(id))
        .map(|conflict| {
            let property = match &conflict.target {
                Target::Property(name) | Target::Attribute(name) => Some(name.as_str()),
                _ => None,
            };
            let show = |value: &Option<Variant>| match (value, property) {
                (None, _) => Shown::Absent,
                (Some(value), Some(name)) => {
                    Shown::Value(format_value(shell, &conflict.class, name, value))
                }
                (Some(Variant::String(text)), None) => Shown::Text(text.clone()),
                (Some(value), None) => Shown::Text(format!("{value:?}")),
            };
            Card {
                id: conflict.id,
                label: target_label(&conflict.target),
                source: matches!(conflict.target, Target::Source(_)),
                existence: matches!(conflict.target, Target::Existence { .. }),
                columns: [
                    show(&conflict.base),
                    show(&conflict.ours),
                    show(&conflict.theirs),
                ],
                chosen: match conflict.resolution {
                    Some(Resolution::Base) => Some(0),
                    Some(Resolution::Ours) => Some(1),
                    Some(Resolution::Theirs) => Some(2),
                    _ => None,
                },
            }
        })
        .collect()
}

fn card_view(
    card: Card,
    session: &Rc<RefCell<MergeSession>>,
    cx: &mut Context<ArgonDiffWindow>,
) -> Div {
    let Card {
        id,
        label,
        source,
        existence,
        columns,
        chosen,
    } = card;
    let row = h_flex()
        .gap(px(10.))
        .items_start()
        .children(columns.into_iter().enumerate().map(|(side, shown)| {
            let (title, pick) = COLUMNS[side];
            let content: AnyElement = match shown {
                Shown::Absent => unknown(),
                Shown::Text(text) if source => v_flex()
                    .id(("merge-source", id * 3 + side))
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .font_family(tokens::font_mono())
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text())
                    .children(text.lines().map(|line| div().child(line.to_string())))
                    .into_any_element(),
                Shown::Text(text) => chip(
                    ("merge-chip", id * 3 + side),
                    &Cell {
                        text: SharedString::from(text),
                        swatch: None,
                    },
                    Side::After,
                ),
                Shown::Value(cell) => chip(("merge-chip", id * 3 + side), &cell, Side::After),
            };
            let resolution = match side {
                0 => Resolution::Base,
                1 => Resolution::Ours,
                _ => Resolution::Theirs,
            };
            let session = session.clone();
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(8.))
                .p(px(10.))
                .rounded(tokens::radius())
                .border_1()
                .border_color(if chosen == Some(side) {
                    tokens::check_on()
                } else {
                    tokens::border()
                })
                .child(
                    div()
                        .text_size(tokens::text_xxs())
                        .line_height(tokens::line_xxs())
                        .font_weight(tokens::WEIGHT_BOLD)
                        .text_color(tokens::text3())
                        .child(title.to_uppercase()),
                )
                .child(div().flex().min_w_0().child(content))
                .when(!(existence && side == 0), |this| {
                    this.child(
                        small_button(("merge-pick", id * 3 + side), pick, chosen == Some(side))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                let _ = session.borrow_mut().merge.resolve(id, resolution.clone());
                                cx.notify();
                            })),
                    )
                })
        }));
    v_flex()
        .flex_none()
        .gap(px(10.))
        .p(px(12.))
        .rounded(tokens::radius_tile())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .child(
            div()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .text_color(tokens::text())
                .child(label),
        )
        .child(row)
}

fn small_button(id: (&'static str, usize), label: &'static str, active: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .tab_index(0)
        .flex_none()
        .h(px(26.))
        .px(px(10.))
        .items_center()
        .justify_center()
        .rounded(tokens::radius())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .font_weight(tokens::WEIGHT_SEMIBOLD)
        .cursor_pointer()
        .map(|this| {
            if active {
                this.bg(tokens::check_on()).text_color(tokens::black())
            } else {
                this.bg(tokens::dock())
                    .border_1()
                    .border_color(tokens::border2())
                    .text_color(tokens::text())
                    .hover(|this| tokens::hover_fx(this).bg(tokens::secondary_hover()))
            }
        })
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .child(label)
}
