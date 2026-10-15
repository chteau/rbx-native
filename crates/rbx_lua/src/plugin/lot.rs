//! Parking the plugins' own instances outside the place between
//! dispatches, and turning a dispatch's raw change log into what the editor
//! should see.

use std::collections::HashSet;

use rbx_dom::{Change, Ref, WeakDom};

/// The host's parked instances: every plugin-owned root and its subtree,
/// held as the roots of a DOM of their own with their referents unchanged.
#[derive(Default)]
pub(crate) struct Lot {
    pub(crate) parked: WeakDom,
}

/// What [`Lot::merge`] saw, for [`Lot::split`].
pub(crate) struct Merge {
    /// Every referent that was parked (roots and descendants).
    pub(crate) hidden: HashSet<Ref>,
    /// The place's own roots before the merge.
    pub(crate) place_roots: HashSet<Ref>,
}

impl Lot {
    /// Moves every parked subtree into `dom` as roots (see
    /// `WeakDom::move_subtrees`; nothing is logged). A parked root whose
    /// referent the place already uses stays parked (it cannot happen while
    /// the editor keeps its allocator monotonic).
    pub(crate) fn merge(&mut self, dom: &mut WeakDom) -> Merge {
        let place_roots = dom.root_refs().iter().copied().collect();
        let roots = self.parked.root_refs().to_vec();
        let subtrees: Vec<Vec<Ref>> = roots
            .iter()
            .map(|&root| subtree(&self.parked, root))
            .collect();
        let left: HashSet<Ref> = self.parked.move_subtrees(&roots, dom).into_iter().collect();
        let hidden = roots
            .iter()
            .zip(subtrees)
            .filter(|(root, _)| !left.contains(root))
            .flat_map(|(_, refs)| refs)
            .collect();
        Merge {
            hidden,
            place_roots,
        }
    }

    /// Parks every root of `dom` that is not in `merge.place_roots` (the
    /// plugin roots merged back, plus whatever a plugin created or
    /// parented to `nil`), then returns `log` rewritten for the editor:
    /// - a change about an instance hidden before (or created during the
    ///   dispatch) and hidden after is dropped;
    /// - an instance visible before and parked now is reported `Removed`,
    ///   itself and every descendant, once, at its first mention;
    /// - an instance hidden before and visible now (a plugin parented it
    ///   into the place) is reported `Added`, itself and every descendant,
    ///   once, at its first mention, and its `Parent` change dropped;
    /// - `Added` for an instance visible now is kept, `Removed` for one
    ///   visible before is kept, any other change is kept only when the
    ///   instance is visible both before (or was added) and after.
    /// Order is otherwise the log's.
    pub(crate) fn split(
        &mut self,
        dom: &mut WeakDom,
        merge: Merge,
        log: Vec<Change>,
    ) -> Vec<Change> {
        let park: Vec<Ref> = dom
            .root_refs()
            .iter()
            .copied()
            .filter(|root| !merge.place_roots.contains(root))
            .collect();
        let hidden_after = park
            .iter()
            .flat_map(|&root| subtree(dom, root))
            .collect();
        let created = log
            .iter()
            .filter_map(|change| match change {
                Change::Added(referent) => Some(*referent),
                _ => None,
            })
            .collect();
        let view = View {
            hidden_before: merge.hidden,
            hidden_after,
            created,
        };
        let rewritten = view.rewrite(dom, log);
        dom.move_subtrees(&park, &mut self.parked);
        rewritten
    }
}

/// Which instances the editor could see before a dispatch and can see after.
struct View {
    hidden_before: HashSet<Ref>,
    hidden_after: HashSet<Ref>,
    /// Referents the log reports `Added`: they did not exist before.
    created: HashSet<Ref>,
}

impl View {
    fn visible_before(&self, referent: Ref) -> bool {
        !self.hidden_before.contains(&referent) && !self.created.contains(&referent)
    }

    fn visible_after(&self, dom: &WeakDom, referent: Ref) -> bool {
        dom.get(referent).is_some() && !self.hidden_after.contains(&referent)
    }

