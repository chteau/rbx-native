//! The window's own furniture, top to bottom: the title bar this editor
//! draws instead of the one the window manager would (`topbar`), the
//! document tab strip (`document_tabs`), the tab strip every dock wears
//! (`dock_tabs`), and the handles that resize the columns.
//!
//! Everything here reads [`crate::tokens`] and nothing here invents a
//! colour, radius or size: the geometry is the `RbxNative - Studio App`
//! frame's, measured rather than guessed.
//!
//! Chrome icons are Lucide, straight out of the toolkit's bundled catalog —
//! the same set the design is drawn with. The Explorer is the exception and
//! keeps this project's own class icons, because there the icon *is* the
//! class (see [`crate::class_icons`]).

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon, Selectable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::layout::Edge;
use super::Shell;

/// The mark at the top-left, exported from the same Figma file as the rest
/// of this frame. GPUI paints an SVG as a mask, so this renders as a white
/// silhouette of the logo rather than with its own fills — which is what
/// the frame shows anyway.
const LOGO: &[u8] = include_bytes!("../../../../assets/icons/brand/rbxnative-logo.svg");

/// Which editor the centre column is showing. Row A owns this, so that
/// Explorer, Properties and Output stay mounted no matter which document is
/// open (see `UX_GUIDELINES.md`'s independence rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Document {
    #[default]
    Viewport,
    Scripts,
    /// The 2D canvas and the style sheets — see `shell::ui_editor`.
    UiEditor,
}

impl Document {
    pub(super) const ALL: [Document; 3] =
        [Document::Viewport, Document::Scripts, Document::UiEditor];

    fn label(self, place: &SharedString) -> SharedString {
        match self {
            Document::Viewport => place.clone(),
            Document::Scripts => "Script Editor".into(),
            Document::UiEditor => "UI Editor".into(),
        }
    }

    fn icon(self) -> IconName {
        match self {
            Document::Viewport => IconName::Globe,
            Document::Scripts => IconName::Code,
            Document::UiEditor => IconName::LayoutDashboard,
        }
    }
}

/// A drag in progress: which edge, where the pointer went down, and how
/// big that edge was at that moment. The last two are both needed —
/// tracking only the delta since the previous move accumulates rounding
/// drift over a long drag.
///
/// The edge is a `layout::Edge` rather than a variant per handle: a handle
/// is now "the seam beside this edge", and which panel is behind it is a
/// question for the layout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drag {
    pub(crate) edge: Edge,
    pub(crate) origin: Pixels,
    pub(crate) size: Pixels,
}

/// The draggable width of a resize handle. The frame draws no divider at
/// all between a dock and what it sits beside, so this one is invisible
/// until pointed at — but it is still four pixels wide, because an edge you
/// have to hit to the pixel is an edge you miss.
const HANDLE_HIT: Pixels = px(4.);
/// The window buttons, and the logo block mirroring them on the other side
/// so that what sits between the two is centred on the *window* rather than
/// on whatever is left over.
const WINDOW_ACTIONS_WIDTH: Pixels = px(136.);

impl Shell {
    /// The title bar. This editor asks for client-side decorations (see
    /// `main::window_options`), so this row *is* the window's title bar:
    /// dragging it moves the window, and the three buttons on the right are
    /// the only minimize/maximize/close there are.
    pub(super) fn topbar(&self, cx: &mut App) -> impl IntoElement {
        let title = self.title.clone();

        topbar_frame(title).child(
            h_flex()
                .flex_none()
                .w(WINDOW_ACTIONS_WIDTH)
                .h_full()
                .justify_end()
                .child(window_button(
                    &self.tab_order.claim(cx),
                    "window-minimize",
                    IconName::Minus,
                    "Minimize",
                    false,
                    |window| window.minimize_window(),
                ))
                .child(window_button(
                    &self.tab_order.claim(cx),
                    "window-maximize",
                    IconName::Square,
                    "Maximize",
                    false,
                    |window| window.zoom_window(),
                ))
                .child(window_button(
                    &self.tab_order.claim(cx),
                    "window-close",
                    IconName::X,
                    "Close",
                    true,
                    |window| window.remove_window(),
                )),
        )
    }

