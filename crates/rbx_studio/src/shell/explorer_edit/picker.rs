//! The `+` on an Explorer row, and the class list it opens — which is also
//! the list the row menu's Change Class… opens.
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
//!
//! What a picked class is *for* is the picker's [`Purpose`]; only the list
//! and the commit differ between the two. Changing a class adds suggestions
//! above the list and a footer saying what the highlighted class would cost
//! (see `crate::change_class`).
//!
//! The caret stays in the search field. Up and Down move a highlight through
//! the list, Enter commits it, Escape closes (`Shell::close_explorer_popups`).
//! The arrows are caught at the action layer rather than as keystrokes, for
//! the reason `shell::tree_keys` gives: the field binds them to its own caret.

use gpui_kit::base::input::{MoveDown, MoveUp};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::change_class::{self, Choices};
use crate::explorer::insert;
use crate::explorer::resolve_icon;
use crate::tokens;

use super::Shell;

mod options;
mod row;

pub(super) use row::insert_button;
use row::{caption, class_row, note};

/// One open picker: what it picks a class for, and what has been typed.
pub(super) struct Picker {
    purpose: Purpose,
    query: Entity<InputState>,
    scroll: ScrollHandle,
    /// The row Enter commits, counted through the rows as listed. Back to
    /// the top whenever the query changes, since the rows do.
    highlight: usize,
    /// The class under the pointer, which the footer describes in preference
    /// to the highlight: it is the row being looked at.
    hovered: Option<String>,
    /// Kept alive only to stay subscribed — the list has to repaint as the
    /// query is typed, and Enter arrives as the field's own event.
    _subscription: Subscription,
}

/// What a picked class is for.
pub(super) enum Purpose {
    /// A new child of this instance.
    Insert(Ref),
    /// The new class of each of these.
    ChangeClass(Vec<Ref>),
}

