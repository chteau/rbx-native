//! The picker's rows, and the `+` that opens it from a tree row.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer::insert::Choice;
use crate::explorer::ClassIcon;
use crate::tokens;

use super::super::super::{chrome, rows, tooltip};
use super::Shell;

/// The `+` the hovered row carries. Built from the shell's handle rather
/// than through `cx.listener`, because it is assembled inside the tree's own
/// per-row closure, which has an `App` and no `Context<Shell>` (see
/// `ExplorerEdit::row_slots`).
pub(in crate::shell) fn insert_button(shell: &Entity<Shell>, reference: Ref) -> AnyElement {
    let shell = shell.clone();
    chrome::icon_button(
        ("explorer-insert", reference.value() as usize),
        IconName::Plus,
        "Insert an object here (Ctrl+I)",
    )
    // The press must not reach the row underneath. Letting it through
    // selects the row, and the toolkit scrolls a freshly selected row into
    // view — which slides this button out from under the pointer between
    // the press and the release, so the click never completes. The picker
    // is told which row it is inserting under anyway, so there is nothing
    // the selection is needed for here.
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .on_click(move |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_insert_picker(reference, window, cx);
        });
    })
    .into_any_element()
}

/// How far a refused row's icon is faded. A kit tile carries its own
/// colours — the colour *is* the identity, so it is never re-tinted (see
/// `UX_GUIDELINES.md` §5) — and a greyed label beside a full-strength icon
/// reads as a half-disabled row. Fading is the one treatment that works on
/// a multi-colour sprite and on a Lucide glyph alike, and this much still
/// leaves the shape readable.
const REFUSED_ICON_OPACITY: f32 = 0.4;

/// A small muted note under the list. Two lines at most: the footer's
/// property names are the point of it, and one line cut them off after the
/// first long one.
pub(super) fn note(text: String) -> AnyElement {
    div()
        .px(px(8.))
        .text_size(tokens::text_xs())
        .text_color(tokens::text_muted())
        .text_ellipsis()
        .line_clamp(2)
        .child(SharedString::from(text))
        .into_any_element()
}

/// The heading over one run of rows.
pub(super) fn caption(text: &'static str) -> AnyElement {
    div()
        .px(px(8.))
        .pt(px(4.))
        .flex_none()
        .text_size(tokens::text_xs())
        .text_color(tokens::text_muted())
        .child(text)
        .into_any_element()
}

/// One class in the list, with the same identity icon the Explorer draws
/// for an instance of it. A refused class is greyed — label *and* icon —
/// and inert rather than missing, with the reason on hover: the point of
/// the whole affordance (see this module's own comment). The highlighted
/// row wears the hover wash, since it is the row a keypress acts on.
pub(super) fn class_row(
    choice: &Choice,
    icon: ClassIcon,
    highlighted: bool,
    refusal: Option<String>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let class = SharedString::from(choice.class.clone());
    let picked = choice.class.clone();
    let hovered = choice.class.clone();

    h_flex()
        .id(SharedString::from(format!("picker-{class}")))
        .w_full()
        .h(tokens::hit_target())
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .rounded(tokens::RADIUS)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .when(highlighted, |this| this.bg(tokens::hover()))
        .on_hover(cx.listener(move |shell, hovering: &bool, _, cx| {
            shell.hover_picked(&hovered, *hovering, cx);
        }))
        .child(
            div()
                .flex_none()
                .when(refusal.is_some(), |this| this.opacity(REFUSED_ICON_OPACITY))
                .child(rows::class_icon(icon)),
        )
        .child(class)
        .map(|this| match refusal {
            None => this
                .cursor_pointer()
                .text_color(tokens::text_strong())
                .hover(|this| this.bg(tokens::hover()))
                .active(|this| this.bg(tokens::selection()))
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.commit_picked(picked.clone(), cx);
                })),
            Some(reason) => this
                .cursor_not_allowed()
                .text_color(tokens::text_disabled())
                .tooltip(move |window, cx| tooltip::text(reason.clone(), window, cx)),
        })
        .into_any_element()
}