    /// **Row A** — which document the centre column shows.
    ///
    /// One Tab stop for the whole strip, arrows within it, Enter/Space to
    /// switch: the APG Tabs pattern with *manual* activation, so browsing
    /// the strip with the arrows doesn't tear down and rebuild an editor on
    /// the way past (see `shell::roving`).
    pub(super) fn document_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.document;
        let place = self.title.clone();
        self.document_nav
            .begin(&self.tab_order, Some(Document::ALL.len()), cx);

        h_flex()
            .w_full()
            .h(tokens::tabs_height())
            .flex_none()
            .items_end()
            .gap(px(2.))
            .pt(px(8.))
            .px(px(12.))
            .bg(tokens::black())
            .border_b_1()
            .border_color(tokens::border())
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.document_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(
                Document::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, document)| {
                        self.document_nav.item(
                            index,
                            document_tab(
                                document,
                                document.label(&place),
                                document == active,
                                cx.listener(move |shell, _, _, cx| {
                                    shell.set_document(document, cx);
                                }),
                            ),
                            cx,
                        )
                    }),
            )
            // The frame's "+" opens a new document, which this editor has no
            // notion of — its three documents are fixed. Left visibly
            // disabled rather than removed: the strip's shape is part of the
            // design, and a button that says "not yet" beats one that lies.
            .child(
                h_flex()
                    .id("document-tab-add")
                    .flex_none()
                    .w(tokens::tab_add_width())
                    .h_full()
                    .items_center()
                    .justify_center()
                    .cursor_not_allowed()
                    .text_color(tokens::text_disabled())
                    .tooltip(|window, cx| {
                        super::tooltip::text("New document — not implemented", window, cx)
                    })
                    .child(Icon::new(IconName::Plus).size(px(10.))),
            )
    }

    /// **Row B** — the ribbon's category tabs, in the black strip above it.
    pub(super) fn ribbon_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.ribbon_tab;
        self.ribbon_tabs_nav
            .begin(&self.tab_order, Some(super::ribbon::Tab::ALL.len()), cx);

        h_flex()
            .w_full()
            .h(tokens::ribbon_tabs_height())
            .flex_none()
            .items_center()
            .gap(px(22.))
            .px(px(16.))
            .bg(tokens::black())
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.ribbon_tabs_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(
                super::ribbon::Tab::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, ribbon_tab)| {
                        let tab = h_flex()
                            .id(("ribbon-tab", ribbon_tab as usize))
                            .flex_none()
                            .cursor_pointer()
                            .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .map(|this| {
                                if ribbon_tab == active {
                                    this.text_color(tokens::check_on())
                                        .font_weight(tokens::WEIGHT_BOLD)
                                } else {
                                    this.text_color(tokens::text_placeholder())
                                        .hover(|this| this.text_color(tokens::text_full()))
                                }
                            })
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.ribbon_tab = ribbon_tab;
                                cx.notify();
                            }))
                            .child(ribbon_tab.label());

                        self.ribbon_tabs_nav.item(index, tab, cx)
                    }),
            )
    }
}

/// One document tab: icon, title, close. Sized to its own content, not to a
/// fixed slot — the reference floats each tab's label at its own width, and
/// three fixed documents never reflow the strip the way an arbitrary count
/// of them might.
fn document_tab(
    document: Document,
    label: SharedString,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(("document-tab", document as usize))
        .flex_none()
        .items_center()
        .gap(px(7.))
        .px(px(16.))
        .py(px(8.))
        .rounded_t(tokens::RADIUS_TILE)
        .relative()
        .cursor_pointer()
        .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(if active {
            tokens::text_full()
        } else {
            tokens::text_placeholder()
        })
        .when(active, |this| {
            this.bg(tokens::dock())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .bottom(px(-1.))
                        .h(px(2.))
                        .bg(tokens::tab_active_bar()),
                )
        })
        .when(!active, |this| {
            this.hover(|this| this.bg(tokens::hover_subtle()))
        })
        .on_click(on_click)
        .child(Icon::new(document.icon()).size(px(13.)))
        .child(div().truncate().child(label))
    // No close mark. The frame draws one, and §11 already rejects it on
    // a dock tab as "a control that lies" — a document here is a view
    // of the one open place, not a file that closes independently, so
    // the identical argument applies.
}

