//! The Explorer tree and Properties list content, split out of `shell.rs`
//! (kept under 400 lines per `AGENTS.md`) since both are self-contained
//! render methods called back into from `shell::dock`'s panel builder.

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer;
use crate::properties::group_by_category;
use crate::tokens;

use super::explorer_edit::RowWidgets;
use super::reparent::{draggable_row, DraggedInstances};
use super::rows::{guide_mask, row, section_header};
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
        // §3.2's guides are a property of the whole visible list, not of one
        // row: computed once here, read per row below.
        let depths: Vec<usize> = {
            let tree = self.tree.read(cx);
            (0..)
                .map_while(|index| tree.entry(index).map(|entry| entry.depth()))
                .collect()
        };
        let guides = guide_mask(&depths);
        // Which row is hovered and which is being renamed: read once here,
        // since the per-row closure below runs inside the tree's own layout
        // pass and cannot borrow the shell back out of it.
        let slots = self.row_slots();

        let tree_focus = self.tree_focus_handle.clone();
        self.tab_order.register(&tree_focus);
        let tree_entity = self.tree.clone();

        super::tree_keys::intercept_arrows(
            div()
                .id("explorer-tree")
                .size_full()
                // The tree's keyboard door. `TreeState` owns its own focus
                // handle privately and never puts it in the tab order, so
                // the whole APG contract below was reachable by mouse only.
                // This wrapper is the stop; focusing it hands focus
                // straight on to the tree, which is what puts the toolkit's
                // `Tree` key context on the dispatch path.
                .track_focus(&tree_focus)
                .on_click(cx.listener(move |_, _, window, cx| {
                    tree_entity.update(cx, |tree, cx| tree.focus(window, cx));
                }))
                // Home, End and type-ahead arrive as plain keystrokes, so
                // they are caught here; the arrows arrive as actions and are
                // caught by `intercept_arrows` (see `shell::tree_keys`).
                .capture_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                    if shell.handle_tree_key(&event.keystroke, cx) {
                        cx.stop_propagation();
                    }
                }))
                .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                    shell.handle_explorer_key(&event.keystroke, window, cx);
                }))
                .child(
                    base::Tree::new(&self.tree)
                        .item(move |index, entry, _, _, _| {
                            let guide = guides.get(index).copied().unwrap_or_default();
                            let item = entry.item();
                            let icon = explorer.icon(&item.id);
                            let tint = tints.get(&item.id).copied();
                            // A row whose id does not read back as a referent has
                            // nothing to drag or drop onto; it still has to draw.
                            let Some(reference) = explorer::item_ref(&item.id) else {
                                return row(
                                    index,
                                    entry,
                                    false,
                                    icon,
                                    tint,
                                    guide,
                                    RowWidgets::default(),
                                );
                            };
                            let highlighted = selected.contains(&reference);
                            let dragged = DraggedInstances::new(&selected, reference, &item.label);
                            let widgets = slots.of(&shell, reference);
                            draggable_row(
                                &shell,
                                index,
                                reference,
                                dragged,
                                row(index, entry, highlighted, icon, tint, guide, widgets),
                            )
                        })
                        .list_style(StyleRefinement::default().flex_grow_1().size_full())
                        .relative()
                        .size_full(),
                )
                .vertical_scrollbar(&scroll_handle),
            cx,
        )
    }

    /// The dock's displayed title for the Properties panel (see
    /// `shell::dock`): what is selected (see `properties::Properties::title`),
    /// or the plain section name when nothing is — the inner header this used
    /// to feed is gone, the dock's own title bar shows it instead.
    /// The reference's line under the filter: the anchor instance's class
    /// icon in an accent box, its name, then "— Parent" muted. Absent with
    /// nothing selected, and for a multi-selection, which has no one name.
    fn properties_breadcrumb(&self) -> Option<AnyElement> {
        let [reference] = self.selected_all() else {
            return None;
        };
        let instance = self.dom.get(*reference)?;
        let parent = self
            .dom
            .parent(*reference)
            .and_then(|parent| self.dom.get(parent))
            .map(|parent| format!("— {}", parent.name()));
        let icon = crate::explorer::resolve_icon(instance.class(), self.icon_pack());
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .items_center()
                .gap(px(7.))
                .px(tokens::row_padding())
                .child(
                    div()
                        .flex_none()
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(tokens::radius_badge())
                        .bg(tokens::accent_soft())
                        .text_color(tokens::check_on())
                        .child(super::rows::class_icon(icon)),
                )
                .child(
                    div()
                        .text_size(tokens::text_sm())
                        .line_height(tokens::line_sm())
                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                        .text_color(tokens::text())
                        .child(SharedString::from(instance.name().to_owned())),
                )
                .children(parent.map(|parent| {
                    div()
                        .truncate()
                        .text_size(tokens::text_xs())
                        .line_height(tokens::line_xs())
                        .text_color(tokens::text3())
                        .child(SharedString::from(parent))
                }))
                .into_any_element(),
        )
    }

    pub(super) fn properties_title(&self) -> SharedString {
        self.properties
            .title(&self.dom, self.selected_all())
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
        // One Tab stop for the whole panel; Up/Down move between its
        // headers and checkboxes. Opened before the rows are built and
        // closed after, since how many controls there are depends on what
        // is selected.
        self.properties_nav.begin(&self.tab_order, None, cx);
        let folder_color = self
            .selected()
            .and_then(|reference| self.folder_color(reference));
        let rows =
            self.properties
                .rows_matching(&self.dom, self.selected_all(), &filter, folder_color);

        // Built up front — needs `&mut self` to create or reuse each row's
        // widget entity (see `shell::edit::edit_row`) — so the `Accordion`
        // item closures below only ever move already-built elements around,
        // never borrow `self` again.
        let mut sections: Vec<(String, Vec<AnyElement>)> = Vec::new();
        for (category, category_rows) in group_by_category(rows) {
            let mut children = Vec::with_capacity(category_rows.len());
            for row in &category_rows {
                children.push(self.property_element(row, false, window, cx));
            }
            sections.push((category, children));
        }

        // Built before `self.properties_nav.finish()` so its own header and
        // buttons take the next indices in the same single Tab stop the rest
        // of the panel shares — see `shell::attributes_panel`.
        let attributes_and_tags = self.attributes_and_tags(&filter, window, cx);
        self.properties_nav.finish();
        let handle = cx.entity();

        v_flex()
            .size_full()
            .gap(tokens::row_gap())
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.properties_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(super::workspace::search_field(
                self.tab_order.next(),
                &self.filter,
                cx,
            ))
            .children(self.properties_breadcrumb())
            .child(
                div()
                    .id("properties-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.properties_scroll)
                    .child(
                        v_flex()
                            .w_full()
                            // Gestalt proximity, as a ratio: the gap after
                            // a category's last row is the largest in the
                            // panel, then header-to-first-row, then
                            // row-to-row, then label-to-input. Flatten any
                            // one of them into its neighbour and the panel
                            // stops reading as groups at all.
                            //
                            // The gap *here* is the small one, because two
                            // collapsed headers are two tiles in a stack;
                            // `group_gap` is spent below, on the open rows,
                            // where there is actually a group to close.
                            .gap(tokens::group_gap())
                            // Bottom only. A matching top pad stacked on
                            // the gap under the search field and left the
                            // first category floating a long way down the
                            // panel; the list needs air *after* it, not
                            // before.
                            .pb(tokens::panel_padding())
                            .children(sections.into_iter().map(|(category, children)| {
                                // A filter forces every section open, so a
                                // match is never hidden behind a collapsed
                                // one.
                                let open = filtering || !self.is_category_collapsed(&category);
                                let handle = handle.clone();
                                let name = category.clone();
                                v_flex()
                                    .w_full()
                                    .child(self.properties_nav.claim(
                                        section_header(
                                            SharedString::from(category),
                                            open,
                                            move |_, _, cx| {
                                                let name = name.clone();
                                                handle.update(cx, |shell, cx| {
                                                    shell.toggle_category(&name, cx)
                                                });
                                            },
                                        ),
                                        cx,
                                    ))
                                    .when(open, |this| {
                                        this.child(
                                            v_flex()
                                                .w_full()
                                                .pt(tokens::section_gap())
                                                .gap(tokens::row_gap())
                                                .children(children),
                                        )
                                    })
                            }))
                            // Custom attributes and `CollectionService` tags,
                            // after every reflected category — Roblox's own
                            // Properties window puts both "at the bottom of
                            // the window" too (see `shell::attributes_panel`).
                            .child(attributes_and_tags),
                    )
                    .vertical_scrollbar(&self.properties_scroll),
            )
    }
}
