//! Roblox's UI styling engine: the `StyleSheet`/`StyleRule`/`StyleLink`
//! family, applied to a DOM before [`super::plan`] reads it.
//!
//! A `StyleLink` names one `StyleSheet` and applies it to the tree rooted at
//! the link's own parent (`StyleLink`, creator-docs). The sheet's `StyleRule`
//! children — plus those of every sheet it derives from, through `StyleDerive`
//! — each carry a selector and a bag of property overrides, and the ones whose
//! selector matches an instance overwrite that instance's properties.
//!
//! # What a saved file cannot say
//! Studio distinguishes a *styled* property from one a designer then
//! *overrode* on the instance itself, and the override wins ("Modified
//! properties", ui/styling). Neither the flag nor the class defaults that
//! would reveal it survive into a place file, so here a matching rule always
//! wins over the instance's stored value.

use std::collections::{BTreeMap, HashMap, HashSet};

use rbx_dom::{Ref, Variant, WeakDom};

mod cascade;
mod matcher;
mod selector;

const LINK_CLASS: &str = "StyleLink";

/// The DOM's properties as the style sheets leave them.
///
/// Only instances a rule actually matched are held; everything else reads
/// straight off the instance, so a place with no `StyleLink` in it costs one
/// walk of the tree and nothing more.
#[derive(Default)]
pub(super) struct Styled(HashMap<Ref, BTreeMap<String, Variant>>);

impl Styled {
    pub(super) fn new(dom: &WeakDom) -> Self {
        let mut styled = Styled::default();
        for &root in dom.root_refs() {
            styled.gather(dom, root);
        }
        styled
    }

    /// [`Styled::new`] for one tree alone: the instances under `root`
    /// (itself included) come out exactly as the whole-DOM pass would leave
    /// them, and nothing else is walked but the path down to `root` — a
    /// link applies to its parent's tree, so only one on `root`'s path or
    /// inside it can reach it. The walk keeps the whole pass's order, which
    /// is what decides which of two colliding links wins.
    pub(super) fn for_tree(dom: &WeakDom, root: Ref) -> Self {
        let mut path = HashSet::new();
        let mut up = dom.parent(root);
        while let Some(ancestor) = up {
            path.insert(ancestor);
            up = dom.parent(ancestor);
        }
        let mut styled = Styled::default();
        for &top in dom.root_refs() {
            styled.gather_toward(dom, top, root, &path);
        }
        styled
    }

    fn gather_toward(&mut self, dom: &WeakDom, referent: Ref, root: Ref, path: &HashSet<Ref>) {
        let Some(instance) = dom.get(referent) else {
            return;
        };
        if referent == root {
            self.gather(dom, referent);
            return;
        }
        if instance.class() == LINK_CLASS {
            if let (Some(sheet), Some(scope)) = (cascade::reference(instance), dom.parent(referent))
            {
                if path.contains(&scope) {
                    self.apply_from(dom, scope, root, &cascade::flatten(dom, sheet));
                }
            }
            return;
        }
        if !path.contains(&referent) {
            return;
        }
        for &child in instance.children() {
            self.gather_toward(dom, child, root, path);
        }
    }

    /// `instance`'s properties with every rule that matched it applied.
    pub(super) fn properties_of<'a>(
        &'a self,
        instance: &'a rbx_dom::Instance,
    ) -> &'a BTreeMap<String, Variant> {
        self.0
            .get(&instance.referent())
            .unwrap_or_else(|| instance.properties())
    }

    /// Finds every `StyleLink` and applies its sheet to its parent's tree.
    fn gather(&mut self, dom: &WeakDom, referent: Ref) {
        let Some(instance) = dom.get(referent) else {
            return;
        };
        if instance.class() == LINK_CLASS {
            // "Only one StyleSheet can apply to a given tree" (ui/styling), so
            // a second link on the same parent is simply another tree's worth
            // of work; the later one wins where they collide.
            if let (Some(sheet), Some(root)) = (cascade::reference(instance), dom.parent(referent))
            {
                self.apply(dom, root, &cascade::flatten(dom, sheet));
            }
            return;
        }
        for &child in instance.children() {
            self.gather(dom, child);
        }
    }

    /// Every instance in `root`'s tree against every rule.
    ///
    /// ponytail: O(instances × rules) per link; a place carries a handful of
    /// rules, and the alternative is indexing them by class/tag/name, which
    /// only pays once a sheet runs to hundreds.
    fn apply(&mut self, dom: &WeakDom, root: Ref, rules: &[cascade::Rule]) {
        self.apply_from(dom, root, root, rules);
    }

    /// [`Styled::apply`] over `start`'s tree alone, inside a link scoped to
    /// `root` — the selectors still read their ancestry from `root`.
    fn apply_from(&mut self, dom: &WeakDom, root: Ref, start: Ref, rules: &[cascade::Rule]) {
        let mut stack = vec![start];
        while let Some(referent) = stack.pop() {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            stack.extend_from_slice(instance.children());
            for rule in rules {
                let matched = rule
                    .selectors
                    .iter()
                    .any(|selector| matcher::matches(dom, root, referent, selector));
                if !matched || rule.properties.is_empty() {
                    continue;
                }
                self.0
                    .entry(referent)
                    .or_insert_with(|| instance.properties().clone())
                    .extend(rule.properties.iter().map(|(n, v)| (n.clone(), v.clone())));
            }
        }
    }
}
