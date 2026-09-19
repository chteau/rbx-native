//! The editor's selection: zero or more instances, in DOM terms.

use gpui_kit::component::tree::TreeItem;
use gpui_kit::Context;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::{self, Selected};

use crate::explorer;
use crate::transform::Targets;

use super::Shell;

/// The service a viewport click's search for a `Model` ancestor stops at.
/// `Workspace` is itself a `Model` subclass in Roblox's own class hierarchy,
/// so without naming it here every click would "select the model" and land on
/// the whole workspace.
const WORKSPACE_CLASS: &str = "Workspace";
const MODEL_CLASS: &str = "Model";

/// What the viewport outlines for each selected instance: the instance and
/// every drawable part it stands for, resolved here because only the editor
/// side holds a DOM to walk (see `rbx_viewer::pick::Selected`).
///
/// Paired with `crate::transform::Targets::read`, which flattens the entries
/// this returns: one derivation of what a selected `Model` covers — dedup
/// included, so a model selected alongside its own child is outlined by the
/// one box that spans both rather than by two drawn over each other — and so
/// the box the user sees and the handles the cursor can reach are built from
/// the same parts.
pub(super) fn outlined(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referents: &[Ref],
) -> Vec<Selected> {
    pick::selection(dom, database, referents)
}

/// Both halves of what the 3D view shows for a selection, read together: the
/// boxes the renderer outlines it with, and the placements this side
/// hit-tests the handles against.
///
/// Together because they are one answer to one question. What a selected
/// `Model` covers changes whenever the DOM does, so a script that parents
/// another `Part` under it moves the box and the gizmo as surely as picking a
/// different model would — and reading back only one of the two leaves the
/// other describing the membership the selection had before.
pub(super) fn shown(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referents: &[Ref],
) -> (Vec<Selected>, Targets) {
    (
        outlined(dom, database, referents),
        Targets::read(dom, database, referents),
    )
}

/// Whether `nearest` — the part a click actually lands on, `None` for a
/// click on nothing — is already part of the selection `outlined` describes:
/// a selected part itself, or a part beneath a selected `Model`. What lets a
/// Move body-drag begin on it rather than re-select it (see
/// `Shell::pick_in_viewport`).
pub(super) fn covers(outlined: &[Selected], nearest: Option<Ref>) -> bool {
    nearest.is_some_and(|part| outlined.iter().any(|entry| entry.parts().contains(&part)))
}

