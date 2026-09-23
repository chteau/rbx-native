//! The Wally dock: a rail (Home / Installed / Updates) beside the page,
//! or a top band over it when the dock is narrow. Home shows the
//! registry's featured packages, or a search's results with a realm
//! switch, a version picker and Add on each; Installed reads the place's
//! `_Index` folders back; Updates lists what the registry has newer.
//! The data and the install path live in `shell::wally_sync`.
//!
//! Shown only while the Script Editor document is up — see
//! `Shell::hidden_panels` — since it means nothing over the 3D view or
//! the UI Editor's canvas.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::chrome;
use super::layout::Panel;
use super::menu::{self, MenuId};
use super::wally_sync::{Page, Remote};
use super::Shell;

mod cards;
mod rail;
mod result;
mod states;
mod updates;

/// Dock width from which the rail sits beside the page.
const WIDE_MIN: f32 = 720.;
/// The rail's width, and the hairline after it.
const RAIL_WIDTH: f32 = 208.;
/// The page's horizontal padding in the wide layout; 14 on all sides
/// otherwise.
const WIDE_PAGE_PADDING_X: f32 = 20.;
const PAGE_PADDING: f32 = 14.;
/// A card column is at least 220 wide plus the 8px gap.
const CARD_MIN: f32 = 228.;
const GRID_GAP: f32 = 8.;
const MAX_COLUMNS: usize = 4;
/// Page content width under which a search result's controls go under
/// its name instead of beside it.
const RESULT_COLUMN_MAX: f32 = 520.;
/// Page content width under which an update's versions go under its name.
const UPDATE_STACK_MAX: f32 = 420.;

/// What one dock width decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Layout {
    /// The rail beside the page; otherwise the band above it.
    pub(super) wide: bool,
    /// Cards per grid row.
    pub(super) columns: usize,
    /// A search result's controls under its name, in two rows.
    pub(super) stack_result: bool,
    /// An update row's versions under its name.
    pub(super) stack_update: bool,
}

/// The layout for a dock `width` pixels wide.
pub(super) fn layout_for(width: f32) -> Layout {
    let wide = width >= WIDE_MIN;
    let content = if wide {
        width - RAIL_WIDTH - 1. - 2. * WIDE_PAGE_PADDING_X
    } else {
        width - 2. * PAGE_PADDING
    };
    Layout {
        wide,
        columns: columns_for(content),
        stack_result: content < RESULT_COLUMN_MAX,
        stack_update: content < UPDATE_STACK_MAX,
    }
}

/// `clamp(floor((content + 8) / 228), 1, 4)`.
pub(super) fn columns_for(content: f32) -> usize {
    (((content + GRID_GAP) / CARD_MIN).floor() as usize).clamp(1, MAX_COLUMNS)
}

/// The dock's own state: the bounds the last frame gave it and the
/// page's scroll.
pub(super) struct WallyDock {
    width: Rc<Cell<f32>>,
    height: Rc<Cell<f32>>,
    scroll: ScrollHandle,
}

impl WallyDock {
    pub(super) fn new(_cx: &mut Context<Shell>) -> Self {
        WallyDock {
            width: Rc::new(Cell::new(WIDE_MIN)),
            height: Rc::new(Cell::new(0.)),
            scroll: ScrollHandle::new(),
        }
    }
}

impl Shell {
    pub(super) fn wally_dock(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            MenuId::WallyOverflow,
            chrome::Trigger::new(chrome::dock_options_button(
                "wally-overflow",
                IconName::Package,
                14.,
                "Wally dock options",
            )),
            self.move_items(Panel::Wally),
            cx,
        );

        let layout = layout_for(self.wally_ui.width.get());
        if self.wally_page() == Page::Home && matches!(self.wally.featured, Remote::Idle) {
            self.wally_load_featured(cx);
        }
        let installed = self.wally_installed(cx);
        let pending = super::wally_sync::updates(&installed, |id| self.wally_listing(id).cloned());
        let counts = rail::Counts {
            installed: installed.len(),
            updates: pending.len(),
        };

        let page = self.page(layout, &installed, &pending, cx);
        let body = if layout.wide {
            h_flex()
                .size_full()
                .items_stretch()
                .child(self.rail(counts, cx))
                .child(div().w(px(1.)).flex_none().bg(crate::tokens::border()))
                .child(page)
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .child(self.band(counts, cx))
                .child(page)
                .into_any_element()
        };

        let width = self.wally_ui.width.clone();
        let height = self.wally_ui.height.clone();
        let shell = cx.entity_id();
        let measured = div()
            .size_full()
            .flex()
            .flex_col()
            // The size this frame gave the body is what the next frame lays
            // out for. When it changed, ask for that next frame now, so a
            // resize settles in one extra pass rather than waiting for the
            // next unrelated redraw.
            .on_children_prepainted(move |bounds, window, cx| {
                if let Some(bounds) = bounds.first() {
                    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    if (w - width.get()).abs() >= 0.5 || (h - height.get()).abs() >= 0.5 {
                        width.set(w);
                        height.set(h);
                        cx.notify(shell);
                        // A floated panel is its own window and doesn't
                        // redraw on the shell's notify; ask it directly, once
                        // this draw is over (a refresh mid-draw is dropped).
                        let handle = window.window_handle();
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, _| window.refresh());
                        });
                    }
                }
            })
            .child(body);

        (
            Some(overflow.into_any_element()),
            Some(measured.into_any_element()),
        )
    }

    /// The page: a fixed header row, then its content in the scroll area.
    fn page(
        &mut self,
        layout: Layout,
        installed: &[super::wally_sync::Installed],
        pending: &[super::wally_sync::Update],
        cx: &mut Context<Self>,
    ) -> Div {
        let (header, content) = match self.wally_page() {
            Page::Home => self.home_page(layout, cx),
            Page::Installed => self.installed_page(layout, installed, cx),
            Page::Updates => self.updates_page(layout, installed, pending, cx),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap(px(10.))
            .py(px(PAGE_PADDING))
            .px(px(if layout.wide {
                WIDE_PAGE_PADDING_X
            } else {
                PAGE_PADDING
            }))
            .child(header)
            .child(
                div()
                    .id("wally-page-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.wally_ui.scroll)
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests;