const WIDTH: f32 = 240.;
/// Wider for Change Class, whose footer has property names to fit.
const CHANGE_WIDTH: f32 = 300.;
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
        if self.dom.get(parent).is_some() {
            self.open_picker(Purpose::Insert(parent), window, cx);
        }
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

    /// The same picker, to change the class of every one of `targets`.
    pub(super) fn open_change_class_picker(
        &mut self,
        targets: Vec<Ref>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !targets.is_empty() {
            self.open_picker(Purpose::ChangeClass(targets), window, cx);
        }
    }

    fn open_picker(&mut self, purpose: Purpose, window: &mut Window, cx: &mut Context<Self>) {
        // `window` is what `InputState::new` needs; the caret itself lands a
        // frame later (see `Shell::focus_explorer_edit`).
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription = cx.subscribe(&query, |shell, _, event: &InputEvent, cx| match event {
            InputEvent::Change => {
                if let Some(picker) = shell.explorer_edit.picker.as_mut() {
                    picker.highlight = 0;
                    picker.scroll.scroll_to_item(0);
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } => shell.commit_highlighted(cx),
            _ => {}
        });
        self.explorer_edit.focus_next = Some(query.clone());

        self.explorer_edit.menu = None;
        self.explorer_edit.renaming = None;
        self.explorer_edit.picker = Some(Picker {
            purpose,
            query,
            scroll: ScrollHandle::new(),
            highlight: 0,
            hovered: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// What `picker` lists for what has been typed so far.
    fn listed(&self, picker: &Picker, cx: &App) -> Choices {
        let query = picker.query.read(cx).value().to_string();
        match &picker.purpose {
            Purpose::Insert(parent) => {
                let parent_class = self.dom.get(*parent).map(|instance| instance.class());
                Choices {
                    suggested: Vec::new(),
                    rest: insert::choices(&self.database, parent_class, &query),
                }
            }
            Purpose::ChangeClass(targets) => {
                // A service in the selection is refused whatever is picked,
                // so it has no say in what is suggested either.
                let sources: Vec<&str> = targets
                    .iter()
                    .filter(|&&target| change_class::changeable(&self.dom, &self.database, target))
                    .filter_map(|&target| self.dom.get(target))
                    .map(|instance| instance.class())
                    .collect();
                change_class::choices(
                    &self.database,
                    &sources,
                    &self.explorer_edit.recent_classes,
                    &query,
                )
            }
        }
    }

    pub(super) fn picker_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.explorer_edit.picker.as_ref()?;
        let listed = self.listed(picker, cx);
        let total = listed.suggested.len() + listed.rest.len();
        let hidden = total.saturating_sub(MAX_ROWS);
        let highlight = picker.highlight.min(total.saturating_sub(1));
        let changing = matches!(picker.purpose, Purpose::ChangeClass(_));

        // One lookup per painted row, not per class in the dump: the kit's
        // rasterizer memoizes (see `class_icons::icon_tile`), so this is a
        // hash lookup each, and the rows past `MAX_ROWS` cost nothing.
        let pack = self.icon_pack();
        let mut painted: Vec<AnyElement> = Vec::new();
        for (index, choice) in listed.iter().take(MAX_ROWS).enumerate() {
            if !listed.suggested.is_empty() && index == 0 {
                painted.push(caption("Suggested"));
            }
            if !listed.suggested.is_empty() && index == listed.suggested.len() {
                painted.push(caption("All classes"));
            }
            let icon = resolve_icon(&choice.class, pack);
            let refusal = (!choice.legal).then(|| self.refusal(picker, &choice.class));
            painted.push(class_row(choice, icon, index == highlight, refusal, cx));
        }

        // The pointer's row, else the highlighted one — whichever is being
        // looked at is the one worth pricing.
        let footer = match &picker.purpose {
            Purpose::ChangeClass(targets) => picker
                .hovered
                .as_deref()
                .and_then(|class| listed.iter().find(|choice| choice.class == class))
                .or_else(|| listed.iter().nth(highlight))
                .filter(|choice| choice.legal)
                .map(|choice| self.change_class_summary(targets, &choice.class)),
            Purpose::Insert(_) => None,
        };

        let surface = v_flex()
            .id("insert-picker")
            .occlude()
            .w(px(if changing { CHANGE_WIDTH } else { WIDTH }))
            .p(px(4.))
            .gap(px(4.))
            .bg(tokens::chrome())
            .rounded(tokens::RADIUS)
            .shadow(tokens::elevation())
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.explorer_edit.picker = None;
                cx.notify();
            }))
            .capture_action(cx.listener(|shell, _: &MoveUp, _, cx| {
                shell.move_highlight(false, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &MoveDown, _, cx| {
                shell.move_highlight(true, cx);
                cx.stop_propagation();
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
                    .when(!changing, |this| this.child(self.insertion_options(cx))),
            )
            .child(
                div()
                    .id("insert-picker-list")
                    .max_h(px(LIST_HEIGHT))
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    // The rows are the scroll container's own children, so
                    // `ScrollHandle::scroll_to_item` can find the highlight.
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .children(painted)
                    .vertical_scrollbar(&picker.scroll),
            )
            .when(hidden > 0, |this| {
                this.child(note(format!("{hidden} more — keep typing")))
            })
            .when_some(footer, |this, footer| this.child(note(footer)));

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

    /// Why a refused row's `class` cannot be picked, for its tooltip.
    fn refusal(&self, picker: &Picker, class: &str) -> String {
        match &picker.purpose {
            Purpose::Insert(_) => format!("{class} cannot be created here"),
            Purpose::ChangeClass(_) if change_class::is_target(&self.database, class) => {
                format!("Already a {class}")
            }
            Purpose::ChangeClass(_) => format!("{class} cannot be created"),
        }
    }

    /// Up (`down == false`) or Down: one row, stopping at either end rather
    /// than wrapping, the way the Explorer's own tree does.
    fn move_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(picker) = self.explorer_edit.picker.as_ref() else {
            return;
        };
        let listed = self.listed(picker, cx);
        let last = listed.iter().take(MAX_ROWS).count().saturating_sub(1);
        let suggested = listed.suggested.len();
        let Some(picker) = self.explorer_edit.picker.as_mut() else {
            return;
        };
        let current = picker.highlight.min(last);
        picker.highlight = if down {
            (current + 1).min(last)
        } else {
            current.saturating_sub(1)
        };
        // Past the captions painted before it (see `picker_popup`).
        let captions = match suggested {
            0 => 0,
            count if picker.highlight < count => 1,
            _ => 2,
        };
        picker.scroll.scroll_to_item(picker.highlight + captions);
        cx.notify();
    }

    /// Enter: commits the highlighted row, if it can be picked at all.
    fn commit_highlighted(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.explorer_edit.picker.as_ref() else {
            return;
        };
        let listed = self.listed(picker, cx);
        let picked = listed
            .iter()
            .nth(picker.highlight)
            .filter(|choice| choice.legal)
            .map(|choice| choice.class.clone());
        if let Some(class) = picked {
            self.commit_picked(class, cx);
        }
    }

    /// Commits one row of the picker: closes it, then runs the one path its
    /// purpose already has — an insert goes exactly where the Insert menu
    /// and the quick-insert keys go, so a picked class costs one undo step
    /// and reaches the viewport the same way every other insert does.
    fn commit_picked(&mut self, class: String, cx: &mut Context<Self>) {
        let Some(picker) = self.explorer_edit.picker.take() else {
            return;
        };
        match picker.purpose {
            Purpose::Insert(parent) => self.insert_instance_under(Some(parent), &class, cx),
            Purpose::ChangeClass(targets) => self.change_class(&targets, &class, cx),
        }
    }

    fn hover_picked(&mut self, class: &str, hovered: bool, cx: &mut Context<Self>) {
        let Some(picker) = self.explorer_edit.picker.as_mut() else {
            return;
        };
        if hovered {
            picker.hovered = Some(class.to_owned());
        } else if picker.hovered.as_deref() == Some(class) {
            picker.hovered = None;
        } else {
            return;
        }
        cx.notify();
    }
}