    fn rewrite(&self, dom: &WeakDom, log: Vec<Change>) -> Vec<Change> {
        let mut out = Vec::with_capacity(log.len());
        let mut removed: HashSet<Ref> = HashSet::new();
        let mut announced: HashSet<Ref> = HashSet::new();
        for change in log {
            let referent = match &change {
                Change::Property { referent, .. } | Change::Parent { referent, .. } => *referent,
                Change::Class(referent) | Change::Added(referent) | Change::Removed(referent) => {
                    *referent
                }
            };
            let after = self.visible_after(dom, referent);
            match change {
                Change::Removed(_) => {
                    if self.visible_before(referent) && removed.insert(referent) {
                        out.push(Change::Removed(referent));
                    }
                    continue;
                }
                Change::Added(_) => {
                    if after && announced.insert(referent) {
                        out.push(Change::Added(referent));
                    }
                    continue;
                }
                _ => {}
            }
            if self.visible_before(referent) && dom.get(referent).is_some() && !after {
                for gone in subtree(dom, referent) {
                    if self.visible_before(gone)
                        && !self.visible_after(dom, gone)
                        && removed.insert(gone)
                    {
                        out.push(Change::Removed(gone));
                    }
                }
                continue;
            }
            if self.hidden_before.contains(&referent) && after && !announced.contains(&referent) {
                for shown in subtree(dom, referent) {
                    if self.hidden_before.contains(&shown)
                        && self.visible_after(dom, shown)
                        && announced.insert(shown)
                    {
                        out.push(Change::Added(shown));
                    }
                }
                if matches!(change, Change::Parent { .. }) {
                    continue;
                }
            }
            if after && (self.visible_before(referent) || announced.contains(&referent)) {
                out.push(change);
            }
        }
        out
    }
}

/// Every referent in the subtree under `root` (root included), in `dom`.
pub(crate) fn subtree(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut refs = Vec::new();
    let mut stack = vec![root];
    while let Some(current) = stack.pop() {
        if let Some(instance) = dom.get(current) {
            refs.push(current);
            stack.extend(instance.children().iter().rev());
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A place with a `Workspace`, and a plugin folder (with one child)
    /// already parked in a lot.
    fn setup() -> (WeakDom, Lot, Ref, Ref, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let folder = dom.new_instance("Folder", "Plugin", None);
        let child = dom.new_instance("Part", "Inner", Some(folder));
        let mut lot = Lot::default();
        dom.move_subtrees(&[folder], &mut lot.parked);
        dom.take_changes();
        (dom, lot, workspace, folder, child)
    }

    #[test]
    fn parked_folder_is_merged_and_parked_again() {
        let (mut dom, mut lot, workspace, folder, child) = setup();
        let merge = lot.merge(&mut dom);
        assert!(merge.hidden.contains(&folder) && merge.hidden.contains(&child));
        assert_eq!(dom.root_refs(), &[workspace, folder]);
        assert!(dom.take_changes().is_empty());

        let log = dom.take_changes();
        assert!(lot.split(&mut dom, merge, log).is_empty());
        assert_eq!(dom.root_refs(), &[workspace]);
        assert_eq!(lot.parked.root_refs(), &[folder]);
        assert!(lot.parked.get(child).is_some());
    }

    #[test]
    fn root_instance_created_in_a_dispatch_is_parked_silently() {
        let (mut dom, mut lot, workspace, _, _) = setup();
        let merge = lot.merge(&mut dom);
        let made = dom.new_instance("Folder", "Made", None);
        let log = dom.take_changes();
        assert_eq!(log, vec![Change::Added(made)]);

        assert!(lot.split(&mut dom, merge, log).is_empty());
        assert_eq!(dom.root_refs(), &[workspace]);
        assert!(lot.parked.get(made).is_some());
    }

    #[test]
    fn place_part_parked_is_reported_removed_with_its_child() {
        let (mut dom, mut lot, workspace, _, _) = setup();
        let part = dom.new_instance("Part", "Part", Some(workspace));
        let inner = dom.new_instance("Part", "Inner", Some(part));
        dom.take_changes();
        let merge = lot.merge(&mut dom);
        dom.set_parent(part, None);
        let log = dom.take_changes();

        let out = lot.split(&mut dom, merge, log);
        assert_eq!(out, vec![Change::Removed(part), Change::Removed(inner)]);
        assert!(lot.parked.get(inner).is_some());
        assert!(dom.get(part).is_none());
    }

    #[test]
    fn parked_folder_moved_into_the_place_is_reported_added() {
        let (mut dom, mut lot, workspace, folder, child) = setup();
        let merge = lot.merge(&mut dom);
        dom.set_parent(folder, Some(workspace));
        let log = dom.take_changes();

        let out = lot.split(&mut dom, merge, log);
        assert_eq!(out, vec![Change::Added(folder), Change::Added(child)]);
        assert_eq!(dom.parent(folder), Some(workspace));
        assert!(lot.parked.root_refs().is_empty());
    }

    #[test]
    fn property_changes_follow_visibility_in_order() {
        let (mut dom, mut lot, workspace, folder, _) = setup();
        let part = dom.new_instance("Part", "Part", Some(workspace));
        dom.take_changes();
        let merge = lot.merge(&mut dom);
        dom.set_name(part, "A").unwrap();
        dom.set_name(folder, "Hidden").unwrap();
        dom.set_name(workspace, "B").unwrap();
        dom.set_name(part, "C").unwrap();
        let log = dom.take_changes();

        let out = lot.split(&mut dom, merge, log);
        let name = |referent| Change::Property {
            referent,
            name: "Name".to_string(),
        };
        assert_eq!(out, vec![name(part), name(workspace), name(part)]);
    }
}
