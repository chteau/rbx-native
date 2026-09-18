//! The shell's own furniture: the document tab row (Row A), the ribbon's
//! category tab row (Row B), the frame every dock panel sits in, and the
//! handles that resize them.
//!
//! Everything here reads [`crate::tokens`] and nothing here invents a
//! colour, radius or spacing step. The two tab rows share one renderer
//! ([`tab`]) on purpose: a tab is a tab, and the only difference between
//! "which document" and "which ribbon page" is what it switches.

use gpui_kit::component::{h_flex, v_flex, Selectable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens::{self, Cast};
use crate::ui_icons;

use super::Shell;

/// Which editor the centre column is showing. The dock used to own this as
/// three tabbed panels; Row A owns it now, so that Explorer, Properties and
/// Output stay mounted no matter which document is open (see
/// `UX_GUIDELINES.md` §7's independence rule).
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
}

/// Which edge is being dragged. One variant per resize handle in the shell;
/// `Shell::dragging` holds at most one at a time.
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

const ROW_A_HEIGHT: Pixels = px(40.);
const ROW_B_HEIGHT: Pixels = px(36.);
/// The draggable width of a resize handle. Wider than the line it draws, so
/// the edge is catchable without being visible (4.3).
const HANDLE_HIT: Pixels = px(4.);
const PANEL_HEADER_HEIGHT: Pixels = px(32.);
/// §7.2's keyboard order, in bands: Row A's document tabs take 0..,
/// Row B's category tabs start here, and the ribbon's own controls follow
/// (see `shell::ribbon`).
const RIBBON_TAB_INDEX: isize = 10;
pub(super) const RIBBON_CONTROL_INDEX: isize = 20;

impl Shell {
    /// **Row A** — which document the centre column shows, plus whatever
    /// controls belong to that document (the viewport's quality dropdown
    /// and its own settings menu).
    pub(super) fn document_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.document;
        let place = self.title.clone();

        h_flex()
            .w_full()
            .h(ROW_A_HEIGHT)
            .flex_none()
            .items_center()
            .justify_between()
            .pl(tokens::SPACE_2)
            .pr(tokens::SPACE_2)
            .bg(tokens::bg_0())
            .child(
                h_flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .children(Document::ALL.map(|document| {
                        tab(
                            ("document-tab", document as usize),
                            document.label(&place),
                            document == active,
                            document as isize,
                            cx.listener(move |shell, _, _, cx| {
                                shell.document = document;
                                cx.notify();
                            }),
                        )
                    })),
            )
            .when(active == Document::Viewport, |this| {
                this.child(
                    h_flex()
                        .items_center()
                        .gap(tokens::SPACE_1)
                        .child(self.quality_control())
                        .child(self.viewport_menu(cx)),
                )
            })
    }

    /// **Row B** — the ribbon's own category tabs (§1). Same renderer as
    /// Row A: the pill/hover/press matrix is one behaviour, not two.
    pub(super) fn ribbon_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.ribbon_tab;

        h_flex()
            .w_full()
            .h(ROW_B_HEIGHT)
            .flex_none()
            .items_center()
            .gap(tokens::SPACE_1)
            .pl(tokens::SPACE_2)
            .bg(tokens::bg_0())
            .children(super::ribbon::Tab::ALL.map(|ribbon_tab| {
                tab(
                    ("ribbon-tab", ribbon_tab as usize),
                    ribbon_tab.label().into(),
                    ribbon_tab == active,
                    RIBBON_TAB_INDEX + ribbon_tab as isize,
                    cx.listener(move |shell, _, _, cx| {
                        shell.ribbon_tab = ribbon_tab;
                        cx.notify();
                    }),
                )
            }))
    }
}