impl Shell {
    /// Re-resolves the selection against the current `self.dom` and sends the
    /// viewport both halves of it (see [`shown`]).
    ///
    /// One method for both of its callers — a selection change, and a reload
    /// — because a reload is the other way what a selected container covers
    /// can change, and the user has no way to ask for the box again short of
    /// reselecting.
    pub(super) fn sync_viewport_selection(&mut self, cx: &mut Context<Self>) {
        let (outline, targets) = shown(&self.dom, &self.database, self.selection.all());
        self.covered = targets.iter().map(|target| target.referent).collect();
        self.viewport.update(cx, |viewport, _| {
            viewport.set_selection(&outline);
            viewport.set_targets(targets);
        });
    }
}

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
    /// Replaces the whole selection, reporting whether it changed — what an
    /// undo needs, since a multi-selection survives one exactly as far as
    /// its referents still resolve.
    pub(super) fn replace(&mut self, selected: Vec<Ref>) -> bool {
        let changed = self.0 != selected;
        self.0 = selected;
        changed
    }

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
    use rbx_dom::{CFrameData, Variant, Vector3Data};

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
    fn replacing_keeps_every_survivor_of_a_multi_selection() {
        let (a, b, c) = (Ref::new(1), Ref::new(2), Ref::new(3));
        let mut selection = Selection::new([a, b, c]);

        assert!(!selection.replace(vec![a, b, c]), "nothing changed");
        assert!(selection.replace(vec![a, c]), "b is gone");
        assert_eq!(selection.all(), [a, c]);
        assert_eq!(selection.get(), Some(a));
        assert!(selection.replace(Vec::new()));
        assert_eq!(selection.get(), None);
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

    // The bug this exists for: with the Move tool up, a click on a part
    // standing in front of the selection body-dragged the selection instead
    // of selecting the part, because the view only ever tested the
    // selection's own boxes.
    #[test]
    fn a_click_on_a_selected_part_or_inside_a_selected_model_is_a_body_grab() {
        let (dom, loose, outer, _, deep) = nested_place();
        let database = ReflectionDatabase::embedded();

        let part_selected = outlined(&dom, &database, &[loose]);
        assert!(covers(&part_selected, Some(loose)));
        assert!(
            !covers(&part_selected, Some(deep)),
            "a part outside the selection is a pick"
        );

        let model_selected = outlined(&dom, &database, &[outer]);
        assert!(
            covers(&model_selected, Some(deep)),
            "any part beneath the model drags it"
        );
        assert!(!covers(&model_selected, Some(loose)));
    }

    #[test]
    fn a_click_on_nothing_never_grabs() {
        let (dom, loose, ..) = nested_place();
        let database = ReflectionDatabase::embedded();
        assert!(!covers(&outlined(&dom, &database, &[loose]), None));
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

    /// What a Command Bar script does to a selected model, and what the
    /// reload after it has to answer for: the box and the handles both cover
    /// the part that appeared, with nothing asked of the user.
    ///
    /// Reading back only the targets — which is all a reload used to do —
    /// left a drag carrying three parts while the box drawn round them still
    /// spanned two.
    #[test]
    fn a_part_parented_under_a_selected_model_joins_both_halves_of_what_is_shown() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let model = dom.new_instance("Model", "House", Some(workspace));
        for index in 0..2 {
            placed_part(&mut dom, model, index as f32);
        }
        let database = ReflectionDatabase::embedded();

        let (outline, targets) = shown(&dom, &database, &[model]);
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].parts().len(), 2);
        assert_eq!(targets.iter().count(), 2);

        placed_part(&mut dom, model, 40.0);

        let (outline, targets) = shown(&dom, &database, &[model]);
        assert_eq!(outline[0].parts().len(), 3);
        assert_eq!(targets.iter().count(), 3);
    }

    /// The outline and the targets are one derivation, dedup and all: a model
    /// selected with its own child is one box, over exactly the parts a drag
    /// carries.
    #[test]
    fn a_model_and_a_part_inside_it_are_one_box_over_the_parts_a_drag_carries() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let model = dom.new_instance("Model", "House", Some(workspace));
        let wall = placed_part(&mut dom, model, 0.0);
        placed_part(&mut dom, model, 8.0);
        let database = ReflectionDatabase::embedded();

        let (outline, targets) = shown(&dom, &database, &[model, wall]);
        assert_eq!(outline.len(), 1, "one box, not one per selected referent");
        assert_eq!(outline[0].referent(), model);
        assert_eq!(outline[0].parts().len(), targets.iter().count());
    }

    /// A unit cube `x` studs along, which is what `Targets::read` needs to
    /// see a part as draggable at all.
    fn placed_part(dom: &mut WeakDom, parent: Ref, x: f32) -> Ref {
        let part = dom.new_instance("Part", "Part", Some(parent));
        let _ = dom.set_property(
            part,
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data { x, y: 0.0, z: 0.0 },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
        let _ = dom.set_property(
            part,
            "size",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
        );
        part
    }

    /// Alt-click's whole point is reaching *one* part inside a model, so the
    /// aggregate box a plain click gets must not follow it there: what the
    /// cycled-to part is outlined and gizmoed with is its own box alone, over
    /// itself alone, even though the same click without `Alt` would have
    /// picked the model spanning it and its siblings.
    #[test]
    fn alt_click_reaches_one_part_inside_a_model_rather_than_the_whole_thing() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let model = dom.new_instance("Model", "House", Some(workspace));
        let wall = placed_part(&mut dom, model, 0.0);
        placed_part(&mut dom, model, 8.0);
        let database = ReflectionDatabase::embedded();

        let plain = from_click(&dom, &database, &[wall], None, false);
        assert_eq!(plain, Some(model));
        let (outline, targets) = shown(&dom, &database, &Vec::from_iter(plain));
        assert_eq!(outline[0].parts().len(), 2, "the model carries both");

        let cycled = from_click(&dom, &database, &[wall], plain, true);
        assert_eq!(
            cycled,
            Some(wall),
            "Alt steps to the raw part, not its model"
        );

        let (outline, targets_inside) = shown(&dom, &database, &Vec::from_iter(cycled));
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].referent(), wall);
        assert!(
            outline[0].is_part(),
            "its own oriented box, not an aggregate"
        );
        assert_eq!(outline[0].parts(), [wall]);
        assert_eq!(targets_inside.iter().count(), 1);
        assert_ne!(targets.iter().count(), targets_inside.iter().count());
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
