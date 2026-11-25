//! The `Animate` LocalScript of the R6 and R15 rigs. The script and its
//! default-id tables live in `rbx_sim::character::animate`, shared with
//! Local Run; what stays here is replacing it on a rig in the editor.

use rbx_dom::{Ref, WeakDom};

use crate::shell::clipboard;

#[cfg(test)]
use rbx_sim::character::animate::SOURCE;
pub(crate) use rbx_sim::character::animate::{add, r15_animation};

/// Replaces the `Animation`s of every `Animate` state that the animation
/// package `source` names (its folder holds one `StringValue` per state:
/// `idle`, `walk`, ...), as a `HumanoidDescription` does. Returns how many
/// states changed.
pub(crate) fn replace(dom: &mut WeakDom, model: Ref, source: &WeakDom) -> usize {
    let Some(script) = child(dom, model, "Animate") else {
        return 0;
    };
    let mut changed = 0;
    for root in source.root_refs() {
        let mut pending = vec![*root];
        while let Some(next) = pending.pop() {
            let Some(node) = source.get(next) else {
                continue;
            };
            pending.extend(node.children());
            if node.class() != "StringValue" || node.children().is_empty() {
                continue;
            }
            let Some(state) = child(dom, script, node.name()) else {
                continue;
            };
            let old = dom
                .get(state)
                .map_or_else(Vec::new, |v| v.children().to_vec());
            for animation in old {
                dom.remove(animation);
            }
            for &animation in node.children() {
                clipboard::graft(dom, source, animation, state);
            }
            changed += 1;
        }
    }
    changed
}

fn child(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|c| c.name() == name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_parses_as_strict_luau() {
        assert!(SOURCE.starts_with("--!strict"));
        full_moon::parse(SOURCE).expect("animate.luau parses");
    }
}
