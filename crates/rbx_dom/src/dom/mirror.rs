//! Keeping a second copy of a DOM in step with the first, one change log at
//! a time, at a cost that scales with the edit rather than the tree.
//!
//! A consumer on another thread (a viewport's render thread, say) that reads
//! the DOM after every edit cannot borrow the editor's copy, and cloning the
//! whole tree per keystroke is the cost the change log exists to avoid. So
//! the editor hands over a [`Snapshot`] of each instance the log names — the
//! instance as it stands *now*, or its absence — and the consumer's own copy
//! is brought in line by [`WeakDom::mirror`]. The log is read as a list of
//! *which* instances may differ, never for the values it does not carry, so
//! an undo hands over the very log its mutation produced against the restored
//! tree and the mirror ends up restored too.

use std::collections::HashSet;

use super::WeakDom;
use crate::change::Change;
use crate::instance::Instance;
use crate::reference::Ref;

/// One instance as the DOM that produced a change log holds it now, with the
/// parent it hangs under — or its absence. Produced by [`WeakDom::snapshot`],
/// consumed by [`WeakDom::mirror`].
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    referent: Ref,
    /// `None` at the root level; meaningless when `instance` is `None`.
    parent: Option<Ref>,
    /// `None` when the DOM no longer holds the instance.
    instance: Option<Instance>,
}

impl WeakDom {
    /// What a mirror of this DOM has to be handed to come back in step after
    /// `changes` (see [`WeakDom::mirror`]), each referent once: every
    /// instance the log names, as it stands now, and — for one added,
    /// removed or moved — the parents whose child lists changed with it: the
    /// one it hangs under now and the one a move took it from. A property
    /// write costs one instance copy; only a structural change copies a
    /// parent, since only a structural change reorders one, and a parent's
    /// copy carries its children in the order this DOM holds them, which is
    /// what an undo needs to put a moved child back in its old place among
    /// its siblings rather than at the end.
    pub fn snapshot(&self, changes: &[Change]) -> Vec<Snapshot> {
        let mut wanted = Vec::new();
        let mut seen = HashSet::new();
        let mut want = |referent: Ref, wanted: &mut Vec<Ref>| {
            if seen.insert(referent) {
                wanted.push(referent);
            }
        };
        for change in changes {
            match change {
                // A class change reorders no child list, so, like a write,
                // it costs the one instance — which carries its new class.
                Change::Property { referent, .. } | Change::Class(referent) => {
                    want(*referent, &mut wanted)
                }
                Change::Parent { referent, old, new } => {
                    want(*referent, &mut wanted);
                    for parent in [old, new].into_iter().flatten() {
                        want(*parent, &mut wanted);
                    }
                    if let Some(parent) = self.parent(*referent) {
                        want(parent, &mut wanted);
                    }
                }
                Change::Added(referent) | Change::Removed(referent) => {
                    want(*referent, &mut wanted);
                    // Nothing in the log names the parent a removal detached
                    // from, and the undo of that removal puts the instance
                    // back under it: its child list is what says where.
                    if let Some(parent) = self.parent(*referent) {
                        want(parent, &mut wanted);
                    }
                }
            }
        }
        wanted
            .into_iter()
            .map(|referent| Snapshot {
                referent,
                parent: self.parent(referent),
                instance: self.get(referent).cloned(),
            })
            .collect()
    }

    /// Brings this DOM — a copy of another, kept in step edit by edit — in
    /// line with what [`WeakDom::snapshot`] took off that other after one of
    /// its change logs. Each snapshot is the truth about its instance: an
    /// absent one is taken out with whatever still hangs under it, a present
    /// one is put in place (class, name, properties and children as copied),
    /// hung under its parent and, where that parent was copied too, in the
    /// exact place among its siblings the source holds it. Nothing here is
    /// recorded in this DOM's own change log: a mirror's edits are not edits.
    pub fn mirror(&mut self, snapshots: Vec<Snapshot>) {
        for snapshot in snapshots {
            match snapshot.instance {
                Some(instance) => self.mirror_present(snapshot.referent, snapshot.parent, instance),
                None => self.mirror_absent(snapshot.referent),
            }
        }
    }

    fn mirror_present(&mut self, referent: Ref, parent: Option<Ref>, instance: Instance) {
        let hung_under = self.parents.get(&referent).copied();
        let at_root = hung_under.is_none() && self.root_refs.contains(&referent);
        if hung_under != parent || (parent.is_none() && !at_root) {
            self.detach(referent);
            match parent {
                Some(parent) => {
                    self.parents.insert(referent, parent);
                    if let Some(instance) = self.instances.get_mut(&parent) {
                        // Appended only where the parent's own snapshot does
                        // not settle the order — see `snapshot`.
                        if !instance.children().contains(&referent) {
                            instance.children_mut().push(referent);
                        }
                    }
                }
                None => self.root_refs.push(referent),
            }
        }
        self.next_ref = self.next_ref.max(referent.value().saturating_add(1));
        self.instances.insert(referent, instance);
    }

