//! The detail pane: the selected change's header (tile, name, class
//! badge, kind pill, stats, full path), then its PROPERTIES table, its
//! SOURCE card and its CONTENTS grid, each only when it applies.

use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::shell::argon_sync::{ChangeKind, DiffNode};
use crate::tokens;

use super::values::{class_icon, dot, kind_colour, plus_minus, FormattedProperty};

mod table;
use super::ArgonDiffWindow;
use table::{contents_grid, properties_table};

impl ArgonDiffWindow {
    pub(super) fn detail_pane(
        &mut self,
        node: Option<&DiffNode>,
        properties: &[FormattedProperty],
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
                .child("No change selected");
        };
        let header = self.detail_header(node, pack, narrow);
        let body = self.detail_body(node, properties, pack, narrow, cx);
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(header)
            .child(body)
    }

    /// `padding:16px 20px 14px` (16 14 14 narrow), a hairline under it.
    fn detail_header(&mut self, node: &DiffNode, pack: IconPack, narrow: bool) -> Div {
        let (pill_label, pill_bg) = match node.kind {
            ChangeKind::Added => ("Added", tokens::diff_add_pill()),
            ChangeKind::Updated => ("Updated", tokens::accent_soft()),
            ChangeKind::Removed => ("Removed", tokens::diff_remove_pill()),
        };
        let ink = kind_colour(node.kind);
        let stats = self.stats(node);
        v_flex()
            .flex_none()
            .gap(px(8.))
            .pt(px(16.))
            .pb(px(14.))
            .px(px(if narrow { 14. } else { 20. }))
            .border_b_1()
            .border_color(tokens::border())
            .child(
                h_flex()
                    .h(px(28.))
                    .items_center()
                    .gap(px(10.))
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
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(tokens::WEIGHT_BOLD)
                            .text_color(tokens::text())
                            .child(node.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px(px(6.))
                            .py(px(1.))
                            .border_1()
                            .border_color(tokens::border2())
                            .rounded(tokens::radius_badge())
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .text_size(tokens::text_xs())
                            .line_height(tokens::line_xs())
                            .text_color(tokens::text2())
                            .child(node.class.clone()),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap(px(6.))
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(tokens::radius_badge())
                            .bg(pill_bg)
                            .text_size(tokens::text_badge())
                            .line_height(tokens::line_badge())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(ink)
                            .child(dot(ink))
                            .child(pill_label),
                    )
                    .child(div().flex_1())
                    .child(stats),
            )
            .child(
                div()
                    .pl(px(38.))
                    .truncate()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_badge())
                    .line_height(tokens::line_badge())
                    .text_color(tokens::text3())
                    .child(node.full_path()),
            )
    }

    /// Mono 11/16 at the header's right: `+a −r lines`, `+N lines`,
    /// `−N lines`, `N properties`, `+N nested` or `N nested`.
    fn stats(&mut self, node: &DiffNode) -> AnyElement {
        let word = |text: String| {
            div()
                .flex_none()
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_badge())
                .line_height(tokens::line_badge())
                .text_color(tokens::text3())
                .child(text)
        };
        if node.source.is_some() {
            let (added, removed) = self.line_counts(node);
            return h_flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .child(plus_minus(
                    added,
                    removed,
                    tokens::text_badge(),
                    tokens::line_badge(),
                ))
                .child(word("lines".to_owned()))
                .into_any_element();
        }
        let text = match node.kind {
            ChangeKind::Updated => format!("{} properties", node.properties.len()),
            ChangeKind::Added if node.nested > 0 => format!("+{} nested", node.nested),
            ChangeKind::Removed if node.nested > 0 => format!("{} nested", node.nested),
            _ => format!("{} properties", node.properties.len()),
        };
        word(text).into_any_element()
    }

    /// `padding:16px 20px 20px` (16 14 20 narrow), blocks 16 apart. The
    /// SOURCE card takes what is left and scrolls inside itself.
    fn detail_body(
        &mut self,
        node: &DiffNode,
        properties: &[FormattedProperty],
        pack: IconPack,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_stretch()
            .gap(px(16.))
            .pt(px(16.))
            .pb(px(20.))
            .px(px(if narrow { 14. } else { 20. }));
        if node.kind != ChangeKind::Removed && !node.properties.is_empty() {
            body = body
                .child(section_label("PROPERTIES", None))
                .child(properties_table(node, properties));
        }
        if let Some(rows) = self.code_rows(node, cx) {
            let unified = matches!(&node.source, Some(source) if source.old.is_some() && source.new.is_some());
            body = body
                .child(section_label("SOURCE", None))
                .child(self.code_card(rows, unified, narrow, cx));
        }
        if node.kind != ChangeKind::Updated && node.nested > 0 {
            body = body
                .child(section_label("CONTENTS", Some(node.nested)))
                .child(contents_grid(node, pack));
            if node.kind == ChangeKind::Added {
                body = body.child(
                    div()
                        .text_size(tokens::text_sm())
                        .line_height(tokens::line_sm())
                        .text_color(tokens::text3())
                        .child(format!(
                            "Select an item under {} in the list to see its own properties.",
                            node.name
                        )),
                );
            }
        }
        body
    }

    /// The card's rows for this node, cached on the batch and the node.
    fn code_rows(
        &mut self,
        node: &DiffNode,
        cx: &mut Context<Self>,
    ) -> Option<Rc<Vec<super::code::CodeRow>>> {
        node.source.as_ref()?;
        Some(self.rows_for(node, cx))
    }
}

/// h14: 10/14 700 `text3`, and a mono count at the right when given.
fn section_label(title: &'static str, count: Option<usize>) -> Div {
    h_flex()
        .h(px(14.))
        .flex_none()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(tokens::text_xxs())
                .line_height(tokens::line_xxs())
                .font_weight(tokens::WEIGHT_BOLD)
                .text_color(tokens::text3())
                .child(title),
        )
        .child(div().flex_1())
        .children(count.map(|count| {
            div()
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_xxs())
                .line_height(tokens::line_xxs())
                .text_color(tokens::text3())
                .child(count.to_string())
        }))
}
