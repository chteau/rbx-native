//! The UI Editor's Stylesheet tab: every `StyleSheet` in the place as a
//! section, its rules as lines inside it, and an open rule's selector,
//! priority and property overrides editable in place (Roblox's own editor:
//! `ui/styling/editor.md`). Rules start closed and open when selected, so
//! the tab reads as an outline until something is being edited.
//!
//! Every write goes through the one DOM take/put-back the Command Bar and the
//! Properties panel already use (see [`Shell::apply_style_edit`]), so undo,
//! the Explorer and the live viewport all see a styling edit the way they see
//! any other.
//!
//! `RBX_STUDIO_STYLE_EDITOR=1` brings this panel's tab to the front at
//! startup; given a `Prop=value` instead, it applies that edit to whatever
//! `StyleRule` `RBX_STUDIO_SELECT` selected, through this same commit path,
//! and deliberately leaves the layout alone so the Viewport stays in front
//! and a screenshot catches the live preview rather than this panel. A
//! debugging aid, like the other `RBX_STUDIO_*` variables: the panel starts
//! stacked behind the Viewport, and nothing else can click its tab on the
//! editor's behalf (see `AGENTS.md`'s safety rules).

mod field;
mod rule;
mod view;

use std::collections::{HashMap, HashSet};

use crate::shell::chrome::ScrollbarY as _;
use gpui_kit::component::input::InputState;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::style_editor::{self, StyleRow};
use crate::tokens;

use super::Shell;

/// Read once at startup by `Shell::new`; documented in this module's doc
/// comment.
pub(crate) const STYLE_EDITOR_VARIABLE: &str = "RBX_STUDIO_STYLE_EDITOR";

/// What committing one of the panel's text fields writes.
#[derive(Clone)]
enum Target {
    /// A plain property of a styling instance — a rule's `Selector` or
    /// `Priority` — committed through `properties::edit::commit`, the same
    /// path a Properties row takes.
    Instance { referent: Ref, property: String },
    /// One entry of a rule's `PropertiesSerialize` blob.
    RuleProperty { rule: Ref, name: String },
    /// The rule's "add a property" field, which takes `Name = value` in one
    /// line rather than Studio's property dropdown beside a typed value
    /// widget: this panel has no per-type widgets to switch between, and the
    /// spelling is the one `RBX_STUDIO_EDIT` already uses.
    NewProperty { rule: Ref },
}

/// One open field: its widget and the subscription that commits it.
struct Field {
    input: Entity<InputState>,
    target: Target,
    _subscription: Subscription,
}

/// The panel's live state: one `Input` per editable cell currently on screen,
/// the last rejected edit's message, and what is folded. Kept here rather
/// than as further fields on `Shell` for the same reason `shell::edit::Edits`
/// is.
#[derive(Default)]
pub(super) struct StyleEdits {
    fields: HashMap<SharedString, Field>,
    error: Option<String>,
    /// Sheets start open and rules closed: a place has a handful of sheets
    /// but can have dozens of rules, and a closed rule is one line.
    closed_sheets: HashSet<Ref>,
    open_rules: HashSet<Ref>,
    /// The selection [`Shell::follow_selection`] last acted on.
    followed: Option<Ref>,
}

impl Shell {
    /// The Stylesheet tab's body.
    pub(super) fn style_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rows = style_editor::rows(&self.dom);
        let selected = self.selected();
        self.follow_selection(selected);
        let link_target = selected.filter(|&referent| {
            self.dom.get(referent).is_some_and(|instance| {
                self.database
                    .is_subclass_of(instance.class(), "LayerCollector")
            })
        });
        let mut counts: HashMap<Ref, usize> = HashMap::new();
        for row in &rows {
            if let StyleRow::Property { rule, .. } = row {
                *counts.entry(*rule).or_default() += 1;
            }
        }

