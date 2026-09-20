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
    StyleEditor,
}

impl Document {
    pub(super) const ALL: [Document; 3] =
        [Document::Viewport, Document::Scripts, Document::StyleEditor];

    fn label(self, place: &SharedString) -> SharedString {
        match self {
            Document::Viewport => place.clone(),
            Document::Scripts => "Script Editor".into(),
            Document::StyleEditor => "Style Editor".into(),
        }
    }

    fn icon(self) -> IconName {
        match self {
            Document::Viewport => IconName::Globe,
            Document::Scripts => IconName::Code,
            Document::StyleEditor => IconName::Palette,
        }
    }
}

/// Which edge is being dragged. One variant per resize handle in the shell;
/// `Shell::drag` holds at most one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Between the Properties column and the centre column.
    Properties,
    /// Between the centre column and the Explorer column.
    Explorer,
    /// Between the document and the Output dock beneath it.
    Output,
}

impl Handle {
    fn is_vertical_edge(self) -> bool {
        !matches!(self, Handle::Output)
    }
}

/// A drag in progress: where the pointer went down, and how big the panel
/// was at that moment. Both are needed — tracking only the delta since the
/// last move accumulates rounding drift over a long drag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drag {
    pub(crate) handle: Handle,
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
        let title = SharedString::from(format!("{} - RbxNative", self.title));

        h_flex()
            .w_full()
            .h(tokens::topbar_height())
            .flex_none()
            .items_center()
            .bg(tokens::black())
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
                    .text_color(tokens::text_full())
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
                    .child(div().truncate().child(title)),
            )
            .child(
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
                        |window| window.minimize_window(),
                    ))
                    .child(window_button(
                        &self.tab_order.claim(cx),
                        "window-maximize",
                        IconName::Square,
                        "Maximize",
                        |window| window.zoom_window(),
                    ))
                    .child(window_button(
                        &self.tab_order.claim(cx),
                        "window-close",
                        IconName::X,
                        "Close",
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
            .items_stretch()
            .bg(tokens::chrome())
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
                                    shell.document = document;
                                    cx.notify();
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
            .items_stretch()
            .bg(tokens::dock())
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
                            .h_full()
                            .items_center()
                            .px(px(10.))
                            .cursor_pointer()
                            .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text_full())
                            .map(|this| {
                                if ribbon_tab == active {
                                    this.bg(tokens::ribbon_tab_active())
                                } else {
                                    this.hover(|this| this.bg(tokens::hover()))
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

/// One document tab: icon, title, close. Fixed width — these do not grow to
/// fit their titles, which is what keeps the strip from reflowing every
/// time a place with a longer name is opened.
fn document_tab(
    document: Document,
    label: SharedString,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(("document-tab", document as usize))
        .flex_none()
        .w(tokens::tab_width())
        .h_full()
        .items_center()
        .gap(px(10.))
        .px(px(16.))
        .border_r(px(1.))
        .border_color(tokens::tab_border())
        .relative()
        .cursor_pointer()
        .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(if active {
            tokens::text_full()
        } else {
            tokens::text_label()
        })
        .map(|this| {
            if active {
                this.bg(tokens::tab_active())
            } else {
                this.hover(|this| this.bg(tokens::hover()))
            }
        })
        .on_click(on_click)
        .when(active, |this| {
            this.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(tokens::tab_active_bar()),
            )
        })
        .child(Icon::new(document.icon()).size(tokens::text_md()))
        .child(div().flex_1().truncate().child(label))
    // No close mark. The frame draws one, and §11 already rejects it on
    // a dock tab as "a control that lies" — a document here is a view
    // of the one open place, not a file that closes independently, so
    // the identical argument applies.
}

/// One window button. Three of these fill the block the title is centred
/// against, so each is exactly a third of it.
fn window_button(
    focus: &FocusHandle,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    action: fn(&mut Window),
) -> impl IntoElement {
    titlebar_button(Some(focus), id, icon, label, move |_, window, _| {
        action(window)
    })
}

fn titlebar_button(
    focus: Option<&FocusHandle>,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
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
        .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
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
                    on_close,
                )),
        )
}

/// A dock's tab strip: the panel it holds, then the button that owns the
/// dock's own settings.
///
/// The frame puts a "+" in that second slot. This editor's docks hold one
/// panel each and always will — Explorer is the Explorer — so the slot
/// carries the panel's overflow menu instead: same cell, same divider, but
/// a button with somewhere to go. For the same reason the tab has no close
/// "×": the frame draws one, and here it would be a control that lies.
pub(super) fn dock_tabs(title: SharedString, trailing: Option<AnyElement>) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(tokens::dock_tabs_height())
        .flex_none()
        .items_stretch()
        .p(px(5.))
        .gap(px(4.))
        .child(
            h_flex()
                // Hugs its own title rather than filling the strip: the
                // frame's tab is a pill on the dock's black ground, and a
                // full-width bar would read as a header instead.
                .flex_none()
                .max_w(px(tokens::dock_width() - 60.))
                .items_center()
                .gap(px(7.))
                .px(px(6.))
                .rounded(tokens::RADIUS)
                .bg(tokens::chrome())
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text_strong())
                .child(div().truncate().child(title)),
        )
        .when_some(trailing, |this, trailing| {
            this.child(
                h_flex()
                    // The frame's cell is a fixed 32px around one "+".
                    // Output's strip carries a filter row as well, so this
                    // is a floor rather than a width: the divider lands
                    // where the design puts it, and a wider set of controls
                    // grows away from it instead of being clipped by it.
                    .flex_none()
                    .min_w(tokens::dock_tab_add_width())
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .pl(px(4.))
                    .border_l(px(1.))
                    .border_color(tokens::divider())
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
                this.bg(tokens::ribbon_tab_active())
                    .text_color(tokens::text_full())
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
    handle: Handle,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let vertical_edge = handle.is_vertical_edge();

    div()
        .id(match handle {
            Handle::Properties => "handle-properties",
            Handle::Explorer => "handle-explorer",
            Handle::Output => "handle-output",
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
                .group_hover("resize-handle", |this| this.bg(tokens::divider())),
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
    element: Stateful<Div>,
    open: bool,
    accent: Option<Rgba>,
}

impl Trigger {
    pub(super) fn new(element: Stateful<Div>) -> Self {
        Self {
            element,
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
        self.element.when(self.open, |this| match accent {
            Some(accent) => super::ribbon::selected(this, accent),
            None => this.bg(tokens::ribbon_tab_active()),
        })
    }
}
