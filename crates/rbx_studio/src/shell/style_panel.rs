//! The Style Editor panel: every `StyleSheet` in the place, its derives and
//! its rules, each rule's selector, priority and property overrides editable
//! in place.
//!
//! Roblox's own Style Editor (`ui/styling/editor.md`) is a left column of
//! sheets beside a main panel for the selected one; this is the same material
//! as one list, at the fidelity the editor's other panels have. Clicking a row
//! selects its instance exactly as the Explorer would, so the Properties panel
//! follows along — a rule's own `Name` and anything this panel does not offer
//! is edited there.
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

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::style_editor::{self, StyleRow, PRIORITY_PROPERTY, SELECTOR_PROPERTY};

use super::Shell;

/// Indent per nesting level of a rule inside its sheet, in pixels — the same
/// step the Explorer's tree rows use.
const INDENT: f32 = 12.0;
const LABEL_WIDTH: f32 = 150.0;

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
/// and the last rejected edit's message. Kept here rather than as further
/// fields on `Shell` for the same reason `shell::edit::Edits` is.
#[derive(Default)]
pub(super) struct StyleEdits {
    fields: HashMap<SharedString, Field>,
    error: Option<String>,
}

impl Shell {
    /// The dock's Style Editor panel.
    pub(super) fn style_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rows = style_editor::rows(&self.dom);
        let selected = self.selected();