/// The title bar every window of this app wears: logo, "RbxNative | title"
/// centred on the window, and the drag stretch that moves it (double-click
/// maximises). The caller adds the window buttons at the right.
pub(crate) fn topbar_frame(title: SharedString) -> Div {
    h_flex()
        .w_full()
        .h(tokens::topbar_height())
        .flex_none()
        .items_center()
        .bg(tokens::black())
        .border_b(px(1.))
        .border_color(tokens::border())
        .child(
            h_flex()
                .flex_none()
                .w(WINDOW_ACTIONS_WIDTH)
                .items_center()
                .pl(px(12.))
                .child(
                    Icon::empty()
                        .data(LOGO)
                        .w(px(23.8))
                        .h(px(17.))
                        .text_color(tokens::text_full()),
                ),
        )
        // Only this middle stretch moves the window: the logo block and
        // the buttons flanking it are not drag targets, so a click that
        // lands on a button can never also nudge the window.
        .child(
            h_flex()
                .id("window-drag")
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .text_color(tokens::text2())
                .gap(px(10.))
                // The move starts on a *drag*, not on a press. Handing
                // `start_window_move` to mouse-down grabs the pointer at
                // the compositor on the first press of every double
                // click, and the second press is then never delivered —
                // which is exactly why double-clicking to maximize did
                // nothing until the second attempt. Waiting for actual
                // motion costs nothing (a drag always moves) and leaves
                // a stationary double click intact.
                //
                // `zoom_window` is the platform's own call
                // (`PlatformWindow::zoom`), so this is macOS's zoom and
                // Windows/Linux's maximize rather than one convention
                // forced on both. It toggles on X11, Wayland and macOS;
                // on Windows it only maximizes, which is GPUI's gap and
                // not something this can paper over.
                .on_mouse_move(|event: &MouseMoveEvent, window, _| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        window.start_window_move();
                    }
                })
                .on_click(|event, window, _| {
                    if event.click_count() >= 2 {
                        window.zoom_window();
                    }
                })
                .child(
                    div()
                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                        .child("RbxNative"),
                )
                .child(div().w(px(1.)).h(px(12.)).bg(tokens::border2()))
                .child(div().truncate().child(title)),
        )
}

/// [`topbar_frame`] with minimise, maximise and close — for a window with
/// no tab order of its own (the launcher's). A fixed-size window leaves
/// maximise out (`resizable`); `on_close` decides what closing means.
pub(crate) fn window_topbar(
    title: SharedString,
    resizable: bool,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    topbar_frame(title).child(
        h_flex()
            .flex_none()
            .w(WINDOW_ACTIONS_WIDTH)
            .h_full()
            .justify_end()
            .child(titlebar_button(
                None,
                "window-minimize",
                IconName::Minus,
                "Minimize",
                false,
                |_, window, _| window.minimize_window(),
            ))
            .when(resizable, |this| {
                this.child(titlebar_button(
                    None,
                    "window-maximize",
                    IconName::Square,
                    "Maximize",
                    false,
                    |_, window, _| window.zoom_window(),
                ))
            })
            .child(titlebar_button(
                None,
                "window-close",
                IconName::X,
                "Close",
                true,
                move |_, window, cx| on_close(window, cx),
            )),
    )
}

/// One window button. Three of these fill the block the title is centred
/// against, so each is exactly a third of it.
fn window_button(
    focus: &FocusHandle,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    danger: bool,
    action: fn(&mut Window),
) -> impl IntoElement {
    titlebar_button(Some(focus), id, icon, label, danger, move |_, window, _| {
        action(window)
    })
}

fn titlebar_button(
    focus: Option<&FocusHandle>,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    danger: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover_bg = if danger {
        tokens::danger_hover()
    } else {
        tokens::hover()
    };
    h_flex()
        .id(id)
        .flex_none()
        .w(WINDOW_ACTIONS_WIDTH / 3.)
        .h_full()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when_some(focus, |this, focus| this.track_focus(focus))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::black())))
        .text_color(tokens::text_label())
        .hover(move |this| this.bg(hover_bg).text_color(tokens::text_full()))
        .active(|this| this.bg(tokens::ribbon_tab_active()))
        .tooltip(move |window, cx| super::tooltip::text(label, window, cx))
        .on_click(on_click)
        .child(Icon::new(icon).size(tokens::text_md()))
}