        // Built up front: each field needs `&mut self` to create or reuse its
        // widget, so the elements are finished before the list borrows them.
        let mut sections: Vec<Vec<AnyElement>> = Vec::new();
        let mut sheet_open = true;
        for (index, row) in rows.iter().enumerate() {
            let next = rows.get(index + 1);
            let lines = match row {
                StyleRow::Sheet {
                    referent,
                    name,
                    parent,
                } => {
                    sheet_open = !self.style_edits.closed_sheets.contains(referent);
                    let header = self.style_sheet_header(
                        *referent,
                        name,
                        parent,
                        sheet_open,
                        selected == Some(*referent),
                        link_target,
                        cx,
                    );
                    let mut lines = vec![header];
                    let bare = next.is_none_or(|next| matches!(next, StyleRow::Sheet { .. }));
                    if sheet_open && bare {
                        lines.push(self.style_sheet_empty());
                    }
                    sections.push(lines);
                    continue;
                }
                _ if !sheet_open => continue,
                StyleRow::Derive {
                    referent,
                    sheet,
                    priority,
                } => vec![self.derive_line(
                    *referent,
                    sheet,
                    *priority,
                    selected == Some(*referent),
                    cx,
                )],
                StyleRow::Rule {
                    referent,
                    depth,
                    selector,
                    priority,
                } => {
                    let open = self.style_edits.open_rules.contains(referent);
                    let count = counts.get(referent).copied().unwrap_or(0);
                    let mut lines = vec![self.rule_header(
                        *referent,
                        *depth,
                        selector,
                        open,
                        selected == Some(*referent),
                        count,
                        window,
                        cx,
                    )];
                    if open {
                        lines.push(self.priority_line(*referent, *depth, *priority, window, cx));
                        if count == 0 {
                            lines.push(self.add_property_line(*referent, *depth, window, cx));
                        }
                    }
                    lines
                }
                StyleRow::Property {
                    rule,
                    depth,
                    name,
                    value,
                } => {
                    if !self.style_edits.open_rules.contains(rule) {
                        continue;
                    }
                    let mut lines = vec![self.property_override(
                        *rule,
                        *depth,
                        name,
                        value.as_deref(),
                        window,
                        cx,
                    )];
                    // The add field closes the rule's own lines, after its
                    // last property and before any rule nested in it.
                    let last = !next.is_some_and(|next| {
                        matches!(next, StyleRow::Property { rule: other, .. } if other == rule)
                    });
                    if last {
                        lines.push(self.add_property_line(*rule, *depth, window, cx));
                    }
                    lines
                }
            };
            if let Some(section) = sections.last_mut() {
                section.extend(lines);
            }
        }

        let sheets = sections.len();
        let toolbar = (sheets > 0).then(|| self.style_toolbar(sheets, cx));
        let body = if sections.is_empty() {
            self.style_empty(cx)
        } else {
            div()
                .id("style-editor-rows")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.style_scroll)
                .child(
                    v_flex()
                        .w_full()
                        .text_size(tokens::text_md())
                        .line_height(tokens::line_md())
                        .children(sections.into_iter().map(|lines| {
                            // The UI Editor inspector's section frame (see
                            // `ui_editor::inspector::view::section`).
                            v_flex()
                                .w_full()
                                .gap(tokens::row_padding())
                                .px(px(8.))
                                .pt(px(8.))
                                .pb(px(10.))
                                .border_b_1()
                                .border_color(tokens::border())
                                .children(lines)
                        })),
                )
                .scrollbar_y(&self.style_scroll)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(tokens::border())
            .children(toolbar)
            .children(self.style_error())
            .child(body)
    }

    /// Opens what a new selection lands on, so selecting a rule anywhere —
    /// the Explorer, this tab, or by adding one — shows its properties
    /// without a second click. Only on a change: a rule closed by hand while
    /// selected stays closed.
    fn follow_selection(&mut self, selected: Option<Ref>) {
        if self.style_edits.followed == selected {
            return;
        }
        self.style_edits.followed = selected;
        let Some(referent) = selected else {
            return;
        };
        if self
            .dom
            .get(referent)
            .is_some_and(|instance| instance.class() == style_editor::RULE_CLASS)
        {
            self.style_edits.open_rules.insert(referent);
        }
        // A sheet is only reopened for something inside it: selecting the
        // sheet itself is what clicking its header does.
        let mut at = self.dom.parent(referent);
        while let Some(ancestor) = at {
            if self
                .dom
                .get(ancestor)
                .is_some_and(|instance| instance.class() == style_editor::SHEET_CLASS)
            {
                self.style_edits.closed_sheets.remove(&ancestor);
                break;
            }
            at = self.dom.parent(ancestor);
        }
    }

    /// Brings the style sheets to the front: the UI Editor document, on its
    /// Stylesheet sub-tab — the View menu's Style Editor item (see
    /// `crate::menu_bar`).
    pub(crate) fn reveal_style_editor(&mut self, cx: &mut Context<Self>) {
        self.set_document(super::chrome::Document::UiEditor, cx);
        self.show_stylesheet(cx);
    }

    /// `RBX_STUDIO_STYLE_EDITOR=1|Prop=value`: documented in this module's
    /// doc comment. A selection that is not a `StyleRule`, or a rejected
    /// value, does nothing — this is a screenshot aid, not user input, and
    /// must never crash a debugging session.
    pub(super) fn apply_debug_style_editor(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(STYLE_EDITOR_VARIABLE) else {
            return;
        };
        let Some((name, value)) = spec.split_once('=') else {
            // No edit to apply: the whole point of the variable is then to
            // raise the tab for a screenshot of the panel itself.
            self.reveal_style_editor(cx);
            return;
        };
        let Some(rule) = self.selected().filter(|&reference| {
            self.dom
                .get(reference)
                .is_some_and(|instance| instance.class() == style_editor::RULE_CLASS)
        }) else {
            return;
        };
        let (name, value) = (name.trim().to_owned(), value.trim().to_owned());
        self.apply_style_edit(
            move |dom, database| {
                style_editor::set_rule_property(dom, database, rule, &name, &value)
            },
            cx,
        );
    }
}
