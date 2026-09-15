//! The editor's selection: zero or more instances, in DOM terms.

use gpui_kit::component::tree::TreeItem;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::explorer;

/// The service a viewport click's search for a `Model` ancestor stops at.
/// `Workspace` is itself a `Model` subclass in Roblox's own class hierarchy,
/// so without naming it here every click would "select the model" and land on
/// the whole workspace.
const WORKSPACE_CLASS: &str = "Workspace";
const MODEL_CLASS: &str = "Model";

/// The selection, in the order instances were added to it. Kept apart from
/// the tree's own selected row because the tree can track only one of them,
/// forgets even that one whenever its rows are replaced, and because the
/// viewport and Properties panel want referents, not a row index.
///
/// The first entry is the *anchor*: what the transform gizmo goes on (see
/// `crate::transform::Targets::anchor`, which picks the same way) and what
/// the Properties panel shows, exactly as if it were still the only thing
/// selected. `Shift`/`Ctrl`/`Cmd`-click ([`Selection::toggle`]) only ever
/// appends or removes from the end of this list; nothing here reorders it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Selection(Vec<Ref>);

impl Selection {
    pub(super) fn new(selected: impl IntoIterator<Item = Ref>) -> Self {
        Selection(selected.into_iter().collect())
    }

    /// The anchor — see this type's own doc comment — or `None` with nothing
    /// selected.
    pub(super) fn get(&self) -> Option<Ref> {
        self.0.first().copied()
    }

    /// Every selected instance, anchor first.
    pub(super) fn all(&self) -> &[Ref] {
        &self.0
    }

    /// Replaces the whole selection with at most one instance — a plain
    /// click, in the viewport or the Explorer, always replaces rather than
    /// extends. Reports whether anything changed so a caller redrawing on
    /// every tree update can stay quiet when it did not.
    pub(super) fn set(&mut self, selected: Option<Ref>) -> bool {
        let selected = Vec::from_iter(selected);
        let changed = self.0 != selected;
        self.0 = selected;
        changed
    }

    /// `Shift`/`Ctrl`/`Cmd`-click: adds `reference` to the selection if it
    /// was not already in it, or drops it if it was — the standard
    /// multi-select toggle. `creator-docs` (`studio/ui-overview.md`) only
    /// documents the "adds another object" half; toggling back off on a
    /// second click of the same object is not spelled out there, but is
    /// standard multi-select behaviour and what Studio itself does.
    pub(super) fn toggle(&mut self, reference: Ref) {
        match self.0.iter().position(|&selected| selected == reference) {
            Some(index) => {
                self.0.remove(index);
            }
            None => self.0.push(reference),
        }
    }

    /// The tree's selected row, read back as a referent.
    pub(super) fn of_item(item: Option<&TreeItem>) -> Option<Ref> {
        item.and_then(|item| explorer::item_ref(&item.id))
    }
}

/// What a click in the 3D view selects, given everything under the cursor
/// (`hits`, nearest first — see `rbx_viewer::pick::parts_along`) and whatever
/// is selected now.
///
/// Two behaviours, both taken from `creator-docs`
/// (`parts/models.md#select-models`, `studio/ui-overview.md#selection-cycling`):
///
/// - A plain click takes the nearest hit and selects the **outermost model**
///   it belongs to, which is what makes clicking any wall of a house select
///   the house.
/// - `cycling` — Studio's `Alt`/`⌥`-click — steps to "the next further object
///   behind the currently selected object" instead, one raw part at a time and
///   without reaching for a model, which is how a child buried inside one is
///   reached without leaving the viewport. It wraps back to the nearest hit at
///   the end, so holding `Alt` and clicking repeatedly goes round rather than
///   sticking on the last one.
pub(super) fn from_click(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    hits: &[Ref],
    current: Option<Ref>,
    cycling: bool,
) -> Option<Ref> {
    let &nearest = hits.first()?;
    if !cycling {
        return Some(outermost_model(dom, database, nearest));
    }

    // A selection that is not itself under the cursor — nothing selected, a
    // model picked by an earlier plain click, or a part elsewhere entirely —
    // has no "next" to step past, so cycling starts over at the front.
    let position = current.and_then(|current| hits.iter().position(|&hit| hit == current));
    Some(match position {
        Some(position) => hits[(position + 1) % hits.len()],
        None => nearest,
    })
}

