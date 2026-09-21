//! The `+` on an Explorer row, and the class list it opens.
//!
//! Real Studio's own shape, from `studio/explorer.md`: "you can select from
//! a full array of objects by hovering over the intended parent and clicking
//! the ⊕ button (shortcut of Ctrl+I)", with the two insertion preferences
//! behind a `⋯` beside the search field.
//!
//! What this adds over Studio is that a class the hovered parent cannot take
//! is **greyed rather than missing** — see `explorer::insert` for the two
//! refusals that rule is built from, and why it is only those two.
//!
//! Every row carries the class's own identity icon, resolved through the
//! same `explorer::resolve_icon` the tree's rows use: a class here and an
//! instance of it there are the same thing, and two lookups would be two
//! chances to disagree.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer::insert::{self, Choice};
use crate::explorer::{resolve_icon, ClassIcon};
use crate::tokens;

use super::super::menu::{self, MenuId};
use super::super::{chrome, rows, tooltip};
use super::Shell;

/// One open picker: which row's `+` opened it, and what has been typed.
pub(super) struct Picker {
    parent: Ref,
    query: Entity<InputState>,
    scroll: ScrollHandle,
    /// Kept alive only to stay subscribed — the list has to repaint as the
    /// query is typed.
    _subscription: Subscription,
}

const WIDTH: f32 = 240.;
const LIST_HEIGHT: f32 = 260.;
/// How many matches the list paints at once. Every class the dump names is
/// a candidate, so an empty query matches several hundred; painting them all
/// costs a frame to say nothing the search field cannot say faster.
const MAX_ROWS: usize = 200;

impl Shell {
    /// Opens the picker under `parent`, closing whatever else was open. The
    /// caret starts in the search field: the list is long enough that typing
    /// is the way through it, and an extra click to reach the box would be
    /// in the way every single time.
    pub(crate) fn open_insert_picker(
        &mut self,
        parent: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `window` is what `InputState::new` needs; the caret itself lands a
        // frame later (see `Shell::focus_explorer_edit`).
        if self.dom.get(parent).is_none() {
            return;
        }
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription = cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        self.explorer_edit.focus_next = Some(query.clone());

        self.explorer_edit.menu = None;
        self.explorer_edit.renaming = None;
        self.explorer_edit.picker = Some(Picker {
            parent,
            query,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Ctrl+I: the same picker, on the selected row — the keyboard half of
    /// an affordance that is otherwise only reachable by hovering a row.
    pub(in crate::shell) fn open_insert_picker_on_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selected) = self.selected() {
            self.open_insert_picker(selected, window, cx);
        }
    }

    pub(super) fn insert_picker_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.explorer_edit.picker.as_ref()?;
        let parent_class = self
            .dom
            .get(picker.parent)
            .map(|instance| instance.class().to_owned());
        let query = picker.query.read(cx).value().to_string();
        let choices = insert::choices(&self.database, parent_class.as_deref(), &query);
        let hidden = choices.len().saturating_sub(MAX_ROWS);

        // One lookup per painted row, not per class in the dump: the kit's
        // rasterizer memoizes (see `class_icons::icon_tile`), so this is a
        // hash lookup each, and the rows past `MAX_ROWS` cost nothing.
        let pack = self.icon_pack();
        let rows: Vec<AnyElement> = choices
            .iter()
            .take(MAX_ROWS)
            .map(|choice| {
                let icon = resolve_icon(&choice.class, pack);
                class_row(choice, icon, cx)
            })
            .collect();

        let surface = v_flex()
            .id("insert-picker")
            .occlude()
            .w(px(WIDTH))
            .p(px(4.))
            .gap(px(4.))
            .bg(tokens::chrome())
            .rounded(tokens::RADIUS)
            .shadow(tokens::elevation())
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.explorer_edit.picker = None;
                cx.notify();
            }))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        h_flex()
                            .flex_1()
                            .h(tokens::input_height())
                            .items_center()
                            .px(px(8.))
                            .rounded(tokens::RADIUS)
                            .bg(tokens::field_select())
                            .child(
                                Input::new(&picker.query)
                                    .appearance(false)
                                    .with_size(tokens::field_size())
                                    .h_full(),
                            ),
                    )
                    .child(self.insertion_options(cx)),
            )
            .child(
                div()
                    .id("insert-picker-list")
                    .max_h(px(LIST_HEIGHT))
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .child(v_flex().w_full().gap(px(1.)).children(rows))
                    .vertical_scrollbar(&picker.scroll),
            )
            .when(hidden > 0, |this| {
                this.child(
                    div()
                        .px(px(8.))
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_muted())
                        .child(SharedString::from(format!("{hidden} more — keep typing"))),
                )
            });

        Some(
            deferred(
                anchored()
                    .position(self.popup_anchor())
                    .snap_to_window_with_margin(px(8.))
                    .child(surface),
            )
            .into_any_element(),
        )
    }

    /// The `⋯` beside the search field: real Studio's two insertion
    /// preferences, in the place real Studio keeps them.
    fn insertion_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let increment = self.increment_names();
        let expand = self.expand_on_select();
        menu::dropdown(
            self,
            MenuId::InsertOptions,
            chrome::Trigger::new(chrome::icon_button(
                "insert-options",
                IconName::Ellipsis,
                "Insertion options",
            )),
            vec![
                menu::item("Increment names for new instances")
                    .checked(increment)
                    .on_click(move |shell, cx| shell.set_increment_names(!increment, cx)),
                menu::item("Expand hierarchy when selecting")
                    .checked(expand)
                    .on_click(move |shell, cx| shell.set_expand_on_select(!expand, cx)),
            ],
            cx,
        )
        .into_any_element()
    }

    /// Commits one row of the picker: closes it, then inserts through the
    /// exact path the Insert menu and the quick-insert keys already use, so
    /// a picked class costs one undo step and reaches the viewport the same
    /// way every other insert does.
    fn insert_picked(&mut self, class: String, cx: &mut Context<Self>) {
        let Some(picker) = self.explorer_edit.picker.take() else {
            return;
        };
        self.insert_instance_under(Some(picker.parent), &class, cx);
    }

    /// Whether a new instance of a class a sibling already carries the name
    /// of is numbered — see `explorer::insert::incremented_name`.
    pub(in crate::shell) fn increment_names(&self) -> bool {
        self.increment_names
    }

    fn set_increment_names(&mut self, increment: bool, cx: &mut Context<Self>) {
        self.increment_names = increment;
        self.save_settings();
        cx.notify();
    }

    /// Whether inserting, pasting or selecting expands the tree to reveal
    /// the instance — see `Shell::select`.
    pub(in crate::shell) fn expand_on_select(&self) -> bool {
        self.expand_on_select
    }

    fn set_expand_on_select(&mut self, expand: bool, cx: &mut Context<Self>) {
        self.expand_on_select = expand;
        self.save_settings();
        cx.notify();
    }
}