/// One tab, in every state §1.2 asks for.
///
/// The committed (active) state is a pill; hover on an inactive tab is a
/// rounded rectangle. The shape difference is the point: it keeps "I am
/// pointing at this" visually distinct from "this is the one that's open",
/// which a colour change alone doesn't do for someone who reads shape
/// faster than tint.
///
/// The press state paints `bg-3` rather than scaling the tab down: GPUI's
/// style system has no transform, so the spec's `scale(0.98)` has no
/// expression here (see `UX_GUIDELINES.md` §10's deviation list).
fn tab(
    id: impl Into<ElementId>,
    label: SharedString,
    active: bool,
    tab_index: isize,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .flex_none()
        // §7.2: the two tab rows come first in the keyboard order, in the
        // order they are read.
        .tab_index(tab_index)
        .focus(|this| this.shadow(tokens::focus_ring(tokens::bg_0())))
        .cursor_pointer()
        .px(tokens::SPACE_3)
        .py(px(6.))
        .rounded(if active {
            tokens::RADIUS_PILL
        } else {
            tokens::RADIUS_MD
        })
        .text_size(tokens::UI_LABEL_SIZE)
        .line_height(tokens::UI_LABEL_LINE_HEIGHT)
        .font_weight(if active {
            tokens::UI_LABEL_ACTIVE_WEIGHT
        } else {
            tokens::UI_LABEL_WEIGHT
        })
        .text_color(if active {
            tokens::text_primary()
        } else {
            tokens::text_secondary()
        })
        .map(|this| {
            if active {
                this.bg(tokens::accent_soft_bg())
                    .hover(|this| this.bg(tokens::accent_soft_bg_hover()))
            } else {
                this.hover(|this| this.bg(tokens::bg_2()))
            }
        })
        .active(|this| this.bg(tokens::bg_3()))
        .on_click(on_click)
        .child(label)
}

/// The frame a dock panel sits in: its own surface, its top corners
/// rounded (the bottom meets the window edge, so those stay square), and
/// the elevation that lifts it off the viewport. `cast` points away from
/// the viewport — the panel throws its shadow over what it sits beside.
pub(super) fn panel(cast: Cast, radius: Pixels, content: impl IntoElement) -> Div {
    v_flex()
        .size_full()
        .overflow_hidden()
        .bg(tokens::bg_1())
        .rounded_tl(radius)
        .rounded_tr(radius)
        .shadow(tokens::elevation_3(cast))
        .child(content)
}

/// A panel's title strip: its name, and whatever controls it owns.
pub(super) fn panel_header(title: SharedString, suffix: impl IntoElement) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(PANEL_HEADER_HEIGHT)
        .flex_none()
        .items_center()
        .justify_between()
        .px(tokens::SPACE_3)
        .text_size(tokens::UI_LABEL_SIZE)
        .line_height(tokens::UI_LABEL_LINE_HEIGHT)
        .text_color(tokens::text_secondary())
        .child(div().flex_1().truncate().child(title))
        .child(suffix)
}

/// A small square icon button for panel chrome: an overflow "…", a collapse
/// chevron. Icon-only, so §6 requires the tooltip — it doubles as the
/// control's accessible label.
pub(super) fn icon_button(
    id: impl Into<ElementId>,
    icon: &str,
    label: &'static str,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS_SM)
        .cursor_pointer()
        .text_color(tokens::text_secondary())
        .hover(|this| this.bg(tokens::bg_2()).text_color(tokens::text_primary()))
        .active(|this| this.bg(tokens::bg_3()))
        .focus(|this| this.shadow(tokens::focus_ring(tokens::bg_1())))
        .tooltip(move |window, cx| super::tooltip::text(label, window, cx))
        .child(ui_icons::icon(icon).size(px(14.)))
}

/// A resize handle: a hairline at rest, a brighter line while pointed at,
/// and a hit target wide enough to actually catch (4.3).
///
/// The drag itself is instant — no easing, no animation. A handle that
/// eases toward the pointer reads as lag, not polish.
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
                .bg(tokens::border_soft())
                .group_hover("resize-handle", |this| {
                    let thicker = if vertical_edge {
                        this.w(px(2.))
                    } else {
                        this.h(px(2.))
                    };
                    thicker.bg(tokens::border_mid())
                }),
        )
}

/// Wraps any element so it can be a [`Popover`](gpui_kit::component::popover::Popover)
/// trigger.
///
/// The toolkit hands a trigger its own open state through `Selectable`,
/// which is exactly §5.3's "Open (menu currently showing)" row: a trigger
/// whose menu is showing paints `bg-3`, so the button stays visibly held
/// down for as long as what it opened is on screen.
#[derive(IntoElement)]
pub(super) struct Trigger {
    element: Stateful<Div>,
    open: bool,
}

impl Trigger {
    pub(super) fn new(element: Stateful<Div>) -> Self {
        Self {
            element,
            open: false,
        }
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
        self.element
            .when(self.open, |this| this.bg(crate::tokens::bg_3()))
    }
}
