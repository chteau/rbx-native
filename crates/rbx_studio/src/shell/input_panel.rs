//! The Input Actions dock: every `InputContext` in the place as a section,
//! its `InputAction`s as rows, and each action's `InputBinding`s sorted into
//! one column per device family (`crate::input_actions`). Selecting a
//! context, action or binding — here or in the Explorer — opens it in the
//! inspector beside the table, which edits every property the API dump
//! saves for its class. Check mode marks what `input_actions::check` finds
//! inline.
//!
//! Every write goes through [`Shell::apply_input_edit`], the same DOM
//! take/put-back the Style Editor uses, so undo, the Explorer and the
//! Properties panel see these edits like any other.
//!
//! `RBX_STUDIO_INPUT_ACTIONS=1` opens the dock at startup, and `=check`
//! opens it in Check mode: a screenshot aid like the other `RBX_STUDIO_*`
//! variables.

mod inspector;
mod table;
mod toolbar;

use std::collections::{HashMap, HashSet};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::component::{h_flex, v_flex, IndexPath};
use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::edit::EnumOptions;
use super::layout::Panel;
use super::{chrome, menu, Shell};
use crate::input_actions::{self, Family, ACTION_CLASS, BINDING_CLASS, CONTEXT_CLASS};
use crate::{explorer, tokens};

/// One open text field and the property it commits to.
struct Field {
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// One open enum dropdown.
struct Choice {
    state: Entity<SelectState<EnumOptions>>,
    _subscription: Subscription,
}

/// The dock's live state. Widgets are cached by `"<referent>:<property>"` so
/// a keystroke survives the panel rebuilding around it.
#[derive(Default)]
pub(super) struct InputEdits {
    fields: HashMap<SharedString, Field>,
    choices: HashMap<SharedString, Choice>,
    /// The context, action or binding the inspector shows.
    pub(super) focused: Option<Ref>,
    /// The selection [`Shell::follow_input_selection`] last acted on.
    followed: Option<Ref>,
    /// Check mode: issues drawn inline.
    pub(super) checking: bool,
    /// The last rejected edit.
    pub(super) error: Option<String>,
    /// Contexts folded to their header.
    pub(super) collapsed: HashSet<Ref>,
    pub(super) table_scroll: ScrollHandle,
    pub(super) inspector_scroll: ScrollHandle,
}

fn is_input_class(class: &str) -> bool {
    matches!(class, CONTEXT_CLASS | ACTION_CLASS | BINDING_CLASS)
}

fn widget_key(referent: Ref, property: &str) -> SharedString {
    format!("{}:{property}", referent.value()).into()
}

/// `properties::edit::commit`, plus the one case it cannot type-check: a
/// property with neither a stored value nor a reflection default
/// (`InputBinding.Deadzone` has none in the defaults file, though Studio
/// saves `0.1, 1` on a new binding). The value is
/// then parsed against a zero of the dump's type — which only shapes the
/// parse; nothing is written unless the typed text parses.
pub(super) fn commit_typed(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    referent: Ref,
    property: &str,
    text: &str,
) -> Result<(), String> {
    let instance = dom
        .get(referent)
        .ok_or_else(|| "the instance no longer exists".to_owned())?;
    if property != crate::properties::edit::NAME_PROPERTY
        && db.stored_or_default(instance, property).is_none()
    {
        let class = instance.class().to_owned();
        let zero = db
            .resolve_property(&class, property)
            .and_then(|descriptor| zero_of(&descriptor.value_type));
        if let Some(zero) = zero {
            let value = crate::properties::edit::parse(&zero, db, &class, property, text)?;
            dom.set_property(referent, property, value)
                .map_err(|err| err.to_string())?;
            return Ok(());
        }
    }
    crate::properties::edit::commit(dom, db, referent, property, text).map(|_| ())
}

/// A zero value of a dump value type, for the types the Input Action
/// classes declare without a default.
fn zero_of(value_type: &str) -> Option<Variant> {
    match value_type {
        "NumberRange" => Some(Variant::NumberRange(rbx_dom::NumberRange {
            min: 0.0,
            max: 0.0,
        })),
        _ => None,
    }
}

/// Read once at startup; documented in this module's doc comment.
const INPUT_ACTIONS_VARIABLE: &str = "RBX_STUDIO_INPUT_ACTIONS";

impl Shell {
    /// `RBX_STUDIO_INPUT_ACTIONS=1|check`: see this module's doc comment.
    pub(super) fn apply_debug_input_actions(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(INPUT_ACTIONS_VARIABLE) else {
            return;
        };
        self.input_edits.checking = spec == "check";
        self.set_panel_open(Panel::InputActions, true, cx);
    }

