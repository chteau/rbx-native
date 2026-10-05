//! The pieces every launcher window is built from: the three button
//! weights, pills,
//! the modal frame, section headers. Colours are the shell's own tokens —
//! the design's panel/panel2/border… are exactly those values.

use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::tokens;

pub(crate) fn panel() -> Rgba {
    tokens::dock()
}
pub(crate) fn panel2() -> Rgba {
    tokens::field_select()
}
pub(crate) fn bg() -> Rgba {
    tokens::black()
}
pub(crate) fn accent() -> Rgba {
    tokens::check_on()
}
pub(crate) fn green() -> Rgba {
    tokens::diff_add()
}
pub(crate) fn green_soft() -> Rgba {
    tokens::diff_add_pill()
}
pub(crate) fn red() -> Rgba {
    tokens::text_error()
}
pub(crate) fn red_soft() -> Rgba {
    tokens::error_soft()
}
/// `rgba(224,108,108,.55)`: a field holding an error.
pub(crate) fn red_line() -> Rgba {
    Rgba { a: 0.55, ..red() }
}
/// `rgba(255,255,255,.05)`: a neutral pill, a skeleton bar.
pub(crate) fn wash() -> Rgba {
    tokens::hover()
}
/// `rgba(255,255,255,.04)`: the lighter skeleton bar.
pub(crate) fn wash_faint() -> Rgba {
    tokens::wash_faint()
}
/// A Lucide icon from the kit's full catalogue, by file name.
pub(crate) fn icon(name: &'static str, size: f32) -> Icon {
    Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
}

/// Text at a given `size`/`line` height, in px.
pub(crate) fn text(size: f32, line: f32) -> Div {
    div().text_size(px(size)).line_height(px(line))
}

pub(crate) fn mono(size: f32, line: f32) -> Div {
    text(size, line).font_family(tokens::FONT_FAMILY_MONO)
}

/// The four button weights, 34 tall unless `small` (28, 12 px text) — except
/// Primary and Danger, which are the buttons that commit or destroy and are
/// 44 either way (WCAG 2.5.5; see [`button_height`]).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Weight {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub(crate) fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    weight: Weight,
    small: bool,
) -> Stateful<Div> {
    button_frame(id, weight, small).child(label.into())
}

/// [`button`] with a glyph ahead of its label ("Paste", "Manage key").
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    label: impl Into<SharedString>,
    weight: Weight,
    small: bool,
) -> Stateful<Div> {
    button_frame(id, weight, small)
        .child(icon(glyph, if small { 12. } else { 13. }))
        .child(label.into())
}

/// [`button`] with a trailing ↗ — a link that leaves the app.
pub(crate) fn external_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    weight: Weight,
    small: bool,
) -> Stateful<Div> {
    button(id, label, weight, small).child(icon("external-link", if small { 12. } else { 13. }))
}

/// A button's height in px. Primary and Danger take 2.5.5's 44 even when
/// `small`: the visual grows with the hit area because a gpui element has no
/// separate hit slop, and 44 matches the platform buttons people already
/// hit (Material 48, Apple 44).
pub(crate) fn button_height(weight: Weight, small: bool) -> f32 {
    let h: f32 = if small { 28. } else { 34. };
    match weight {
        Weight::Primary | Weight::Danger => h.max(44.),
        Weight::Secondary | Weight::Ghost => h,
    }
}

fn button_frame(id: impl Into<ElementId>, weight: Weight, small: bool) -> Stateful<Div> {
    let h = button_height(weight, small);
    let id: ElementId = id.into();
    let probe = id.to_string();
    let (size, line, px_x) = if small {
        (12., 16., 10.)
    } else {
        (12.5, 17., if weight == Weight::Primary { 16. } else { 14. })
    };
    let base = h_flex()
        .id(id)
        .debug_selector(move || probe)
        .h(px(h))
        .min_w(px(44.))
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .px(px(px_x))
        .rounded(px(5.))
        .text_size(px(size))
        .line_height(px(line))
        .cursor_pointer()
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())));
    match weight {
        Weight::Primary => base
            .bg(accent())
            .text_color(bg())
            .font_weight(FontWeight::BOLD)
            .hover(|this| tokens::hover_fx(this).bg(tokens::accent_hover())),
        Weight::Danger => base
            .bg(red())
            .text_color(bg())
            .font_weight(FontWeight::BOLD),
        Weight::Secondary => base
            .border_1()
            .border_color(tokens::border2())
            .bg(panel2())
            .text_color(tokens::text())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|this| tokens::hover_fx(this).bg(tokens::secondary_hover())),
        Weight::Ghost => base
            .px(px(if small { 8. } else { 10. }))
            .text_color(tokens::text2())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|this| tokens::hover_fx(this).bg(wash()).text_color(tokens::text())),
    }
}