        // Built up front: each field needs `&mut self` to create or reuse its
        // widget, so the elements are finished before the list borrows them.
        let mut children: Vec<AnyElement> = Vec::with_capacity(rows.len());
        for row in &rows {
            children.push(self.style_row(row, selected, window, cx));
        }
        if rows.is_empty() {
            children.push(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("No StyleSheet in this place yet.")
                    .into_any_element(),
            );
        }

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.style_toolbar(cx))
            .children(self.style_error(cx))
            .child(
                div()
                    .id("style-editor-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.style_scroll)
                    .child(v_flex().w_full().children(children))
                    .vertical_scrollbar(&self.style_scroll),
            )
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

    fn style_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .p_1()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("style-new-sheet")
                    .label("New StyleSheet")
                    .xsmall()
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.apply_style_edit(
                            |dom, _| {
                                style_editor::add_sheet(dom);
                                Ok(())
                            },
                            cx,
                        );
                    })),
            )
    }

    fn style_error(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let message = self.style_edits.error.clone()?;
        Some(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(SharedString::from(message)),
        )
    }

    fn style_row(
        &mut self,
        row: &StyleRow,
        selected: Option<Ref>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let referent = row.referent();
        let highlighted = selected == Some(referent);
        let body = match row {
            StyleRow::Sheet { name, parent, .. } => self.sheet_row(referent, name, parent, cx),
            StyleRow::Derive {
                sheet, priority, ..
            } => derive_row(sheet, *priority, cx).into_any_element(),
            StyleRow::Rule {
                depth,
                selector,
                priority,
                ..
            } => self.rule_row(referent, *depth, selector, *priority, window, cx),
            StyleRow::Property {
                rule,
                depth,
                name,
                value,
            } => self.property_row(*rule, *depth, name, value.as_deref(), window, cx),
        };

        div()
            .id(("style-row", referent.value() as usize))
            .w_full()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .when(highlighted, |this| {
                this.bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
            })
            // A click selects the row's instance the way the Explorer's own
            // click does, so the Properties panel shows it — a property row
            // stands for the rule that holds it (see `StyleRow::referent`).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, _, _, cx| shell.select(referent, cx)),
            )
            .child(body)
            .into_any_element()
    }

    /// A sheet: its name, where it sits, and the two things Studio's own
    /// editor offers on a sheet — a new rule, and a `StyleLink` onto the
    /// currently selected `ScreenGui` (`ui/styling/editor.md`'s
    /// "Insert StyleLink").
    fn sheet_row(
        &mut self,
        sheet: Ref,
        name: &str,
        parent: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .text_xs()
            .child(
                div()
                    .flex_shrink_0()
                    .child(SharedString::from(name.to_owned())),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(parent.to_owned())),
            )
            .child(
                Button::new(("style-add-rule", sheet.value() as usize))
                    .label("Add rule")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.apply_style_edit(
                            move |dom, _| {
                                style_editor::add_rule(dom, sheet);
                                Ok(())
                            },
                            cx,
                        );
                    })),
            )
            .child(
                Button::new(("style-link", sheet.value() as usize))
                    .label("Insert StyleLink")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        let Some(target) = shell.selected() else {
                            shell.style_edits.error =
                                Some("select a ScreenGui to link this sheet to".to_owned());
                            cx.notify();
                            return;
                        };
                        shell.apply_style_edit(
                            move |dom, database| {
                                style_editor::add_link(dom, database, target, sheet).map(|_| ())
                            },
                            cx,
                        );
                    })),
            )
            .into_any_element()
    }

    /// A rule: its selector and priority, both editable, plus the field that
    /// adds a property to it.
    fn rule_row(
        &mut self,
        rule: Ref,
        depth: usize,
        selector: &str,
        priority: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selector_field = self.style_field(
            format!("selector:{}", rule.value()),
            selector,
            "Selector",
            Target::Instance {
                referent: rule,
                property: SELECTOR_PROPERTY.to_owned(),
            },
            window,
            cx,
        );
        let priority_field = self.style_field(
            format!("priority:{}", rule.value()),
            &priority.to_string(),
            "Priority",
            Target::Instance {
                referent: rule,
                property: PRIORITY_PROPERTY.to_owned(),
            },
            window,
            cx,
        );
        let add_field = self.style_field(
            format!("add:{}", rule.value()),
            "",
            "Add a property: Name = value",
            Target::NewProperty { rule },
            window,
            cx,
        );

        v_flex()
            .w_full()
            .gap_1()
            .pl(px((depth + 1) as f32 * INDENT))
            .text_xs()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(LABEL_WIDTH)).flex_shrink_0().child("Selector"))
                    .child(div().flex_1().child(Input::new(&selector_field).xsmall()))
                    .child(div().w(px(60.)).child(Input::new(&priority_field).xsmall())),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(LABEL_WIDTH)).flex_shrink_0())
                    .child(div().flex_1().child(Input::new(&add_field).xsmall())),
            )
            .into_any_element()
    }

    /// One of a rule's property overrides. A value whose type this editor
    /// cannot type back in (see `properties::edit::edit_text`) shows as text
    /// with no field, the same way the Properties panel leaves such a row
    /// read-only.
    fn property_row(
        &mut self,
        rule: Ref,
        depth: usize,
        name: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = value.map(|seed| {
            self.style_field(
                format!("prop:{}:{name}", rule.value()),
                seed,
                "",
                Target::RuleProperty {
                    rule,
                    name: name.to_owned(),
                },
                window,
                cx,
            )
        });
        let dropped = name.to_owned();

        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .text_xs()
            .pl(px((depth + 2) as f32 * INDENT))
            .child(
                div()
                    .w(px(LABEL_WIDTH))
                    .flex_shrink_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(name.to_owned())),
            )
            .child(match field {
                Some(input) => div()
                    .flex_1()
                    .child(Input::new(&input).xsmall())
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .text_color(cx.theme().muted_foreground)
                    .child("<not editable here>")
                    .into_any_element(),
            })
            .child(
                Button::new(SharedString::from(format!(
                    "style-drop:{}:{name}",
                    rule.value()
                )))
                .icon(IconName::Close)
                .ghost()
                .xsmall()
                .accessibility_label("Remove property")
                .on_click(cx.listener(move |shell, _, _, cx| {
                    let name = dropped.clone();
                    shell.apply_style_edit(
                        move |dom, database| {
                            style_editor::remove_rule_property(dom, database, rule, &name)
                        },
                        cx,
                    );
                })),
            )
            .into_any_element()
    }

    /// One cell's live `Input`, created the first time it renders and reused
    /// afterwards so a keystroke survives the panel rebuilding around it —
    /// the same cache, for the same reason, as `shell::edit::edit_row`.
    fn style_field(
        &mut self,
        key: impl Into<SharedString>,
        seed: &str,
        placeholder: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let key = key.into();
        if let Some(field) = self.style_edits.fields.get(&key) {
            let input = field.input.clone();
            resync(&input, seed, window, cx);
            return input;
        }

        let seed = seed.to_owned();
        let placeholder = placeholder.to_owned();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(seed)
        });
        let committed = key.clone();
        let subscription = cx.subscribe(&input, move |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                shell.commit_style_field(&committed, cx);
            }
        });
        self.style_edits.fields.insert(
            key.clone(),
            Field {
                input: input.clone(),
                target,
                _subscription: subscription,
            },
        );
        input
    }

    /// Writes one field's text, then drops its widget so the next render
    /// reseeds it from what actually landed in the DOM (a clamped colour, a
    /// rounded offset) — again the way `shell::edit::commit_row` does.
    fn commit_style_field(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        let Some(field) = self.style_edits.fields.get(key) else {
            return;
        };
        let text = field.input.read(cx).value().to_string();
        let target = field.target.clone();

        match target {
            Target::Instance { referent, property } => {
                self.apply_style_edit(
                    move |dom, database| {
                        crate::properties::edit::commit(dom, database, referent, &property, &text)
                            .map(|_| ())
                    },
                    cx,
                );
            }
            Target::RuleProperty { rule, name } => {
                self.apply_style_edit(
                    move |dom, database| {
                        style_editor::set_rule_property(dom, database, rule, &name, &text)
                    },
                    cx,
                );
            }
            Target::NewProperty { rule } => {
                if text.trim().is_empty() {
                    return;
                }
                let Some((name, value)) = text.split_once('=') else {
                    self.style_edits.error = Some(format!("{text:?} is not a Name = value pair"));
                    cx.notify();
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
        self.style_edits.fields.remove(key);
    }

    /// The one path every Style Editor write takes: `self.dom` handed out and
    /// back the way `shell::command` does it, snapshotted for undo first, and
    /// the resulting `Change` log handed to the viewport — which now rebuilds
    /// the GUI for a styling instance too (see `rbx_viewer`'s
    /// `changes::role`), so an edited rule is visible without a reload.
    fn apply_style_edit(
        &mut self,
        edit: impl FnOnce(&mut WeakDom, &ReflectionDatabase) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = edit(&mut dom, &self.database);
        self.dom = dom;
        let changes = self.dom.take_changes();
        // A new sheet, rule or link is a new Explorer row; a selector or
        // value edit is not, so the tree is only rebuilt when the DOM
        // actually gained or lost an instance.
        let structural = changes.iter().any(|change| {
            matches!(
                change,
                rbx_dom::Change::Added(_) | rbx_dom::Change::Removed(_)
            )
        });
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if structural {
            self.rebuild_explorer(cx);
        }
        self.style_edits.error = result.err();
        cx.notify();
    }
}

/// A derive is nothing but the sheet it names and the priority that orders
/// it, both edited in the Properties panel — the row only has to say what it
/// pulls in.
fn derive_row(sheet: &str, priority: i32, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .pl(px(INDENT))
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from(format!("derives {sheet}")))
        .child(SharedString::from(format!("priority {priority}")))
}

/// See `shell::edit::resync_field`, which this repeats for the Style
/// Editor's own fields: a cached widget must not hide a value that changed
/// from outside it (an undo, a Command Bar script), but must not lose a
/// keystroke in progress either.
fn resync(input: &Entity<InputState>, seed: &str, window: &mut Window, cx: &mut App) {
    if input.focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == seed {
        return;
    }
    input.update(cx, |state, cx| state.set_value(seed.to_owned(), window, cx));
}
