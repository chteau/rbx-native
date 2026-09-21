//! The Explorer tree and Properties list content, split out of `shell.rs`
//! (kept under 400 lines per `AGENTS.md`) since both are self-contained
//! render methods called back into from `shell::dock`'s panel builder.

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer;
use crate::properties::{group_by_category, EditKind};
use crate::tokens;

use super::explorer_edit::RowWidgets;
use super::reparent::{draggable_row, DraggedInstances};
use super::rows::{
    checkbox, expander, guide_mask, property_expandable, property_row, property_row_control,
    render_editor, row, section_header, text_field, OnOpen,
};
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
        // One Tab stop for the whole panel; Up/Down move between its
        // headers and checkboxes. Opened before the rows are built and
        // closed after, since how many controls there are depends on what
        // is selected.
        self.properties_nav.begin(&self.tab_order, None, cx);
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
                    None => property_row(row).into_any_element(),
                    // No persistent entity: a checkbox commits straight
                    // through the same textual path (`shell::Shell::commit_row`)
                    // every other widget uses, via `cx.entity()` since a
                    // `Checkbox`'s `on_click` only hands back `&mut App`
                    // (see `shell::dock`'s "Show all services" toggle for
                    // the same pattern).
                    Some(EditKind::Bool(flag)) => {
                        let handle = cx.entity();
                        let name = row.name.clone();
                        let flag = *flag;
                        let control = self.properties_nav.claim(
                            checkbox(
                                SharedString::from(format!("prop-bool-{}", row.name)),
                                flag,
                                move |_, _, cx| {
                                    let text = if flag { "false" } else { "true" };
                                    let name = name.clone();
                                    handle
                                        .update(cx, |shell, cx| shell.commit_row(&name, text, cx));
                                },
                            ),
                            cx,
                        );
                        property_row_control(row, control, false, None).into_any_element()
                    }
                    Some(kind) => {
                        let tab_index = self.tab_order.next();
                        let (widget, error) = self.edit_row(row, kind, window, cx);
                        // Starting a scrub needs the property's name and
                        // which field moved; the value itself is read off
                        // the field at mouse-down (see `shell::scrub`).
                        let scrub_handle = cx.entity();
                        let scrub_name = row.name.clone();
                        let on_scrub: super::rows::OnScrub =
                            std::rc::Rc::new(move |index, kind| {
                                let handle = scrub_handle.clone();
                                let name = scrub_name.clone();
                                Box::new(move |event: &MouseDownEvent, _, cx| {
                                    let name = name.clone();
                                    handle.update(cx, |shell, cx| {
                                        shell.begin_scrub(&name, index, kind, event.position.x, cx);
                                    });
                                })
                            });

                        // A numeric value keeps the ordinary name/value row
                        // — the whole value in the field, the way it reads
                        // in a script — and hangs its components off an
                        // expander. Collapsed, those component rows are
                        // never built: a `BasePart` alone carries five of
                        // them, and their fields are the bulk of what this
                        // panel lays out and paints every frame.
                        if let Some(summary) = widget.summary().cloned() {
                            let expanded = self.is_row_expanded(&row.name);
                            let fields = expanded.then(|| {
                                render_editor(
                                    tab_index,
                                    &self.tab_order,
                                    widget,
                                    // Neither shape under an expander is a
                                    // flag set or a sequence, so neither
                                    // handler is ever reached.
                                    |_, _| Box::new(|_, _, _| {}),
                                    on_scrub,
                                    Box::new(|_, _, _| {}),
                                    cx,
                                )
                            });
                            let toggle = cx.entity();
                            let toggle_name = row.name.clone();
                            let expander = self.properties_nav.claim(
                                expander(&row.name, expanded, move |_, _, cx| {
                                    let name = toggle_name.clone();
                                    toggle.update(cx, |shell, cx| {
                                        shell.toggle_row_expanded(&name, cx)
                                    });
                                }),
                                cx,
                            );
                            children.push(
                                property_expandable(
                                    expander,
                                    text_field(&summary, tab_index),
                                    fields,
                                    error.as_deref(),
                                )
                                .into_any_element(),
                            );
                            continue;
                        }

                        let composite = widget.is_composite();
                        // A flag's click has to rewrite the *whole* set, so
                        // the row hands the renderer a factory that knows
                        // the current flags and which one moved.
                        let flags = match &widget {
                            super::edit::RowEditor::Flags(_, values) => values.clone(),
                            // An optional's present/absent checkbox commits
                            // through the same one-flag path, so the factory
                            // below hands it `true`/`false` unchanged.
                            super::edit::RowEditor::Optional(present, ..) => vec![*present],
                            _ => Vec::new(),
                        };
                        let handle = cx.entity();
                        let name = row.name.clone();
                        // A sequence row's click opens the graph rather
                        // than committing anything; every other row shape
                        // never reaches this handler.
                        let open_handle = cx.entity();
                        let open_name = row.name.clone();
                        let on_open: OnOpen = Box::new(move |_, window, cx| {
                            let name = open_name.clone();
                            open_handle.update(cx, |shell, cx| {
                                shell.open_sequence_editor(&name, window, cx);
                            });
                        });
                        let control = render_editor(
                            tab_index,
                            &self.tab_order,
                            widget,
                            move |index, checked| {
                                let mut next = flags.clone();
                                next[index] = !checked;
                                let text = next
                                    .iter()
                                    .map(|flag| flag.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let handle = handle.clone();
                                let name = name.clone();
                                Box::new(move |_, _, cx| {
                                    let name = name.clone();
                                    let text = text.clone();
                                    handle
                                        .update(cx, |shell, cx| shell.commit_row(&name, &text, cx));
                                })
                            },
                            on_scrub,
                            on_open,
                            cx,
                        );
                        property_row_control(row, control, composite, error.as_deref())
                            .into_any_element()
                    }
                };
                children.push(element);
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
            ))
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
                            .gap(tokens::header_gap())
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
                                                .pb(tokens::group_gap())
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