/// The title bar a **secondary window** wears — today the sequence graph
/// (`crate::sequence_window`), which is its own fixed-size floating window
/// rather than a dock.
///
/// Built from the same pieces [`Shell::topbar`] is rather than a dialog
/// header of its own: same height, same ground, the same logo block, the
/// same button treatment and the same drag-to-move stretch, so a second
/// window of this application reads as one. The difference is what such a
/// window actually has — one button, because it only closes, where the main
/// window also minimizes and zooms.
/// `grab` is how the bar tells a *move* it started from a press that
/// started *here*. GPUI hands a mouse-move to whatever the pointer is over,
/// not to whatever took the press, so without it a keypoint dragged up out
/// of the plot crosses this bar and takes the window with it.
pub(crate) fn panel_topbar(
    title: SharedString,
    grab: Rc<Cell<bool>>,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(tokens::topbar_height())
        .flex_none()
        .items_center()
        .bg(tokens::black())
        .child(
            h_flex()
                .flex_none()
                .w(WINDOW_ACTIONS_WIDTH / 3.)
                .items_center()
                .pl(px(12.))
                .child(
                    Icon::empty()
                        .data(LOGO)
                        .w(px(23.8))
                        .h(px(17.))
                        .text_color(tokens::text_full()),
                ),
        )
        .child(
            h_flex()
                .id("panel-drag")
                .flex_1()
                .h_full()
                .items_center()
                .overflow_hidden()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .text_color(tokens::text_full())
                .on_mouse_down(MouseButton::Left, {
                    let grab = grab.clone();
                    move |_, _, _| grab.set(true)
                })
                // Dragged, not pressed, for the reason `Shell::topbar`'s
                // own stretch spells out: grabbing the pointer on mouse-down
                // eats the second press of a double click.
                .on_mouse_move(move |event: &MouseMoveEvent, window, _| {
                    if grab.get() && event.pressed_button == Some(MouseButton::Left) {
                        window.start_window_move();
                    }
                })
                .child(div().truncate().child(title)),
        )
        .child(
            h_flex()
                .flex_none()
                .w(WINDOW_ACTIONS_WIDTH / 3.)
                .h_full()
                .justify_end()
                .child(titlebar_button(
                    None,
                    "panel-close",
                    IconName::X,
                    "Close",
                    true,
                    on_close,
                )),
        )
}

/// One tab in a dock's strip: the frame's pill, dimmed when it is not the
/// one showing.
///
/// Unwired on purpose — the caller adds the id, the click that shows it and
/// the drag that moves it (see `shell::workspace`), because a tab is the
/// grab handle for its whole panel and only the caller knows which panel
/// that is.
pub(super) fn dock_tab(
    id: &'static str,
    title: SharedString,
    selected: bool,
    solo: bool,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    tab_pill(id, title, selected, solo).child(
        // Only on the tab that is showing: a strip of two tabs with a
        // cross on each reads as two buttons rather than as one dock,
        // and the tab you are looking at is the one you would close.
        div()
            .id(SharedString::from(format!("dock-close-{id}")))
            .flex_none()
            .when(!selected, |this| this.invisible())
            .cursor_pointer()
            .text_color(tokens::text_label())
            .hover(|this| this.text_color(tokens::text_full()))
            .on_click(on_close)
            .child(Icon::new(IconName::X).size(tokens::text_xs())),
    )
}

