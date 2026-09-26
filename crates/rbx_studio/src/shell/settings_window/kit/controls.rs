//! The controls a row carries: the switch, the segmented picker, the
//! slider and its readout, the ghost icon button.

use std::rc::Rc;

use gpui_kit::base::{Slider as Behaviour, SliderIndicator, SliderThumb, SliderTrack};
use gpui_kit::component::h_flex;
use gpui_kit::component::slider::SliderState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::icon;

/// What picking a segment does.
pub(in crate::shell::settings_window) type OnPick = Rc<dyn Fn(&mut Window, &mut App)>;

/// A 24 px ghost button holding one glyph, in text3.
pub(in crate::shell::settings_window) fn ghost_icon(
    id: impl Into<ElementId>,
    glyph: &'static str,
    label: &'static str,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .flex_none()
        .size(px(24.))
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(tokens::text3())
        .cursor_pointer()
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
        .child(icon(glyph, 12.))
        .tooltip(move |window, cx| super::super::super::tooltip::text(label, window, cx))
}

/// The 32×18 switch: an accent track with a bg knob when on, a 10% white
/// one with a text2 knob when off.
pub(in crate::shell::settings_window) fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .relative()
        .flex_none()
        .w(px(32.))
        .h(px(18.))
        .rounded(px(9.))
        .cursor_pointer()
        .bg(if on {
            tokens::check_on()
        } else {
            rgba(0xFFFFFF1A)
        })
        .on_click(on_click)
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 17. } else { 3. }))
                .size(px(12.))
                .rounded_full()
                .bg(if on { tokens::black() } else { tokens::text2() }),
        )
}

/// `color` as it shows at 40% over a card: what a roadmap row's control
/// is drawn in. Mixed here rather than left to `opacity`, which GPUI
/// applies to each shape on its own, so a knob faded over a faded track
/// lets the track show through it where CSS would fade the two as one.
pub(in crate::shell::settings_window) fn faded(color: Rgba) -> Rgba {
    let card = tokens::field_select();
    let over = |c: f32, under: f32| c * color.a + under * (1. - color.a);
    let mix = |c: f32, under: f32| 0.4 * over(c, under) + 0.6 * under;
    Rgba {
        r: mix(color.r, card.r),
        g: mix(color.g, card.g),
        b: mix(color.b, card.b),
        a: 1.,
    }
}

/// [`toggle`] on a roadmap row: faded, and nothing to click.
pub(in crate::shell::settings_window) fn still_toggle(on: bool) -> Div {
    div()
        .relative()
        .flex_none()
        .w(px(32.))
        .h(px(18.))
        .rounded(px(9.))
        .bg(faded(if on {
            tokens::check_on()
        } else {
            rgba(0xFFFFFF1A)
        }))
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(px(if on { 17. } else { 3. }))
                .size(px(12.))
                .rounded_full()
                .bg(faded(if on { tokens::black() } else { tokens::text2() })),
        )
}

/// A segmented control: `items` as `(label, selected, on_click)`, items
/// `h` tall (26, or 24 beside a slider).
pub(in crate::shell::settings_window) fn segmented(
    id: &'static str,
    h: f32,
    items: Vec<(&'static str, bool, OnPick)>,
) -> Div {
    h_flex()
        .flex_none()
        .gap(px(2.))
        .p(px(2.))
        .border_1()
        .border_color(tokens::border())
        .rounded(px(6.))
        .bg(tokens::dock())
        .children(
            items
                .into_iter()
                .enumerate()
                .map(|(i, (label, selected, on_click))| {
                    h_flex()
                        .id((id, i))
                        .h(px(h))
                        .px(px(11.))
                        .items_center()
                        .rounded(px(4.))
                        .text_size(px(11.5))
                        .line_height(px(16.))
                        .cursor_pointer()
                        .map(|this| {
                            if selected {
                                this.bg(tokens::accent_soft())
                                    .text_color(tokens::check_on())
                                    .font_weight(FontWeight::SEMIBOLD)
                            } else {
                                this.text_color(tokens::text2())
                                    .hover(|this| this.bg(tokens::hover()))
                            }
                        })
                        .on_click(move |_, window, cx| on_click(window, cx))
                        .child(label)
                }),
        )
}

/// The 58×26 mono box beside a slider.
pub(in crate::shell::settings_window) fn readout(value: impl Into<SharedString>) -> Div {
    h_flex()
        .flex_none()
        .w(px(58.))
        .h(px(26.))
        .items_center()
        .justify_center()
        .border_1()
        .border_color(tokens::border())
        .rounded(px(5.))
        .bg(tokens::dock())
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(px(11.5))
        .line_height(px(16.))
        .text_color(tokens::text())
        .child(value.into())
}

/// The rail's fill, the 14 px knob in text colour with a 3 px accent-soft
/// ring, over a 150×18 strip. `fraction` places both.
fn rail(fraction: f32) -> [Div; 2] {
    [
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(7.))
            .h(px(4.))
            .rounded(px(2.))
            .bg(rgba(0xFFFFFF1A))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .h_full()
                    .w(relative(fraction))
                    .rounded(px(2.))
                    .bg(tokens::check_on()),
            ),
        knob(),
    ]
}

