//! A part's `BrickColor` row: the colour's swatch and name, opening onto the
//! 128-colour palette Studio's own picker shows (`BrickColor.palette`'s
//! docs), laid out in palette order. Studio arranges the same colours in a
//! honeycomb; this is a grid of the same 128. A pick commits the colour's
//! number through the row like any other edit, which writes the part's
//! `Color` (see `properties::edit::commit_all`).

use gpui_kit::component::h_flex;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::BrickColor;

use super::chrome::Trigger;
use super::rows::select_box;
use super::Shell;
use crate::tokens;

/// Sixteen across and eight down: the 128 palette colours in whole rows.
const COLUMNS: usize = 16;

impl Shell {
    /// `current` is `None` for a multi-selection whose colours differ: the
    /// field shows no colour then, and a pick sets them all.
    pub(super) fn brick_color_picker(
        &self,
        row: &str,
        current: Option<u32>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        let shown = current.and_then(BrickColor::from_number);
        let field = select_box()
            .id(SharedString::from(format!("brick-color-{row}")))
            .gap(tokens::label_gap())
            .cursor_pointer()
            .when_some(shown, |this, color| {
                this.child(swatch_box(color).size(px(12.)))
                    .child(div().truncate().child(color.name))
            });
        let handle = cx.entity();
        let row = row.to_owned();
        Popover::new(SharedString::from(format!("brick-color-popover-{row}")))
            .trigger(Trigger::new(field))
            .content(move |_, _, cx| palette(handle.clone(), row.clone(), current, cx.entity()))
    }
}

fn palette(
    shell: Entity<Shell>,
    row: String,
    current: Option<u32>,
    popover: Entity<PopoverState>,
) -> impl IntoElement {
    let colors: Vec<&'static BrickColor> = BrickColor::palette().collect();
    v_flex()
        .gap(px(2.))
        .p(px(4.))
        .children(colors.chunks(COLUMNS).map(|line| {
            h_flex().gap(px(2.)).children(line.iter().map(|&color| {
                let shell = shell.clone();
                let popover = popover.clone();
                let row = row.clone();
                swatch_box(color)
                    .id(("brick-color", color.number))
                    .size(px(16.))
                    .cursor_pointer()
                    .when(current == Some(color.number), |this| {
                        this.border_2().border_color(tokens::text_full())
                    })
                    .hover(|this| this.border_1().border_color(tokens::text_strong()))
                    .tooltip(move |window, cx| super::tooltip::text(color.name, window, cx))
                    .on_click(move |_, window, cx| {
                        let number = color.number.to_string();
                        shell.update(cx, |shell, cx| shell.commit_row(&row, &number, cx));
                        popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                    })
            }))
        }))
}

fn swatch_box(color: &BrickColor) -> Div {
    let [r, g, b] = color.rgb;
    div().flex_none().rounded(tokens::RADIUS_TINY).bg(Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    })
}
