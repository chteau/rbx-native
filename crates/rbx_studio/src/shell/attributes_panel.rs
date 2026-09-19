//! The Properties panel's Attributes and Tags section: custom `Instance`
//! attributes and `CollectionService` tags, appended after the ordinary
//! property categories (see `shell::panels::properties`) the way Roblox's
//! own Properties window puts both "at the bottom of the window"
//! (`studio/properties.md`).
//!
//! Split into three submodules, each under this file's own ~400-line
//! budget: [`attribute_rows`] (the Attributes section's rows, rename and
//! add/remove chrome), [`attribute_value`] (routing one attribute's value
//! through the Properties panel's existing per-type editors) and [`tags`]
//! (the Tags section). This file only holds what all three share: the
//! section's live-widget state and the entry point that lays Attributes
//! above Tags.
//!
//! An attribute's *value* renders through exactly the same `RowEditor`/
//! `render_editor` machinery an ordinary property row uses (see
//! `attribute_value`) — reached through a `PropertyRow` whose name is
//! prefixed (`properties::attributes::row_name`) so `shell::edit::apply_edit`
//! can route its commit to `properties::attributes::set_attribute_value`
//! instead of `WeakDom::set_property`. Only the *name* column (a label, or an
//! `Input` while renaming) and the add/remove chrome around it are new here;
//! nothing about how a value itself is typed or edited is reimplemented.
//!
//! Every mutation — add, remove, rename, a value edit, add/remove tag — goes
//! through the same push-history/reflect/record sequence
//! `shell::edit::apply_edit` uses for an ordinary property, so each one costs
//! exactly one undo step.

use gpui_kit::component::input::InputState;
use gpui_kit::component::select::{SearchableVec, SelectState};
use gpui_kit::component::v_flex;
use gpui_kit::*;

use crate::tokens;

use super::Shell;

mod attribute_rows;
mod attribute_value;
mod tags;

const ATTRIBUTES_CATEGORY: &str = "Attributes";
const TAGS_CATEGORY: &str = "Tags";

type AttributeTypeOptions = SearchableVec<SharedString>;

/// One attribute being renamed in place: its old name (so the commit knows
/// what to rename *from*) and the `Input` holding its pending new name.
struct Renaming {
    old_name: String,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// The section's own live widgets — the rename box (if any), the "add
/// attribute"/"add tag" rows' fields, and the last rejected attempt's
/// message. Kept here rather than as further fields on `Shell` for the same
/// reason `shell::edit::Edits` is; cleared on every selection change
/// alongside it (see `Shell::selection_changed`). An attribute's *value*
/// widgets are not here — they live in `shell::edit::Edits` like any other
/// row, keyed by `properties::attributes::row_name`.
#[derive(Default)]
pub(super) struct AttributeEdits {
    renaming: Option<Renaming>,
    new_name: Option<Entity<InputState>>,
    new_type: Option<Entity<SelectState<AttributeTypeOptions>>>,
    new_tag: Option<Entity<InputState>>,
    /// Kept alive only to stay subscribed — see `shell::edit::RowEdit`'s
    /// identical convention.
    _subscriptions: Vec<Subscription>,
    /// Kept separate from `tag_error` so a rejected tag never shows up under
    /// the Attributes header and vice versa.
    attribute_error: Option<String>,
    tag_error: Option<String>,
}

impl AttributeEdits {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl Shell {
    /// The whole section: Attributes above Tags, both collapsible through
    /// the exact same `is_category_collapsed`/`toggle_category` an ordinary
    /// property category uses (see `shell::edit`), just keyed by these two
    /// synthetic category names instead of one the reflection dump named.
    /// `filter` is the same Properties panel filter box every ordinary
    /// property row is already narrowed by (see `shell::panels::properties`).
    pub(super) fn attributes_and_tags(
        &mut self,
        filter: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(reference) = self.selected() else {
            return div().into_any_element();
        };

        v_flex()
            .w_full()
            .gap(tokens::header_gap())
            .child(self.attribute_section(reference, filter, window, cx))
            .child(self.tag_section(reference, filter, window, cx))
            .into_any_element()
    }
}
