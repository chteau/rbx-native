//! The Explorer tree and Properties list content, split out of `shell.rs`
//! (kept under 400 lines per `AGENTS.md`) since both are self-contained
//! render methods called back into from `shell::dock`'s panel builder.

use std::collections::HashSet;

use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{v_flex, ActiveTheme, Sizable};
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer;
use crate::properties::{group_by_category, EditKind};

use super::reparent::{draggable_row, DraggedInstances};
use super::rows::{property_row, property_row_control, render_editor, row};
use super::Shell;

impl Shell {
    /// The instance tree, built on the unstyled `gpui_base` element rather than
    /// the styled `tree()` wrapper: that wrapper also paints a right-click
    /// style and a popup menu hook, and the Explorer has neither — a click
    /// selects (and may expand) a row, nothing more.
    ///
    /// `on_key_down` sits on this outer div rather than inside `base::Tree`
    /// itself (out of reach, in `gpui_base`): GPUI dispatches a key event
    /// bubbling up from whatever holds focus, and `Tree`'s own rows are the
    /// only thing in the Explorer that ever does — see `shell::keys`.
    pub(super) fn instance_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let explorer = self.explorer.clone();
        // Every row's drop handler mutates the DOM, and its `can_drop` has to
        // ask the DOM what it would do — neither runs while this render holds
        // `self`, so both reach the shell back through its own handle.
        let shell = cx.entity();
        let scroll_handle = self.tree.read(cx).scroll_handle().clone();
        // Highlighting reads `self.selection`'s own full set rather than the
        // `TreeState`'s `state.is_selected()`: the tree can track only one
        // selected row, so a `Shift`/`Ctrl`/`Cmd`-click multi-selection would
        // otherwise light up just the anchor.
        let selected: Vec<Ref> = self.selected_all().to_vec();
        // Every tagged `Folder`'s colour, resolved once up front rather than
        // per row — see `shell::folder_color::folder_tints`.
        let tints = self.folder_tints();

        div()
            .id("explorer-tree")
            .size_full()
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                shell.handle_explorer_key(&event.keystroke, cx);
            }))
            .child(
                base::Tree::new(&self.tree)
                    .item(move |index, entry, _, _, _| {
                        let item = entry.item();
                        let icon = explorer.icon(&item.id);
                        let tint = tints.get(&item.id).copied();
                        // A row whose id does not read back as a referent has
                        // nothing to drag or drop onto; it still has to draw.
                        let Some(reference) = explorer::item_ref(&item.id) else {
                            return row(index, entry, false, icon, tint);
                        };
                        let highlighted = selected.contains(&reference);
                        let dragged = DraggedInstances::new(&selected, reference, &item.label);
                        draggable_row(
                            &shell,
                            index,
                            reference,
                            dragged,
                            row(index, entry, highlighted, icon, tint),
                        )
                    })
                    .list_style(StyleRefinement::default().flex_grow_1().size_full())
                    .relative()
                    .size_full(),
            )
            .vertical_scrollbar(&scroll_handle)
    }

    /// The dock's displayed title for the Properties panel (see
    /// `shell::dock`): the selected instance's class and name, or the plain
    /// section name when nothing is selected — the inner header this used to
    /// feed is gone, the dock's own title bar shows it instead.
    pub(super) fn properties_title(&self) -> SharedString {
        self.selected()
            .and_then(|reference| self.properties.title(&self.dom, reference))
            .map_or_else(|| SharedString::from("Properties"), SharedString::from)
    }

    /// A row becomes whichever widget its `EditKind` calls for (see
    /// `shell::edit`/`shell::rows::render_editor`); anything else stays the
    /// read-only text `property_row` always was. Rows are grouped into
    /// collapsible sections by their reflection `Category`, the way
    /// Studio's own Properties panel does (see `properties::group_by_category`);
    /// a filter forces every section open so a match is never hidden behind
    /// a collapsed one.
    pub(super) fn properties(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let filter = self.filter.read(cx).value();
        let filtering = !filter.trim().is_empty();
        let rows = self
            .selected()
            .map(|reference| {
                let folder_color = self.folder_color(reference);
                self.properties
                    .rows_matching(&self.dom, reference, &filter, folder_color)
            })
            .unwrap_or_default();

        // Built up front — needs `&mut self` to create or reuse each row's
        // widget entity (see `shell::edit::edit_row`) — so the `Accordion`
        // item closures below only ever move already-built elements around,
        // never borrow `self` again.
        let mut sections: Vec<(String, Vec<AnyElement>)> = Vec::new();
        for (category, category_rows) in group_by_category(rows) {
            let mut children = Vec::with_capacity(category_rows.len());
            for row in &category_rows {
                let element = match &row.edit {
                    None => property_row(row, cx).into_any_element(),
                    // No persistent entity: a checkbox commits straight
                    // through the same textual path (`shell::Shell::commit_row`)
                    // every other widget uses, via `cx.entity()` since a
                    // `Checkbox`'s `on_click` only hands back `&mut App`
                    // (see `shell::dock`'s "Show all services" toggle for
                    // the same pattern).
                    Some(EditKind::Bool(flag)) => {
                        let handle = cx.entity();
                        let name = row.name.clone();
                        let checkbox =
                            Checkbox::new(SharedString::from(format!("prop-bool-{}", row.name)))
                                .checked(*flag)
                                .xsmall()
                                .on_click(move |checked, _, cx| {
                                    let text = if *checked { "true" } else { "false" };
                                    handle
                                        .update(cx, |shell, cx| shell.commit_row(&name, text, cx));
                                });
                        property_row_control(row, checkbox, None, cx).into_any_element()
                    }
                    Some(kind) => {
                        let (widget, error) = self.edit_row(row, kind, window, cx);
                        let control = render_editor(widget, cx);
                        property_row_control(row, control, error.as_deref(), cx).into_any_element()
                    }
                };
                children.push(element);
            }
            sections.push((category, children));
        }

        let categories: Vec<String> = sections
            .iter()
            .map(|(category, _)| category.clone())
            .collect();
        let handle = cx.entity();
        let toggled_categories = categories.clone();
        let mut accordion = Accordion::new("properties-categories")
            .multiple(true)
            // The vendored `Accordion` renders itself `size_full()`: left
            // alone, it clips to whatever height the scroll container below
            // hands it instead of growing past it, so the container never
            // sees an overflow to scroll — nothing past that height was ever
            // reachable. `h_auto()` lets it take its natural content height
            // instead, which is what makes the container's own
            // `overflow_y_scroll` below have something to scroll.
            .h_auto()
            .on_toggle_click(move |open_indices: &[usize], _, cx| {
                let open: HashSet<usize> = open_indices.iter().copied().collect();
                let collapsed: HashSet<String> = toggled_categories
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !open.contains(index))
                    .map(|(_, category)| category.clone())
                    .collect();
                handle.update(cx, |shell, cx| {
                    shell.set_collapsed_categories(collapsed, cx)
                });
            });
        for (category, children) in sections {
            let open = filtering || !self.is_category_collapsed(&category);
            accordion = accordion.item(move |item| {
                item.title(SharedString::from(category))
                    .open(open)
                    .children(children)
            });
        }

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_1()
                    .my_1()
                    .child(Input::new(&self.filter).small().py_1()),
            )
            .child(
                div()
                    .id("properties-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.properties_scroll)
                    .child(accordion)
                    .vertical_scrollbar(&self.properties_scroll),
            )
    }
}