/// The `+` the hovered row carries. Built from the shell's handle rather
/// than through `cx.listener`, because it is assembled inside the tree's own
/// per-row closure, which has an `App` and no `Context<Shell>` (see
/// `ExplorerEdit::row_slots`).
pub(super) fn insert_button(shell: &Entity<Shell>, reference: Ref) -> AnyElement {
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

/// One class in the list, with the same identity icon the Explorer draws
/// for an instance of it. A refused class is greyed — label *and* icon —
/// and inert rather than missing, with the reason on hover: the point of
/// the whole affordance (see this module's own comment).
fn class_row(choice: &Choice, icon: ClassIcon, cx: &mut Context<Shell>) -> AnyElement {
    let class = SharedString::from(choice.class.clone());
    let picked = choice.class.clone();
    let legal = choice.legal;

    h_flex()
        .id(SharedString::from(format!("insert-{class}")))
        .w_full()
        .h(tokens::hit_target())
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .rounded(tokens::RADIUS)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .child(
            div()
                .flex_none()
                .when(!legal, |this| this.opacity(REFUSED_ICON_OPACITY))
                .child(rows::class_icon(icon)),
        )
        .child(class.clone())
        .map(|this| {
            if choice.legal {
                this.cursor_pointer()
                    .text_color(tokens::text_strong())
                    .hover(|this| this.bg(tokens::hover()))
                    .active(|this| this.bg(tokens::selection()))
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.insert_picked(picked.clone(), cx);
                    }))
            } else {
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
                    .tooltip(move |window, cx| {
                        tooltip::text(format!("{class} cannot be created here"), window, cx)
                    })
            }
        })
        .into_any_element()
}
