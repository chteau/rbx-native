//! The Effects section: one group per `UIShadow`, Figma's drop shadows,
//! listed in the order they draw — by `ZIndex`, ties in tree order, as the
//! renderer stacks them. The `+` adds another rather than standing in for
//! a missing one, since an element may cast several. A group folds to its
//! head and a summary; a hidden shadow's group dims but stays editable.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::super::super::chrome;
use super::sections::add_button;
use super::spec::SHADOW;
use super::value::show;
use super::view::{caption, line, section, Label};
use super::{Key, Shell};
use crate::tokens;

/// What a hidden shadow's label, summary and rows fade to; its buttons do
/// not, so Show and Remove stay within reach.
const HIDDEN: f32 = 0.45;
/// The folded summary's dot is the shadow's colour at its own opacity, but
/// never fainter than this, or a light shadow's dot vanishes.
const DOT_FLOOR: f32 = 0.35;

impl Shell {
    pub(super) fn effects_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let add = add_button(
            "ui-inspect-shadow-add",
            "Add a shadow (UIShadow)",
            cx,
            |shell, cx| shell.add_shadow(cx),
        );
        let shadows: Vec<Ref> = self
            .inspected()
            .first()
            .map(|&element| self.children_of(element, SHADOW).collect())
            .unwrap_or_default();
        let order = draw_order(shadows.iter().map(|&shadow| self.z_index(shadow)));
        let groups = order
            .into_iter()
            .enumerate()
            .map(|(place, n)| {
                let group = self.shadow_group(n, shadows[n], window, cx);
                match place {
                    0 => group,
                    _ => div()
                        .w_full()
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(tokens::border())
                        .child(group)
                        .into_any_element(),
                }
            })
            .collect();
        section("Effects", vec![add], groups).into_any_element()
    }

    /// The `n`th shadow (in tree order, which its fields are keyed by): its
    /// head, and while open its colour, offset, blur and spread.
    fn shadow_group(
        &mut self,
        n: usize,
        shadow: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folded = self.ui.inspector.folded.contains(&shadow);
        let enabled = self.enabled(shadow);
        let dim = |this: Div| this.when(!enabled, |this| this.opacity(HIDDEN));

        let (chevron, fold_label) = match folded {
            true => (IconName::ChevronRight, "Open the shadow"),
            false => (IconName::ChevronDown, "Fold the shadow"),
        };
        let fold = chrome::icon_button(("ui-inspect-shadow-fold", n), chevron, fold_label)
            .on_click(cx.listener(move |shell, _, _, cx| {
                let folded = &mut shell.ui.inspector.folded;
                if !folded.remove(&shadow) {
                    folded.insert(shadow);
                }
                cx.notify();
            }))
            .into_any_element();
        let title = dim(div()
            .flex_1()
            .min_w_0()
            .text_size(tokens::text_sm())
            .text_color(tokens::text_full())
            .child("Drop shadow"))
        .into_any_element();

        let mut buttons = Vec::new();
        if !folded {
            buttons.push(
                chrome::icon_button(
                    ("ui-inspect-shadow-edit", n),
                    IconName::SlidersHorizontal,
                    "More shadow options",
                )
                .on_click(cx.listener(move |shell, _, _, cx| shell.select_nth(SHADOW, n, cx)))
                .into_any_element(),
            );
        }
        let (eye, eye_label) = match enabled {
            true => (IconName::Eye, "Hide the shadow"),
            false => (IconName::EyeOff, "Show the shadow"),
        };
        buttons.push(
            chrome::icon_button(("ui-inspect-shadow-eye", n), eye, eye_label)
                .on_click(cx.listener(move |shell, _, _, cx| shell.toggle_shadow(n, cx)))
                .into_any_element(),
        );
        buttons.push(
            chrome::danger_icon_button(
                ("ui-inspect-shadow-remove", n),
                IconName::Minus,
                "Remove the shadow",
            )
            .on_click(cx.listener(move |shell, _, _, cx| shell.remove_nth(SHADOW, n, cx)))
            .into_any_element(),
        );

        let mut head = vec![fold, title];
        if folded {
            head.push(dim(self.shadow_summary(n)).into_any_element());
        }
        head.push(
            h_flex()
                .flex_none()
                .gap(px(2.))
                .children(buttons)
                .into_any_element(),
        );
        let group = v_flex().w_full().gap(px(6.)).child(line(head));
        if folded {
            return group.into_any_element();
        }

        let paint = self.paint(Key::ShadowColor(n), Key::ShadowAlpha(n), window, cx);
        let x = self.field(Key::ShadowX(n), Label::Text("X"), None, window, cx);
        let y = self.field(Key::ShadowY(n), Label::Text("Y"), None, window, cx);
        let blur = self.field(
            Key::ShadowBlur(n),
            Label::Icon(IconName::Focus),
            Some("px"),
            window,
            cx,
        );
        let spread_x = self.field(Key::ShadowSpreadX(n), Label::Text("X"), None, window, cx);
        let spread_y = self.field(Key::ShadowSpreadY(n), Label::Text("Y"), None, window, cx);
        let body = dim(v_flex().w_full().gap(px(6.)).children([
            line(paint),
            line(vec![caption("Offset"), x, y]),
            line(vec![caption("Blur"), blur]),
            line(vec![caption("Spread"), spread_x, spread_y]),
        ]));
        group.child(body).into_any_element()
    }

    /// A folded shadow's dot and `x y blur`.
    fn shadow_summary(&self, n: usize) -> Div {
        let number = |key| match self.reading(key) {
            Some(Some(value)) => show(self.spec(key).form, &value),
            _ => "–".to_owned(),
        };
        let text = [Key::ShadowX(n), Key::ShadowY(n), Key::ShadowBlur(n)]
            .map(number)
            .join(" ");
        let rgb = match self.reading(Key::ShadowColor(n)) {
            Some(Some(rgb)) => rgb,
            _ => vec![0.0; 3],
        };
        let opacity = match self.reading(Key::ShadowAlpha(n)) {
            Some(Some(percent)) => percent[0] / 100.0,
            _ => 1.0,
        };
        let dot = Rgba {
            r: rgb[0] / 255.0,
            g: rgb[1] / 255.0,
            b: rgb[2] / 255.0,
            a: opacity.max(DOT_FLOOR),
        };
        h_flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            // The ring sits outside the dot, as an outline would: the dot
            // keeps its 8 px in the row and the ring overhangs it by one,
            // placed absolutely so it takes no room of its own.
            .child(
                div()
                    .flex_none()
                    .relative()
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot)
                    .child(
                        div()
                            .absolute()
                            .top(px(-1.))
                            .left(px(-1.))
                            .size(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(tokens::border2()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::scaled(11.))
                    .line_height(tokens::scaled(16.))
                    .text_color(tokens::text_muted())
                    .child(text),
            )
    }
}

/// The tree-order indices of shadows whose `ZIndex`es are `z`, in the order
/// they draw: ascending, ties in tree order.
pub(super) fn draw_order(z: impl Iterator<Item = i32>) -> Vec<usize> {
    let mut order: Vec<(usize, i32)> = z.enumerate().collect();
    order.sort_by_key(|&(_, z)| z);
    order.into_iter().map(|(n, _)| n).collect()
}
