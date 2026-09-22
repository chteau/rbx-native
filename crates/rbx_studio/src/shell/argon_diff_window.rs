//! The Argon review prompt's "Diff" detail view: a second window, the same
//! shape as the `NumberSequence`/`ColorSequence` graph (`crate::
//! sequence_window`) — its own title bar, fixed size, floats above the
//! editor it belongs to.
//!
//! **This window holds no copy of the batch.** Every frame it reads
//! [`Shell::argon_diff_rows`] straight off whatever review is currently
//! pending, so Accept/Cancel from the dock — still the only place either
//! button lives, this window is read-only — is reflected here immediately.
//! Once nothing is pending any more (the review resolved, or the debug var
//! that can seed one, see [`super::argon_sync::DIFF_VARIABLE`], never ran)
//! there is nothing left to show, so the window closes itself rather than
//! sit there empty.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, Icon, Root, Sizable as _};
use gpui_kit::*;

use crate::tokens;

use super::argon_sync::{DiffRow, DiffRowKind, PropertyDiff};
use super::Shell;

/// Wide enough for a property row's "before → after" pair without wrapping
/// for anything but a long `Content`/`Source` value; tall enough for a
/// handful of rows before the list itself takes over scrolling.
const CONTENT_WIDTH: f32 = 420.0;
const CONTENT_HEIGHT: f32 = 420.0;
/// What the toolkit's client-side window frame takes off every side before
/// the content gets any — see `sequence_window`'s own constant of the same
/// name for why this is spelled here rather than read off the toolkit.
const WINDOW_CHROME: f32 = 40.0;

pub(crate) struct ArgonDiffWindow {
    shell: Entity<Shell>,
    grab: Rc<Cell<bool>>,
    scroll: ScrollHandle,
}

impl ArgonDiffWindow {
    pub(super) fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
        let window_size = size(
            tokens::scaled_width(CONTENT_WIDTH + WINDOW_CHROME),
            tokens::scaled_width(CONTENT_HEIGHT + WINDOW_CHROME),
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                window_size,
                cx,
            ))),
            kind: WindowKind::Floating,
            is_resizable: false,
            is_minimizable: false,
            window_min_size: Some(window_size),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("Argon Diff")),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            let view = cx.new(|_| ArgonDiffWindow {
                shell,
                grab: Rc::new(Cell::new(false)),
                scroll: ScrollHandle::new(),
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }
}

impl Render for ArgonDiffWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.shell.read(cx).argon_diff_rows();
        if rows.is_empty() {
            // Nothing pending any more — a window showing an empty review
            // is worse than no window.
            window.remove_window();
            return div().into_any_element();
        }

        let row_elements: Vec<AnyElement> = rows.iter().map(diff_row).collect();

        v_flex()
            .id("argon-diff-window")
            .size_full()
            .bg(tokens::chrome())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text_strong())
            .child(super::chrome::panel_topbar(
                SharedString::from("Argon Diff"),
                self.grab.clone(),
                |_, window: &mut Window, _| window.remove_window(),
            ))
            .child(
                div()
                    .id("argon-diff-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(tokens::group_gap())
                    .child(
                        v_flex()
                            .gap(tokens::group_gap())
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .children(row_elements),
                    )
                    .vertical_scrollbar(&self.scroll),
            )
            .into_any_element()
    }
}

fn diff_row(row: &DiffRow) -> AnyElement {
    let (icon, color) = match row.kind {
        DiffRowKind::Addition => (IconName::Plus, tokens::text_strong()),
        DiffRowKind::Update => (IconName::Pencil, tokens::text_label()),
        DiffRowKind::Removal => (IconName::Minus, tokens::text_error()),
    };
    let title = if row.nested > 0 {
        format!("{} ({}) — +{} nested", row.name, row.class, row.nested)
    } else {
        format!("{} ({})", row.name, row.class)
    };

    v_flex()
        .gap(px(2.))
        .child(
            h_flex()
                .items_center()
                .gap(tokens::label_gap())
                .child(Icon::new(icon).small().text_color(color))
                .child(div().text_color(color).child(title)),
        )
        .children(row.properties.iter().map(property_diff))
        .into_any_element()
}

fn property_diff(property: &PropertyDiff) -> impl IntoElement {
    let value = match &property.before {
        Some(before) => format!("{}: {} → {}", property.name, before, property.after),
        None => format!("{}: {}", property.name, property.after),
    };
    div()
        .pl(tokens::group_gap() + px(16.))
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .text_color(tokens::text_muted())
        .child(value)
}
