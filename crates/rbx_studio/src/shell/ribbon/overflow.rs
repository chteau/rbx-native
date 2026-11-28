//! What the ribbon shows when a tab's buttons don't fit the window: a
//! chevron at each end that has more past it, which scrolls the strip by
//! most of its width. Without one, the buttons past the edge were only
//! reachable by a sideways wheel nobody knew to try.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::Shell;
use crate::tokens;

/// The strip's scroll position, and the tab it belongs to: another tab
/// starts at its beginning.
#[derive(Default)]
pub(in crate::shell) struct RibbonScroll {
    handle: ScrollHandle,
    tab: super::Tab,
}

/// Which ends have buttons past them, from the strip's last layout.
fn hidden(handle: &ScrollHandle) -> (bool, bool) {
    let scrolled = -handle.offset().x;
    let max = handle.max_offset().x;
    (scrolled > px(0.5), scrolled < max - px(0.5))
}

impl Shell {
    /// `strip` (the scrolling row of groups) with its chevrons.
    pub(super) fn ribbon_with_overflow(&mut self, strip: Stateful<Div>) -> AnyElement {
        if self.ribbon_scroll.tab != self.ribbon_tab {
            self.ribbon_scroll.tab = self.ribbon_tab;
            self.ribbon_scroll.handle.set_offset(Point::default());
        }
        let handle = self.ribbon_scroll.handle.clone();
        let shown = hidden(&handle);
        let (left, right) = shown;
        let watch = handle.clone();
        div()
            .relative()
            .w_full()
            .flex_none()
            .child(strip.track_scroll(&handle))
            .when(left, |this| this.child(chevron(&handle, false)))
            .when(right, |this| this.child(chevron(&handle, true)))
            // The overflow is only known once the strip is laid out, and
            // changes with the window's width: draw again whenever it no
            // longer matches the chevrons drawn.
            .child(
                canvas(
                    move |_, window, _| {
                        if hidden(&watch) != shown {
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_0(),
            )
            .into_any_element()
    }
}

fn chevron(handle: &ScrollHandle, forward: bool) -> Stateful<Div> {
    let handle = handle.clone();
    div()
        .id(if forward {
            "ribbon-more-right"
        } else {
            "ribbon-more-left"
        })
        .absolute()
        .top_0()
        .bottom(px(1.))
        .map(|this| {
            if forward {
                this.right_0()
            } else {
                this.left_0()
            }
        })
        .w(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        // Over the strip's edge buttons: a click is the chevron's alone.
        .occlude()
        .bg(tokens::chrome())
        .map(|this| {
            if forward {
                this.border_l_1()
            } else {
                this.border_r_1()
            }
        })
        .border_color(tokens::border())
        .text_color(tokens::text_label())
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
        .tooltip(move |window, cx| super::super::tooltip::text("More buttons", window, cx))
        .child(
            Icon::new(if forward {
                IconName::ChevronRight
            } else {
                IconName::ChevronLeft
            })
            .size(px(14.)),
        )
        .on_click(move |_, window, _| {
            let step = handle.bounds().size.width * 0.6;
            let max = handle.max_offset().x;
            let mut offset = handle.offset();
            let scrolled = if forward {
                (-offset.x + step).min(max)
            } else {
                (-offset.x - step).max(px(0.))
            };
            offset.x = -scrolled;
            handle.set_offset(offset);
            window.refresh();
        })
}