    fn mirror_absent(&mut self, referent: Ref) {
        if !self.instances.contains_key(&referent) {
            return;
        }
        self.detach(referent);
        // Every descendant is named by the same log (`remove` records each),
        // so this is only ever a no-op for them — kept for the invariant that
        // nothing hangs under an instance that is gone.
        let mut stack = vec![referent];
        while let Some(current) = stack.pop() {
            if let Some(instance) = self.instances.remove(&current) {
                stack.extend_from_slice(instance.children());
                self.parents.remove(&current);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::Variant;

    fn part(dom: &mut WeakDom, name: &str, parent: Option<Ref>) -> Ref {
        let referent = dom.new_instance("Part", name, parent);
        dom.set_property(referent, "Transparency", Variant::Float32(0.0))
            .unwrap();
        referent
    }

    struct Fixture {
        storage: Ref,
        model: Ref,
        children: [Ref; 3],
    }

    /// `Workspace { Model { A, B, C } }` next to `ServerStorage`, its log
    /// drained.
    fn place() -> (WeakDom, Fixture) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let storage = dom.new_instance("ServerStorage", "ServerStorage", None);
        let model = dom.new_instance("Model", "Model", Some(workspace));
        let children = [
            part(&mut dom, "A", Some(model)),
            part(&mut dom, "B", Some(model)),
            part(&mut dom, "C", Some(model)),
        ];
        dom.take_changes();
        (
            dom,
            Fixture {
                storage,
                model,
                children,
            },
        )
    }

    /// Structural equality of two DOMs: the same instances (children order
    /// included), the same parent edges, the same roots.
    fn assert_in_step(mirror: &WeakDom, source: &WeakDom) {
        assert_eq!(mirror.instances, source.instances);
        assert_eq!(mirror.parents, source.parents);
        let roots = |dom: &WeakDom| -> HashSet<Ref> { dom.root_refs.iter().copied().collect() };
        assert_eq!(roots(mirror), roots(source));
        assert_eq!(mirror.root_refs.len(), source.root_refs.len());
    }

    /// Runs `edit` on the source, mirrors its log forwards, then undoes it
    /// the way an editor's history does — the tree from before put back
    /// whole, the *edit's* log handed over — mirrors that, and redoes it.
    fn round_trip(edit: impl Fn(&mut WeakDom, &Fixture)) {
        let (mut dom, fixture) = place();
        let mut mirror = dom.clone();
        let before = dom.clone();
        edit(&mut dom, &fixture);
        let log = dom.take_changes();
        assert!(!log.is_empty());

        mirror.mirror(dom.snapshot(&log));
        assert_in_step(&mirror, &dom);

        let after = dom.clone();
        dom = before;
        mirror.mirror(dom.snapshot(&log));
        assert_in_step(&mirror, &dom);

        dom = after;
        mirror.mirror(dom.snapshot(&log));
        assert_in_step(&mirror, &dom);
        assert!(
            mirror.changes.is_empty(),
            "a mirror's edits are not edits of its own"
        );
    }

    #[test]
    fn a_property_write_and_a_rename_follow() {
        round_trip(|dom, fixture| {
            let [a, ..] = fixture.children;
            dom.set_property(a, "Transparency", Variant::Float32(0.5))
                .unwrap();
            dom.set_name(a, "Renamed").unwrap();
        });
    }

    #[test]
    fn a_class_change_follows_and_undoes() {
        round_trip(|dom, fixture| {
            let [a, ..] = fixture.children;
            dom.set_class(a, "WedgePart").unwrap();
            dom.set_class(fixture.model, "Folder").unwrap();
        });
    }

    #[test]
    fn an_insert_with_its_setup_writes_follows_and_undoes() {
        round_trip(|dom, fixture| {
            let inserted = part(dom, "D", Some(fixture.model));
            dom.new_instance("Decal", "Decal", Some(inserted));
        });
    }

    // The middle child goes, so its undo has to put it back *between* its
    // siblings rather than after them.
    #[test]
    fn a_subtree_delete_follows_and_its_undo_restores_sibling_order() {
        round_trip(|dom, fixture| {
            let [_, b, _] = fixture.children;
            dom.new_instance("Decal", "Decal", Some(b));
            dom.take_changes();
            dom.remove(b);
        });
    }

    #[test]
    fn a_reparent_follows_and_its_undo_puts_the_child_back_in_place() {
        round_trip(|dom, fixture| {
            let [_, b, _] = fixture.children;
            dom.set_parent(b, Some(fixture.storage));
        });
    }

    #[test]
    fn a_move_to_the_root_and_back_follows() {
        round_trip(|dom, fixture| {
            dom.set_parent(fixture.model, None);
        });
    }

    #[test]
    fn two_moves_in_one_log_leave_every_parent_exact() {
        round_trip(|dom, fixture| {
            let [a, _, c] = fixture.children;
            let folder = dom.new_instance("Folder", "Folder", Some(fixture.storage));
            dom.set_parent(a, Some(folder));
            dom.set_parent(a, Some(fixture.storage));
            dom.set_parent(c, Some(fixture.model));
        });
    }

    #[test]
    fn an_instance_inserted_and_removed_in_one_log_never_reaches_the_mirror() {
        round_trip(|dom, fixture| {
            let short_lived = part(dom, "Gone", Some(fixture.model));
            dom.remove(short_lived);
        });
    }

    // What the hand-off costs: one instance for a write, the parents too
    // for a move — never the tree.
    #[test]
    fn a_snapshot_copies_the_instances_the_log_names_and_no_more() {
        let (mut dom, fixture) = place();
        let [a, ..] = fixture.children;
        dom.set_property(a, "Transparency", Variant::Float32(1.0))
            .unwrap();
        let log = dom.take_changes();
        let snapshots = dom.snapshot(&log);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].referent, a);

        dom.set_parent(a, Some(fixture.storage));
        let log = dom.take_changes();
        let snapshots = dom.snapshot(&log);
        let named: HashSet<Ref> = snapshots.iter().map(|s| s.referent).collect();
        assert_eq!(named, HashSet::from([a, fixture.model, fixture.storage]));
    }
}