/// A dock tab's pill alone, with no close mark: what a panel's own sub-tabs
/// wear (see `shell::ui_editor`), which switch a view rather than shut one.
///
/// `solo` is the only tab its strip has — Properties and Explorer, almost
/// always — and reads the way the reference's plain `DockHeader` does: bold
/// title text on the dock's own ground, no pill fill, nothing to say
/// "selected" because there is nothing beside it to be selected over. A
/// real choice (Output/Argon/Wally) keeps the pill so the active one is
/// still findable at a glance.
pub(super) fn tab_pill(
    id: &'static str,
    title: SharedString,
    selected: bool,
    solo: bool,
) -> Stateful<Div> {
    h_flex()
        // Keyed by the panel rather than by its label: the Properties tab
        // is named after the selected instance, and an element whose id
        // changes on every selection is a new element every time.
        .id(SharedString::from(format!("dock-tab-{id}")))
        // Hugs its own title rather than filling the strip: the frame's
        // tab is a pill on the dock's black ground, and a full-width bar
        // would read as a header instead.
        .flex_none()
        .max_w(px(tokens::dock_width() - 60.))
        .items_center()
        .gap(px(7.))
        .px(px(14.))
        .py(px(6.))
        .rounded_t(tokens::RADIUS)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .cursor_pointer()
        .map(|this| {
            if solo {
                this.text_color(tokens::text_strong())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .hover(|this| this.bg(tokens::hover()))
            } else if selected {
                this.bg(tokens::field_select())
                    .text_color(tokens::text_strong())
                    .font_weight(tokens::WEIGHT_SEMIBOLD)
            } else {
                // An unselected tab keeps the dock's own ground rather than
                // a second fill: two pills side by side in different greys
                // read as two docks, not as one dock's two tabs.
                this.text_color(tokens::text_muted())
                    .hover(|this| this.bg(tokens::hover()))
            }
        })
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .child(div().truncate().child(title))
}

/// The strip a dock's tabs sit in, with its own trailing cell for an
/// overflow menu — or, with `toolbar`, for a row of controls that takes the
/// rest of the strip (see [`super::layout::Panel::has_toolbar`]).
pub(super) fn dock_strip(
    tabs: Vec<AnyElement>,
    trailing: Option<AnyElement>,
    toolbar: bool,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(tokens::dock_tabs_height())
        .flex_none()
        .items_center()
        .p(px(5.))
        .gap(px(4.))
        // The reference's Output toolbar row closes with a hairline; a strip
        // that is only tabs draws none, the same as its Properties header.
        .when(toolbar, |this| {
            this.border_b_1().border_color(tokens::border())
        })
        .children(tabs)
        .when_some(trailing, |this, trailing| {
            this.child(
                h_flex()
                    // The frame's cell is a fixed 32px around one "+", so
                    // a lone button sits centred in that floor. A toolbar
                    // (Output's filter row) takes the rest of the strip
                    // instead, never shrinking below what it holds, so its
                    // tools can push themselves to the far end; the divider
                    // lands where the design puts it either way.
                    .map(|this| {
                        if toolbar {
                            this.flex_grow_1().flex_shrink_0()
                        } else {
                            this.flex_none().justify_center()
                        }
                    })
                    .min_w(tokens::dock_tab_add_width())
                    .items_center()
                    .gap(px(4.))
                    .pl(px(4.))
                    .border_l(px(1.))
                    .border_color(tokens::border())
                    .child(trailing),
            )
        })
}

/// A dock's body: the frame's 5px inset around whatever the panel draws.
pub(super) fn dock_content(content: impl IntoElement) -> Div {
    v_flex()
        .flex_1()
        .overflow_hidden()
        .p(px(5.))
        .gap(px(10.))
        .child(content)
}

/// A dock's tab-row overflow: 28×28, square-bottomed so it sits in the
/// strip like a tab, `icon` at `icon_size` in `text3`.
pub(super) fn dock_options_button(
    id: &'static str,
    icon: IconName,
    icon_size: f32,
    label: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_t(tokens::RADIUS)
        .cursor_pointer()
        .text_color(tokens::text3())
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .tooltip(move |window, cx| super::tooltip::text(label, window, cx))
        .child(Icon::new(icon).size(px(icon_size)))
}

/// A small icon button for panel chrome: an overflow "…", a collapse
/// chevron. Icon-only, so it carries a tooltip — which doubles as the
/// control's accessible label.
pub(super) fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex_none()
        // WCAG 2.5.8's 24x24 floor. The glyph inside is smaller; the
        // *target* is not, which is the distinction the criterion draws.
        .size(tokens::hit_target())
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .cursor_pointer()
        .text_color(tokens::text_label())
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
        .active(|this| this.bg(tokens::ribbon_tab_active()))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::black())))
        .tooltip(move |window, cx| super::tooltip::text(label, window, cx))
        .child(Icon::new(icon).size(tokens::text_md()))
}

