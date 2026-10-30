//! Roblox's UI styling engine: the `StyleSheet`/`StyleRule`/`StyleLink`
//! family, applied to a DOM before [`super::plan`] reads it.
//!
//! A `StyleLink` names one `StyleSheet` and applies it to the tree rooted at
//! the link's own parent (`StyleLink`, creator-docs). The sheet's `StyleRule`
//! children, plus those of every sheet it derives from, through `StyleDerive`
//! each carry a selector and a bag of property overrides, and the ones whose
//! selector matches an instance overwrite that instance's properties.
//!
//! # What a saved file cannot say
//! Studio distinguishes a *styled* property from one a designer then
//! *overrode* on the instance itself, and the override wins ("Modified
//! properties", ui/styling). Neither the flag nor the class defaults that
//! would reveal it survive into a place file, so here a matching rule always
//! wins over the instance's stored value.

use std::collections::{BTreeMap, HashMap, HashSet};

use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

mod cascade;
mod matcher;
mod selector;

const LINK_CLASS: &str = "StyleLink";

/// The class whose omitted properties [`Styled`] fills in: every
/// `LayerCollector` and `GuiObject`. A `UIComponent` keeps its readers' own
/// fallbacks, since several of them pair a property with the ones it was
/// split into (`UICorner.CornerRadius` and its four corners), and a default
/// for the newer half would outvote an older file's stored value.
const GUI_ROOT: &str = "GuiBase";

/// The DOM's properties as the style sheets leave them, with every GUI
/// property the file left out at its class default.
///
/// Only instances a rule matched or a file left short are held; everything
/// else reads straight off the instance, so a Studio-saved place with no
/// `StyleLink` in it costs one walk of the tree and nothing more.
pub(super) struct Styled<'db> {
    database: &'db ReflectionDatabase,
    styled: HashMap<Ref, BTreeMap<String, Variant>>,
}

impl<'db> Styled<'db> {
    pub(super) fn new(dom: &WeakDom, database: &'db ReflectionDatabase) -> Self {
        let mut styled = Styled::empty(database);
        for &root in dom.root_refs() {
            styled.gather(dom, root);
        }
        styled
    }

    fn empty(database: &'db ReflectionDatabase) -> Self {
        Styled {
            database,
            styled: HashMap::new(),
        }
    }

    /// [`Styled::new`] for one tree alone: the instances under `root`
    /// (itself included) come out exactly as the whole-DOM pass would leave
    /// them, and nothing else is walked but the path down to `root`, a
    /// link applies to its parent's tree, so only one on `root`'s path or
    /// inside it can reach it. The walk keeps the whole pass's order, which
    /// is what decides which of two colliding links wins.
    pub(super) fn for_tree(dom: &WeakDom, database: &'db ReflectionDatabase, root: Ref) -> Self {
        let mut path = HashSet::new();
        let mut up = dom.parent(root);
        while let Some(ancestor) = up {
            path.insert(ancestor);
            up = dom.parent(ancestor);
        }
        let mut styled = Styled::empty(database);
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
        self.styled
            .get(&instance.referent())
            .unwrap_or_else(|| instance.properties())
    }

    /// Roblox reads a property a file leaves out as the class default, what
    /// `Instance.new` gives (a grey `TextButton` in `Legacy`), not what
    /// Studio's Insert sets on a new one (white, `BuilderSans`). A file that
    /// still carries the old `Font` enum keeps it over the default
    /// `FontFace`, which would otherwise win (see `plan::text::face`). Only
    /// what the API dump lists is filled: a hidden property only a file holds
    /// (`ViewportFrame.CameraCFrame`) says how the file was written, and one
    /// left out means nothing was.
    fn fill_defaults(&mut self, instance: &Instance) {
        let class = instance.class();
        if !self.database.is_subclass_of(class, GUI_ROOT) {
            return;
        }
        let properties = instance.properties();
        let mut missing = self
            .database
            .default_values(class)
            .filter(|&(name, _)| {
                !properties.contains_key(name)
                    && !(name == "FontFace" && properties.contains_key("Font"))
                    && self.database.resolve_property(class, name).is_some()
            })
            .peekable();
        if missing.peek().is_none() {
            return;
        }
        let filled = self
            .styled
            .entry(instance.referent())
            .or_insert_with(|| properties.clone());
        for (name, value) in missing {
            filled
                .entry(name.to_owned())
                .or_insert_with(|| value.clone());
        }
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
        self.fill_defaults(instance);
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
    /// `root`: the selectors still read their ancestry from `root`.
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
                self.styled
                    .entry(referent)
                    .or_insert_with(|| instance.properties().clone())
                    .extend(rule.properties.iter().map(|(n, v)| (n.clone(), v.clone())));
            }
        }
    }
}
