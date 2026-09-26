//! The dock's sections and rows: a 10px uppercase header over 28px rows,
//! each a label at the left and its control flush right — a switch, the
//! quality dropdown, the screen-size field with its swap button and
//! preset menu, or the frame-rate readout.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;
use crate::ui_canvas::PRESETS;

use super::super::menu::{self, MenuId};
use super::super::rows::{select_box, toggle_pill};
use super::super::{chrome, Shell};
use super::Toggle;

/// Every control on the right of a row is this wide.
const CONTROL_WIDTH: f32 = 150.;
const CONTROL_HEIGHT: f32 = 26.;
const ROW_HEIGHT: f32 = 28.;
/// A row's hover bleeds this far past its section's edges.
const ROW_BLEED: f32 = 8.;

/// A section: its header, 6px, its rows.
pub(super) fn section(title: &'static str, rows: Vec<AnyElement>) -> Div {
    v_flex()
        .min_w_0()
        .gap(px(6.))
        .child(
            div()
                .h(px(14.))
                .text_size(tokens::text_xxs())
                .line_height(tokens::line_xxs())
                .font_weight(tokens::WEIGHT_BOLD)
                .text_color(tokens::text3())
                .child(title),
        )
        .child(v_flex().children(rows))
}

/// A section at its wide-layout width.
pub(super) fn fixed(section: Div, width: f32) -> Div {
    section.w(px(width)).flex_none()
}

/// Two sections sharing a row equally, a hairline between them.
pub(super) fn pair(left: Div, right: Div, gap: f32) -> Div {
    h_flex()
        .items_start()
        .gap(px(gap))
        .child(left.flex_1())
        .child(vertical_rule())
        .child(right.flex_1())
}

pub(super) fn vertical_rule() -> Div {
    div()
        .w(px(1.))
        .flex_none()
        .self_stretch()
        .bg(tokens::border())
}

pub(super) fn horizontal_rule() -> Div {
    div().h(px(1.)).flex_none().bg(tokens::border())
}

/// 28px, the label at the left in `text2` (truncating), the control
/// flush right, 16px between; the hover bleeds 8px past the section.
fn row(label: &'static str, control: impl IntoElement) -> Div {
    h_flex()
        .h(px(ROW_HEIGHT))
        .mx(px(-ROW_BLEED))
        .px(px(ROW_BLEED))
        .items_center()
        .justify_between()
        .gap(px(16.))
        .rounded(tokens::radius_badge())
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .text_color(tokens::text2())
                .child(label),
        )
        .child(div().flex_none().child(control))
}