/// A text button, in the same language as the editor's fields: a filled
/// surface, the one radius, no border.
///
/// This replaces the toolkit's own `Button`, whose outline style put a
/// 1px-bordered pill next to a row of borderless filled fields and made the
/// Output strip look like it came from a different application. A selected
/// button lights up the way an active ribbon tab does.
pub(super) fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .flex_none()
        .h(tokens::hit_target())
        .items_center()
        .justify_center()
        .px(tokens::input_padding())
        .rounded(tokens::RADIUS)
        .cursor_pointer()
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .map(|this| {
            if selected {
                this.bg(tokens::accent_soft())
                    .text_color(tokens::check_on())
            } else {
                this.bg(tokens::field_select())
                    .text_color(tokens::text_label())
                    .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
            }
        })
        .active(|this| this.bg(tokens::ribbon_tab_active()))
        .child(label.into())
}

/// A resize handle. The frame has none — its docks are fixed at 228px — so
/// this draws nothing until it is pointed at, and then only a hairline.
pub(super) fn resize_handle(
    edge: Edge,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let vertical_edge = edge.is_vertical();

    div()
        .id(match edge {
            Edge::Left => "handle-left",
            Edge::Right => "handle-right",
            Edge::Bottom => "handle-bottom",
        })
        .flex_none()
        .map(|this| {
            if vertical_edge {
                this.w(HANDLE_HIT).h_full().cursor_col_resize()
            } else {
                this.h(HANDLE_HIT).w_full().cursor_row_resize()
            }
        })
        .flex()
        .items_center()
        .justify_center()
        // The handle carries the dock's own surface rather than letting the
        // window's black ground show through. A 4px black slot between a
        // dock and the viewport reads as a crack, not as an edge — the
        // hairline the dock already draws is the edge.
        .bg(tokens::dock())
        .group("resize-handle")
        .on_mouse_down(MouseButton::Left, on_down)
        .child(
            div()
                .map(|this| {
                    if vertical_edge {
                        this.w(px(1.)).h_full()
                    } else {
                        this.h(px(1.)).w_full()
                    }
                })
                .group_hover("resize-handle", |this| this.bg(tokens::border())),
        )
}

/// Wraps any element so it can be a [`Popover`](gpui_kit::component::popover::Popover)
/// trigger.
///
/// The toolkit hands a trigger its own open state through `Selectable`: a
/// trigger whose menu is showing stays visibly held down for as long as
/// what it opened is on screen.
#[derive(IntoElement)]
pub(super) struct Trigger {
    build: Box<dyn FnOnce(bool) -> Stateful<Div>>,
    /// Whether `render` paints the open state itself. A trigger built with
    /// [`Trigger::with_open`] draws its own instead.
    styled: bool,
    open: bool,
    accent: Option<Rgba>,
}

impl Trigger {
    pub(super) fn new(element: Stateful<Div>) -> Self {
        Self {
            build: Box::new(move |_| element),
            styled: true,
            open: false,
            accent: None,
        }
    }

    /// A trigger whose children depend on whether its popover is open —
    /// the snap pills, which each take an accent border while the popover
    /// shows. `build` gets the open state and draws everything itself;
    /// `render` adds no styling of its own.
    pub(super) fn with_open(build: impl FnOnce(bool) -> Stateful<Div> + 'static) -> Self {
        Self {
            build: Box::new(build),
            styled: false,
            open: false,
            accent: None,
        }
    }

    /// Paints this trigger with a transform tool's own pastel while its
    /// popover is showing, wash-border-and-icon exactly as an active tool
    /// button — for the ribbon buttons that *are* tools but whose state
    /// lives in a popover rather than in `Transform::tool`.
    pub(super) fn accent(mut self, accent: Rgba) -> Self {
        self.accent = Some(accent);
        self
    }
}

impl Selectable for Trigger {
    fn selected(mut self, selected: bool) -> Self {
        self.open = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.open
    }
}

impl RenderOnce for Trigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let accent = self.accent;
        (self.build)(self.open).when(self.open && self.styled, |this| match accent {
            Some(accent) => super::ribbon::selected(this, accent),
            None => this
                .bg(tokens::accent_soft())
                .text_color(tokens::check_on()),
        })
    }
}