fn knob() -> Div {
    div()
        .size(px(14.))
        .rounded_full()
        .bg(tokens::text())
        .shadow(vec![BoxShadow {
            color: tokens::accent_soft().into(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(3.),
            inset: false,
        }])
}

/// A slider nobody can move, `width` wide: a roadmap row's picture of
/// one. `ticks` marks that many evenly spaced stops under the rail.
pub(in crate::shell::settings_window) fn still_slider(
    fraction: f32,
    width: f32,
    ticks: usize,
) -> Div {
    let rail = |w: Length, color: Rgba| {
        div()
            .absolute()
            .left_0()
            .top(px(7.))
            .h(px(4.))
            .w(w)
            .rounded(px(2.))
            .bg(faded(color))
    };
    div()
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(18.))
        .child(rail(relative(1.).into(), rgba(0xFFFFFF1A)))
        .child(rail(relative(fraction).into(), tokens::check_on()))
        .children((0..ticks).map(|i| {
            let at = if ticks > 1 {
                i as f32 / (ticks - 1) as f32
            } else {
                0.
            };
            div()
                .absolute()
                .top(px(14.))
                .left(px((width * at).min(width - 1.)))
                .w(px(1.))
                .h(px(4.))
                .bg(faded(tokens::border2()))
        }))
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(width * fraction - 7.))
                .size(px(14.))
                .rounded_full()
                .bg(faded(tokens::text()))
                .shadow(vec![BoxShadow {
                    color: faded(tokens::accent_soft()).into(),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(3.),
                    inset: false,
                }]),
        )
}

/// A live slider over `state`, the toolkit's behaviour in this skin.
pub(in crate::shell::settings_window) fn slider(
    state: &Entity<SliderState>,
    cx: &App,
) -> impl IntoElement {
    ticked_slider(state, 150., &[], cx)
}

/// [`slider`], `width` wide, with a tick under the rail at each fraction
/// in `ticks`.
pub(in crate::shell::settings_window) fn ticked_slider(
    state: &Entity<SliderState>,
    width: f32,
    ticks: &[f32],
    cx: &App,
) -> impl IntoElement {
    let fraction = state.read(cx).percentage().end;
    let [track, knob] = rail(fraction);
    Behaviour::new(state).flex_none().w(px(width)).child(
        SliderTrack::new(state)
            .relative()
            .w_full()
            .h(px(18.))
            .child(
                SliderIndicator::new(state)
                    .absolute()
                    .inset_0()
                    .child(track),
            )
            .children(ticks.iter().map(|at| {
                div()
                    .absolute()
                    .top(px(14.))
                    .left(px((width * at).min(width - 1.)))
                    .w(px(1.))
                    .h(px(4.))
                    .bg(tokens::border2())
            }))
            .child(
                // Centred on the value, as the still one is: at either end
                // half the knob hangs past the rail.
                SliderThumb::new(state)
                    .absolute()
                    .top(px(2.))
                    .left(px(width * fraction - 7.))
                    .child(knob),
            ),
    )
}

/// A small secondary button: 28 tall, border2 over panel2, a 12 px glyph
/// before its label.
pub(in crate::shell::settings_window) fn secondary_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    label: &'static str,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .flex_none()
        .h(px(28.))
        .px(px(10.))
        .gap(px(6.))
        .items_center()
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(5.))
        .bg(tokens::field_select())
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text())
        .cursor_pointer()
        .hover(|this| this.bg(rgba(0x202123FF)))
        .child(icon(glyph, 12.))
        .child(label)
}
