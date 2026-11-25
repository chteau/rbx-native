//! Roblox's UI styling rules, shared by the renderer (which applies a
//! place's sheets before drawing) and Local Run (`GetStyled` at run time):
//! the `StyleRule.Selector` grammar ([`selector`]), whether a selector
//! picks an instance ([`matcher`]), and a `StyleSheet` with its derives,
//! nested rules and tokens flattened into ordered rules ([`cascade`]).

use std::collections::BTreeMap;

use crate::{Ref, Variant, WeakDom};

pub mod cascade;
pub mod matcher;
pub mod selector;

/// The properties the style sheets give `referent`: every `StyleLink` on
/// its path from the top applies its sheet to the link's parent's tree, a
/// deeper link's rules after a shallower one's, each sheet's strongest
/// rule last.
pub fn styled(dom: &WeakDom, referent: Ref) -> BTreeMap<String, Variant> {
    let mut path = Vec::new();
    let mut at = Some(referent);
    while let Some(r) = at {
        path.push(r);
        at = dom.parent(r);
    }
    let mut out = BTreeMap::new();
    for &scope in path.iter().rev() {
        let Some(instance) = dom.get(scope) else {
            continue;
        };
        for &child in instance.children() {
            let Some(link) = dom.get(child).filter(|l| l.class() == "StyleLink") else {
                continue;
            };
            let Some(sheet) = cascade::reference(link).filter(|s| dom.get(*s).is_some()) else {
                continue;
            };
            for rule in cascade::flatten(dom, sheet) {
                let matched = rule
                    .selectors
                    .iter()
                    .any(|selector| matcher::matches(dom, scope, referent, selector));
                if matched {
                    out.extend(rule.properties);
                }
            }
        }
    }
    out
}