    /// The dock's trailing controls and body (see `shell::workspace`).
    pub(super) fn input_actions_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            menu::MenuId::InputActionsOverflow,
            chrome::dock_options_trigger(
                "input-actions-overflow",
                gpui_kit::assets::IconName::Ellipsis,
                16.,
                "Input Actions dock options",
            ),
            self.move_items(Panel::InputActions),
            cx,
        )
        .into_any_element();

        self.follow_input_selection();
        let model = input_actions::model(&self.dom, &self.database);
        let issues = if self.input_edits.checking {
            input_actions::check::check(&model)
        } else {
            Vec::new()
        };
        let table = self.input_table(&model, &issues, window, cx);
        let inspector = self.input_inspector(&issues, window, cx);
        let body = v_flex()
            .size_full()
            .border_t_1()
            .border_color(tokens::border())
            .child(self.input_toolbar(&model, issues.len(), cx))
            .children(self.input_error())
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(div().flex_1().min_w_0().h_full().child(table))
                    .children(inspector),
            )
            .into_any_element();
        (Some(overflow), Some(body))
    }

    /// Moves the inspector to a newly selected context, action or binding.
    /// Only on a change, and only for those classes: picking a `GuiButton`
    /// in the Explorer to bind keeps the binding open.
    fn follow_input_selection(&mut self) {
        let selected = self.selected();
        if self.input_edits.followed == selected {
            return;
        }
        self.input_edits.followed = selected;
        let Some(referent) = selected else {
            return;
        };
        if self
            .dom
            .get(referent)
            .is_some_and(|instance| is_input_class(instance.class()))
        {
            self.input_edits.focused = Some(referent);
            let model = input_actions::model(&self.dom, &self.database);
            if let Some(context) = model.context_of(&self.dom, referent) {
                self.input_edits.collapsed.remove(&context);
            }
        }
    }

    /// The one path every write in the dock takes: snapshotted for undo,
    /// `self.dom` handed out and back, the change log reflected, and the
    /// Explorer rebuilt when instances came or went. An edit that returns a
    /// referent (a new instance) has it selected.
    pub(super) fn apply_input_edit(
        &mut self,
        edit: impl FnOnce(&mut WeakDom, &ReflectionDatabase) -> Result<Option<Ref>, String>,
        cx: &mut Context<Self>,
    ) {
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = edit(&mut dom, &self.database);
        self.dom = dom;
        let changes = self.dom.take_changes();
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
        match result {
            Ok(Some(created)) => {
                self.input_edits.error = None;
                self.select(created, cx);
            }
            Ok(None) => self.input_edits.error = None,
            Err(err) => self.input_edits.error = Some(err),
        }
        cx.notify();
    }

    /// Writes `text` to `property` of `referent` (see [`commit_typed`]) and
    /// drops the cached widget so the next render reseeds from what landed.
    pub(super) fn commit_input_value(
        &mut self,
        referent: Ref,
        property: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let key = widget_key(referent, property);
        let (property, text) = (property.to_owned(), text.to_owned());
        self.apply_input_edit(
            move |dom, db| commit_typed(dom, db, referent, &property, &text).map(|()| None),
            cx,
        );
        self.input_edits.fields.remove(&key);
        self.input_edits.choices.remove(&key);
    }

    /// The cached text field for one property, committing on Enter or blur.
    pub(super) fn input_text(
        &mut self,
        referent: Ref,
        property: &str,
        seed: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let key = widget_key(referent, property);
        if let Some(field) = self.input_edits.fields.get(&key) {
            let input = field.input.clone();
            super::edit::resync_field(&input, seed, window, cx);
            return input;
        }
        let seed = seed.to_owned();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(seed));
        let property = property.to_owned();
        let subscription = cx.subscribe(&input, move |shell, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                let text = input.read(cx).value().to_string();
                shell.commit_input_value(referent, &property, &text, cx);
            }
        });
        self.input_edits.fields.insert(
            key,
            Field {
                input: input.clone(),
                _subscription: subscription,
            },
        );
        input
    }

    /// The cached dropdown for one enum property, its items the dump's
    /// `enum_name` items, committing the picked name. Searchable, since
    /// `KeyCode` has a few hundred items.
    pub(super) fn input_enum(
        &mut self,
        referent: Ref,
        property: &str,
        enum_name: &str,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<EnumOptions>> {
        let key = widget_key(referent, property);
        let items: Vec<SharedString> = self
            .database
            .enum_items(enum_name)
            .unwrap_or_default()
            .iter()
            .map(|(name, _)| SharedString::from(name.clone()))
            .collect();
        let position = items.iter().position(|item| item.as_ref() == current);
        if let Some(choice) = self.input_edits.choices.get(&key) {
            let state = choice.state.clone();
            let shown = state.read(cx).selected_index(cx).map(|index| index.row);
            if shown != position {
                state.update(cx, |state, cx| {
                    state.set_selected_index(position.map(IndexPath::new), window, cx)
                });
            }
            return state;
        }
        let state = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(items),
                position.map(IndexPath::new),
                window,
                cx,
            )
            .searchable(true)
        });
        let property = property.to_owned();
        let subscription = cx.subscribe(
            &state,
            move |shell, _, event: &SelectEvent<EnumOptions>, cx| {
                if let SelectEvent::Confirm(Some(picked)) = event {
                    let picked = picked.to_string();
                    shell.commit_input_value(referent, &property, &picked, cx);
                }
            },
        );
        self.input_edits.choices.insert(
            key,
            Choice {
                state: state.clone(),
                _subscription: subscription,
            },
        );
        state
    }

    /// A new `InputContext` under `ReplicatedStorage` (where
    /// `input/input-action-system.md` keeps them), or the first root.
    pub(super) fn add_input_context(&mut self, cx: &mut Context<Self>) {
        self.apply_input_edit(
            |dom, _| {
                let parent = explorer::find_by_name(dom, "ReplicatedStorage")
                    .or_else(|| dom.root_refs().first().copied());
                let name = explorer::insert::incremented_name(dom, parent, CONTEXT_CLASS);
                Ok(Some(dom.new_instance(CONTEXT_CLASS, &name, parent)))
            },
            cx,
        );
    }

    /// A new `InputAction` inside `context`.
    pub(super) fn add_input_action(&mut self, context: Ref, cx: &mut Context<Self>) {
        self.apply_input_edit(
            move |dom, _| {
                let name = explorer::insert::incremented_name(dom, Some(context), ACTION_CLASS);
                Ok(Some(dom.new_instance(ACTION_CLASS, &name, Some(context))))
            },
            cx,
        );
    }

    /// A new `InputBinding` inside `action`, named for the column it was
    /// added from the way the guide names its examples (`KeyboardBinding`,
    /// `GamepadBinding`, `TouchBinding`). It binds nothing until its key or
    /// button is set in the inspector, which it opens in.
    pub(super) fn add_input_binding(
        &mut self,
        action: Ref,
        family: Family,
        cx: &mut Context<Self>,
    ) {
        let base = match family {
            Family::KeyboardMouse => "KeyboardBinding",
            Family::Gamepad => "GamepadBinding",
            Family::Touch => "TouchBinding",
        };
        self.apply_input_edit(
            move |dom, _| {
                let name = explorer::insert::incremented_name(dom, Some(action), base);
                Ok(Some(input_actions::new_binding(dom, &name, action)))
            },
            cx,
        );
    }

    /// Deletes a context, action or binding with its subtree, through the
    /// Explorer's own delete.
    pub(super) fn delete_input_instance(&mut self, referent: Ref, cx: &mut Context<Self>) {
        if self.input_edits.focused == Some(referent) {
            self.input_edits.focused = None;
        }
        self.remove_instances(&[referent], cx);
    }
}