/// A primary button that cannot be pressed yet: `panel2`, hairline, text3.
pub(crate) fn disabled_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .h(px(34.))
        .flex_none()
        .items_center()
        .justify_center()
        .px(px(16.))
        .rounded(px(5.))
        .border_1()
        .border_color(tokens::border())
        .bg(panel2())
        .text_color(tokens::text3())
        .text_size(px(12.5))
        .line_height(px(17.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

/// `h 20`, radius 4, 10.5/14 semibold: the visibility pill, a restricted
/// grant, a linked Recent entry (`accent` when `accented`).
pub(crate) fn pill(
    label: impl Into<SharedString>,
    glyph: Option<&'static str>,
    accented: bool,
) -> Div {
    h_flex()
        .h(px(20.))
        .max_w_full()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .px(px(7.))
        .rounded(px(4.))
        .overflow_hidden()
        .bg(if accented {
            tokens::accent_soft()
        } else {
            wash()
        })
        .text_color(if accented { accent() } else { tokens::text2() })
        .text_size(px(10.5))
        .line_height(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .when_some(glyph, |this, glyph| {
            this.child(icon(glyph, 11.).flex_none())
        })
        .child(div().truncate().child(label.into()))
}

/// The `h 18` caps tag: RECOMMENDED, READY.
pub(crate) fn tag(label: &'static str, fg: Rgba, fill: Rgba) -> Div {
    h_flex()
        .h(px(18.))
        .flex_none()
        .items_center()
        .px(px(6.))
        .rounded(px(4.))
        .bg(fill)
        .text_color(fg)
        .text_size(px(10.))
        .line_height(px(14.))
        .font_weight(FontWeight::BOLD)
        .child(label)
}

/// A round status glyph: `size` 22 in headers, 16 in table rows.
pub(crate) fn status_dot(glyph: &'static str, fg: Rgba, fill: Option<Rgba>, size: f32) -> Div {
    h_flex()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .items_center()
        .justify_center()
        .text_color(fg)
        .map(|this| match fill {
            Some(fill) => this.bg(fill),
            None => this.border_1().border_color(tokens::border2()),
        })
        .child(icon(glyph, if size > 20. { 13. } else { 10. }))
}

/// A turning `loader-circle`, still under reduced motion.
pub(crate) fn spinner(id: &'static str, size: f32) -> AnyElement {
    let glyph = icon("loader-circle", size);
    if tokens::reduced_motion() {
        return glyph.into_any_element();
    }
    glyph
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
}

/// The modal every dialog sits in: a 55 % black veil below the title bar,
/// a `width`-wide card (panel, border2, radius 10), its header row (glyph,
/// title, text), whatever `body` adds, and the 60 px `panel2` footer.
pub(crate) fn dialog(
    width: f32,
    glyph: AnyElement,
    title: impl Into<SharedString>,
    body_text: impl Into<SharedString>,
    body: Option<AnyElement>,
    footer: Vec<AnyElement>,
) -> impl IntoElement {
    div()
        .id("launcher-dialog-veil")
        .absolute()
        .left_0()
        .right_0()
        .top(tokens::topbar_height())
        .bottom_0()
        .bg(tokens::veil())
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .child(
            v_flex()
                .w(px(width))
                .border_1()
                .border_color(tokens::border2())
                .rounded(px(10.))
                .bg(panel())
                .shadow(vec![BoxShadow {
                    color: rgba(0x00000080).into(),
                    offset: point(px(0.), px(16.)),
                    blur_radius: px(48.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .overflow_hidden()
                .child(
                    h_flex()
                        .items_start()
                        .gap(px(14.))
                        .pt(px(20.))
                        .px(px(20.))
                        .child(glyph)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(4.))
                                .child(
                                    text(15., 21.)
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(tokens::text())
                                        .child(title.into()),
                                )
                                .child(
                                    text(12.5, 19.)
                                        .text_color(tokens::text2())
                                        .child(body_text.into()),
                                ),
                        ),
                )
                .children(body)
                .child(
                    h_flex()
                        .h(px(60.))
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .px(px(20.))
                        .mt(px(20.))
                        .border_t_1()
                        .border_color(tokens::border())
                        .bg(panel2())
                        .children(footer),
                ),
        )
}

/// The round tinted glyph a failing dialog leads with.
pub(crate) fn dialog_glyph(glyph: &'static str, fg: Rgba, fill: Rgba) -> AnyElement {
    h_flex()
        .size(px(44.))
        .flex_none()
        .rounded_full()
        .bg(fill)
        .text_color(fg)
        .items_center()
        .justify_center()
        .child(icon(glyph, 20.))
        .into_any_element()
}

/// The single-line field chrome search and the place
/// link: `w`×32, radius 6, a leading glyph, `border` swapped by state.
pub(crate) fn field_frame(
    width: Option<f32>,
    fill: Rgba,
    border: Rgba,
    glyph: &'static str,
) -> Div {
    h_flex()
        .when_some(width, |this, w| this.w(px(w)))
        .h(px(32.))
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(border)
        .bg(fill)
        .text_color(tokens::text3())
        .child(icon(glyph, 13.).flex_none())
}

#[cfg(test)]
mod tests {
    use super::{button, button_height, Weight};
    use crate::probe::assert_primary;

    /// WCAG 2.5.5: the buttons that commit or destroy are 44 tall in both
    /// sizes, with Large Click Targets off (it is not consulted here at all).
    #[test]
    fn primary_and_danger_buttons_are_44_tall_in_every_size() {
        for weight in [Weight::Primary, Weight::Danger] {
            for small in [false, true] {
                assert!(button_height(weight, small) >= 44.);
            }
        }
        // Secondary and Ghost keep the compact design.
        assert_eq!(button_height(Weight::Secondary, false), 34.);
    }

    /// The same floor, measured through gpui layout rather than read off the
    /// constant: every Primary and Danger button, in both sizes.
    #[gpui_kit::test]
    fn primary_and_danger_buttons_lay_out_at_least_44_by_44(cx: &mut gpui_kit::TestAppContext) {
        let cx = cx.add_empty_window();
        for (id, weight, small) in [
            ("primary", Weight::Primary, false),
            ("primary-small", Weight::Primary, true),
            ("danger", Weight::Danger, false),
            ("danger-small", Weight::Danger, true),
        ] {
            assert_primary(cx, id, 200., button(id, "OK", weight, small));
        }
    }
}