/// The highest `Model` `referent` sits inside, or `referent` itself when it
/// sits in none.
///
/// Walks down from the roots rather than up from the hit: an `Instance` here
/// knows its children but not its parent, and carrying the enclosing model
/// down the descent answers the question in one pass without building a
/// parent map for the whole DOM on every click.
fn outermost_model(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Ref {
    let mut pending: Vec<(Ref, Option<Ref>)> =
        dom.root_refs().iter().map(|&root| (root, None)).collect();

    while let Some((current, model)) = pending.pop() {
        if current == referent {
            return model.unwrap_or(referent);
        }
        let Some(instance) = dom.get(current) else {
            continue;
        };
        // `or` rather than a replacement: the *outermost* model wins, so a
        // model nested inside another never overrides it.
        let model = model.or_else(|| {
            let class = instance.class();
            (class != WORKSPACE_CLASS && database.is_subclass_of(class, MODEL_CLASS))
                .then_some(current)
        });
        pending.extend(instance.children().iter().map(|&child| (child, model)));
    }
    referent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_moves_between_instances_and_back_to_nothing() {
        let a = Ref::new(1);
        let b = Ref::new(2);
        let mut selection = Selection::default();
        assert_eq!(selection.get(), None);

        assert!(selection.set(Some(a)));
        assert_eq!(selection.get(), Some(a));

        assert!(selection.set(Some(b)));
        assert_eq!(selection.get(), Some(b));

        // Re-selecting the same instance is not a change worth a redraw.
        assert!(!selection.set(Some(b)));

        assert!(selection.set(None));
        assert_eq!(selection.get(), None);
        assert!(!selection.set(None));
    }

    #[test]
    fn toggling_adds_and_then_removes_an_instance() {
        let a = Ref::new(1);
        let b = Ref::new(2);
        let mut selection = Selection::new([a]);

        // Adds `b` alongside `a` rather than replacing it — this is the
        // `Shift`/`Ctrl`/`Cmd`-click path, not a plain click.
        selection.toggle(b);
        assert_eq!(selection.all(), [a, b]);
        assert_eq!(
            selection.get(),
            Some(a),
            "the anchor stays the first one added"
        );

        // Clicking the same object again with the modifier held drops it.
        selection.toggle(b);
        assert_eq!(selection.all(), [a]);

        // Toggling the anchor itself off promotes whatever is left.
        selection.toggle(a);
        assert!(selection.all().is_empty());
        assert_eq!(selection.get(), None);
    }

    #[test]
    fn toggling_off_the_anchor_promotes_the_next_instance() {
        let a = Ref::new(1);
        let b = Ref::new(2);
        let c = Ref::new(3);
        let mut selection = Selection::new([a]);
        selection.toggle(b);
        selection.toggle(c);
        assert_eq!(selection.all(), [a, b, c]);

        selection.toggle(a);
        assert_eq!(selection.all(), [b, c]);
        assert_eq!(selection.get(), Some(b));
    }

    #[test]
    fn a_fresh_selection_can_start_with_several_instances() {
        let a = Ref::new(1);
        let b = Ref::new(2);
        let selection = Selection::new([a, b]);
        assert_eq!(selection.all(), [a, b]);
        assert_eq!(selection.get(), Some(a));
    }

    #[test]
    fn a_tree_row_reads_back_as_its_referent() {
        let item = TreeItem::new(explorer::item_id(Ref::new(42)), "Baseplate");

        assert_eq!(Selection::of_item(Some(&item)), Some(Ref::new(42)));
        assert_eq!(Selection::of_item(None), None);
        // A row whose id is not a referent (none exist, but the tree does not
        // know that) must never turn into a bogus selection.
        let stray = TreeItem::new("not-a-ref", "?");
        assert_eq!(Selection::of_item(Some(&stray)), None);
    }

    /// A workspace holding a loose part and a two-level model, returned as
    /// `(dom, loose part, outer model, inner model, deep part)`.
    fn nested_place() -> (WeakDom, Ref, Ref, Ref, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let loose = dom.new_instance("Part", "Baseplate", Some(workspace));
        let outer = dom.new_instance("Model", "House", Some(workspace));
        let inner = dom.new_instance("Model", "Door", Some(outer));
        let deep = dom.new_instance("Part", "Handle", Some(inner));
        (dom, loose, outer, inner, deep)
    }

    #[test]
    fn a_plain_click_selects_the_outermost_model_the_part_belongs_to() {
        let (dom, _, outer, _, deep) = nested_place();
        let database = ReflectionDatabase::embedded();

        assert_eq!(
            from_click(&dom, &database, &[deep], None, false),
            Some(outer),
            "clicking a door handle selects the whole house, not the door"
        );
    }

    #[test]
    fn a_part_in_no_model_is_selected_as_itself() {
        let (dom, loose, ..) = nested_place();
        let database = ReflectionDatabase::embedded();

        assert_eq!(
            from_click(&dom, &database, &[loose], None, false),
            Some(loose)
        );
    }

    #[test]
    fn the_workspace_itself_is_never_what_a_click_selects() {
        // `Workspace` is a `Model` subclass in Roblox's class hierarchy, so
        // a naive search upwards would answer every click with it.
        let (dom, loose, ..) = nested_place();
        let database = ReflectionDatabase::embedded();
        let selected = from_click(&dom, &database, &[loose], None, false);

        let class = selected
            .and_then(|referent| dom.get(referent))
            .map(|i| i.class());
        assert_eq!(class, Some("Part"));
    }

    #[test]
    fn clicking_nothing_selects_nothing() {
        let (dom, ..) = nested_place();
        let database = ReflectionDatabase::embedded();

        assert_eq!(from_click(&dom, &database, &[], None, false), None);
        assert_eq!(
            from_click(&dom, &database, &[], Some(Ref::new(1)), true),
            None
        );
    }

    #[test]
    fn alt_click_cycles_through_what_is_under_the_cursor() {
        let (dom, loose, outer, _, deep) = nested_place();
        let database = ReflectionDatabase::embedded();
        let hits = [deep, loose];

        // Nothing under the cursor selected yet — start at the nearest, and
        // stay on the raw part rather than reaching for its model.
        assert_eq!(from_click(&dom, &database, &hits, None, true), Some(deep));
        // A model picked by an earlier plain click is not itself a hit, so
        // cycling starts over rather than having nowhere to go.
        assert_eq!(
            from_click(&dom, &database, &hits, Some(outer), true),
            Some(deep)
        );
        // Then step behind it, and wrap round at the end.
        assert_eq!(
            from_click(&dom, &database, &hits, Some(deep), true),
            Some(loose)
        );
        assert_eq!(
            from_click(&dom, &database, &hits, Some(loose), true),
            Some(deep)
        );
    }

    #[test]
    fn alt_click_with_one_thing_under_the_cursor_stays_on_it() {
        let (dom, loose, ..) = nested_place();
        let database = ReflectionDatabase::embedded();

        assert_eq!(
            from_click(&dom, &database, &[loose], Some(loose), true),
            Some(loose)
        );
    }
}
