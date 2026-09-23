//! The list pane: section headers and change rows, an addition's subtree
//! under it — or, when the window is narrow, the picker bar that stands
//! in for it, whose menu is the same list.

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::shell::argon_sync::{ChangeKind, DiffNode};
use crate::tokens;

use super::model::{self, Row};
use super::values::{class_icon, dot, kind_colour, plus_minus};
use super::ArgonDiffWindow;

mod picker;

pub(super) const LIST_WIDTH: f32 = 320.;

impl ArgonDiffWindow {
    /// The 320 px pane, its hairline at the right, rows 2 apart under 8 px
    /// of padding, scrolling on its own.
    pub(super) fn list_pane(
        &mut self,
        nodes: &[DiffNode],
        rows: &[Row],
        pack: IconPack,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let query = self.query_text(cx);
        let body: AnyElement = if rows.is_empty() {
            div()
                .p(px(16.))
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .text_color(tokens::text2())
                .text_center()
                .child(format!("No changes match \u{201c}{query}\u{201d}"))
                .into_any_element()
        } else {
            v_flex()
                .gap(px(2.))
                .children(rows.iter().map(|row| self.list_row(nodes, row, pack, cx)))
                .into_any_element()
        };
        v_flex()
            .id("argon-diff-list")
            .w(px(LIST_WIDTH))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(tokens::border())
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .child(div().p(px(8.)).child(body))
            .vertical_scrollbar(&self.list_scroll)
    }

    fn list_row(
        &mut self,
        nodes: &[DiffNode],
        row: &Row,
        pack: IconPack,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match *row {
            Row::Section { kind, count, open } => self.section_row(kind, count, open, cx),
            Row::Node {
                id,
                depth,
                has_children,
                open,
            } => match model::find(nodes, id) {
                Some(node) => self.node_row(node, depth, has_children, open, pack, cx),
                None => div().into_any_element(),
            },
        }
    }

    /// h28, `padding:0 8px`, `gap:6px`: chevron, dot, label, count.
    fn section_row(
        &mut self,
        kind: ChangeKind,
        count: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = match kind {
            ChangeKind::Added => "ADDITIONS",
            ChangeKind::Updated => "UPDATES",
            ChangeKind::Removed => "REMOVALS",
        };
        h_flex()
            .id(SharedString::from(format!("argon-diff-section-{label}")))
            .h(px(28.))
            .flex_none()
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .hover(|this| this.bg(tokens::hover_subtle()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.collapsed.remove(&kind) {
                    this.collapsed.insert(kind);
                }
                this.ensure_selection(cx);
                cx.notify();
            }))
            .child(chevron(open))
            .child(dot(kind_colour(kind)))
            .child(
                div()
                    .text_size(tokens::text_xxs())
                    .line_height(tokens::line_xxs())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text3())
                    .child(label),
            )
            .child(
                div()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_xxs())
                    .line_height(tokens::line_xxs())
                    .text_color(tokens::text3())
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    /// A root row (h40: name over path, the meta at the right) or a nested
    /// one (h28, the name alone, 16 px in per level, its meta kept).
    fn node_row(
        &mut self,
        node: &DiffNode,
        depth: usize,
        has_children: bool,
        open: bool,
        pack: IconPack,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = node.id;
        let selected = self.selected == Some(id);
        let root = depth == 0;
        let marker = match node.kind {
            ChangeKind::Added => "+",
            ChangeKind::Updated => "~",
            ChangeKind::Removed => "\u{2212}",
        };
        let disclosure: AnyElement = if has_children {
            div()
                .id(("argon-diff-disclose", id))
                .flex_none()
                .w(px(9.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_expanded(id);
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(chevron(open))
                .into_any_element()
        } else {
            div().flex_none().w(px(9.)).into_any_element()
        };
        let meta = self.row_meta(node);
        h_flex()
            .id(("argon-diff-row", id))
            .h(px(if root { 40. } else { 28. }))
            .flex_none()
            .pr(px(8.))
            .pl(px(8. + 16. * depth as f32))
            .gap(px(6.))
            .items_center()
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .map(|this| {
                if selected {
                    this.bg(tokens::accent_soft())
                } else {
                    this.hover(|this| this.bg(tokens::hover_subtle()))
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select(id, cx);
                cx.notify();
            }))
            .child(disclosure)
            .child(
                div()
                    .flex_none()
                    .w(px(9.))
                    .text_center()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .font_weight(tokens::WEIGHT_SEMIBOLD)
                    .text_color(kind_colour(node.kind))
                    .child(if root { marker } else { "" }),
            )
            .child(class_icon(&node.class, pack, 14.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        div()
                            .truncate()
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child(node.name.clone()),
                    )
                    .when(root, |this| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(tokens::text_xs())
                                .line_height(tokens::line_xs())
                                .text_color(tokens::text3())
                                .child(node.path.clone()),
                        )
                    }),
            )
            .child(meta)
            .into_any_element()
    }

    /// The right column of a row: line counts for a script, the nested
    /// count for a container, the property count otherwise — and nothing
    /// when every count is zero.
    fn row_meta(&mut self, node: &DiffNode) -> AnyElement {
        if node.source.is_some() {
            let (added, removed) = self.line_counts(node);
            return plus_minus(added, removed, tokens::text_xs(), tokens::line_xs())
                .into_any_element();
        }
        let text = if node.nested > 0 {
            format!("+{} nested", node.nested)
        } else if !node.properties.is_empty() {
            format!("{} props", node.properties.len())
        } else {
            return div().into_any_element();
        };
        div()
            .flex_none()
            .font_family(tokens::FONT_FAMILY_MONO)
            .text_size(tokens::text_xs())
            .line_height(tokens::line_xs())
            .text_color(tokens::text3())
            .child(text)
            .into_any_element()
    }
}

/// A 9 px chevron in `text3`: down when open, right when not.
fn chevron(open: bool) -> Div {
    div()
        .flex_none()
        .w(px(9.))
        .flex()
        .text_color(tokens::text3())
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(px(9.)),
        )
}