impl Shell {
    /// The switch rows of one section, numbered on from `first` in the
    /// roving group. The whole row is the target, not only the pill: the
    /// label is the obvious thing to click. Space and Enter on the
    /// focused row arrive as this same click.
    pub(super) fn switch_rows(
        &mut self,
        first: usize,
        toggles: Vec<Toggle>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let handle = cx.entity();
        toggles
            .into_iter()
            .enumerate()
            .map(|(offset, (label, on, set))| {
                let index = first + offset;
                let handle = handle.clone();
                let rows = self.viewport_rows.clone();
                let element = row(label, toggle_pill(Some(on)))
                    // Where this row landed, for the keyboard's scroll-into-view.
                    .on_children_prepainted(move |bounds, _, _| {
                        if let (Some(slot), Some(first)) =
                            (rows.borrow_mut().get_mut(index), bounds.first())
                        {
                            *slot = *first;
                        }
                    })
                    .id(SharedString::from(format!("viewport-{label}")))
                    .cursor_pointer()
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover_subtle()))
                    .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            set(shell, !on, cx);
                            cx.notify();
                        });
                    });
                self.viewport_nav
                    .item(index, element, cx)
                    .into_any_element()
            })
            .collect()
    }

    /// "Graphics quality": the 150×26 dropdown, the toolkit's select in
    /// the editor's own box. A Tab stop of its own: `SelectState` is
    /// `Focusable`, and its handle is the one `Select` itself focuses, so
    /// recording that handle in the window's order is all it takes. The
    /// toolkit draws no focus ring on a select without its own chrome, so
    /// the box draws the editor's, inset, since the row clips outside it.
    pub(super) fn quality_row(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let handle = self.quality.read(cx).focus_handle(cx);
        self.tab_order.register(&handle);
        let ringed = handle.contains_focused(window, cx) && window.last_input_was_keyboard();
        let control = select_box()
            .w(px(CONTROL_WIDTH))
            .h(px(CONTROL_HEIGHT))
            .px(px(9.))
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .font_weight(FontWeight::NORMAL)
            .text_color(tokens::text2())
            .when(ringed, |this| this.shadow(tokens::focus_ring_inset()))
            .child(
                Select::new(&self.quality)
                    .appearance(false)
                    .with_size(tokens::field_size())
                    .h_full()
                    .py_0()
                    .px_0()
                    // A 10px chevron, as every other dropdown here has, not the
                    // toolkit's 15px one.
                    .icon(Icon::new(IconName::ChevronDown).size(px(10.)))
                    .menu_width(px(CONTROL_WIDTH))
                    .accessibility_label("Graphics quality"),
            );
        row("Graphics quality", control).into_any_element()
    }

    /// "Screen size": a 26×26 swap button, 6px, then the 150×26 field —
    /// width × height in mono, and a 26px preset button at its right that
    /// opens "Viewport size" and the device presets. The same setting as
    /// the UI Editor's resolution, so a GUI laid out on the canvas is laid
    /// out the same in the 3D view; typing into either commits through
    /// the UI Editor's own fields.
    pub(super) fn screen_row(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_size_fields(window, cx);
        let (width, height) = self.ui_size_fields();
        let (w, h) = self.ui_resolution();
        let screen = self.viewport.read(cx).emulated_screen();
        let items = std::iter::once(
            menu::item("Viewport size")
                .checked(screen.is_none())
                .on_click(|shell, cx| shell.clear_viewport_screen(cx)),
        )
        .chain(PRESETS.iter().map(|&(label, pw, ph)| {
            menu::item(label)
                .checked(screen == Some((pw, ph)))
                .on_click(move |shell, cx| shell.set_resolution((pw, ph), cx))
        }))
        .collect();
        for state in [&width, &height] {
            self.tab_order.register(&state.read(cx).focus_handle(cx));
        }
        let input = |state: &Entity<gpui_kit::component::input::InputState>| {
            // The input at its line's height, centred by its slot, so the
            // digits sit where the field's own centre is.
            div()
                .w(px(48.))
                .flex_none()
                .h_full()
                .flex()
                .items_center()
                .child(
                    Input::new(state)
                        .appearance(false)
                        .w_full()
                        .h(tokens::line_sm())
                        .px(px(0.))
                        .py(px(0.))
                        .text_center()
                        .font_family(tokens::FONT_FAMILY_MONO)
                        .text_size(tokens::text_sm())
                        .line_height(tokens::line_sm())
                        .text_color(tokens::text()),
                )
        };
        let presets = menu::dropdown(
            self,
            MenuId::ViewportScreen,
            chrome::Trigger::new(
                div()
                    .id("viewport-screen")
                    .tab_index(self.tab_order.next())
                    .w(px(CONTROL_HEIGHT))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_l_1()
                    .border_color(tokens::border())
                    .rounded_r(tokens::radius_badge())
                    .cursor_pointer()
                    .text_color(tokens::text2())
                    .hover(|this| {
                        tokens::hover_fx(this)
                            .bg(tokens::hover())
                            .text_color(tokens::text())
                    })
                    .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
                    .tooltip(|window, cx| {
                        super::super::tooltip::text("Screen size presets", window, cx)
                    })
                    .child(Icon::new(IconName::ChevronDown).size(px(10.))),
            ),
            items,
            cx,
        );
        let field = h_flex()
            .w(px(CONTROL_WIDTH))
            .h(px(CONTROL_HEIGHT))
            .flex_none()
            .items_center()
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .child(input(&width))
            .child(
                div()
                    .flex_none()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text3())
                    .child("×"),
            )
            .child(input(&height))
            .child(div().flex_1())
            .child(presets);
        let swap = div()
            .id("viewport-screen-turn")
            .tab_index(self.tab_order.next())
            .size(px(CONTROL_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(tokens::radius())
            .cursor_pointer()
            .text_color(tokens::text2())
            .hover(|this| {
                tokens::hover_fx(this)
                    .bg(tokens::hover())
                    .text_color(tokens::text())
            })
            .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
            .tooltip(|window, cx| super::super::tooltip::text("Swap width and height", window, cx))
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.set_resolution((h, w), cx);
            }))
            .child(Icon::new(IconName::ArrowLeftRight).size(px(13.)));
        row(
            "Screen size",
            h_flex().items_center().gap(px(6.)).child(swap).child(field),
        )
        .into_any_element()
    }

    /// "Frame rate": the rate and, after a 3px dot, the frame time, in
    /// mono, in a 150px slot with the controls' 9px inset.
    pub(super) fn frame_rate_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let [(_, rate), _] = self.viewport.read(cx).readout();
        let mut parts = rate.split('\u{b7}');
        let fps = parts.next().unwrap_or_default().trim().to_owned();
        let ms = parts.next().map(|text| text.trim().to_owned());
        let readout = h_flex()
            .w(px(CONTROL_WIDTH))
            .px(px(9.))
            .items_center()
            .gap(px(8.))
            .font_family(tokens::FONT_FAMILY_MONO)
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .child(div().text_color(tokens::text()).child(fps))
            .children(
                ms.map(|ms| {
                    vec![
                        div()
                            .flex_none()
                            .size(px(3.))
                            .rounded_full()
                            .bg(tokens::text3())
                            .into_any_element(),
                        div()
                            .text_color(tokens::text2())
                            .child(ms)
                            .into_any_element(),
                    ]
                })
                .into_iter()
                .flatten(),
            );
        row("Frame rate", readout).into_any_element()
    }
}
