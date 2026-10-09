//! The `Animate` LocalScript of the R6 and R15 rigs: our own source
//! (`animate.luau`), and one `StringValue` per animation state holding its `Animation`
//! children with the default ids, so a new rig walks, jumps and idles when
//! it is played. Ids and layout are those of a Studio rig.

use rbx_dom::{Content, Ref, Variant, WeakDom};

use crate::shell::clipboard;

const SOURCE: &str = include_str!("animate.luau");

/// `(animation name, id, weight)`; a weight is set when a state has several.
type Entry = (&'static str, u64, Option<f64>);
type State = (&'static str, &'static [Entry]);

const R15: &[State] = &[
    ("cheer", &[("CheerAnim", 507770677, None)]),
    ("climb", &[("ClimbAnim", 507765644, None)]),
    (
        "dance",
        &[
            ("Animation1", 507771019, Some(10.)),
            ("Animation2", 507771955, Some(10.)),
            ("Animation3", 507772104, Some(10.)),
        ],
    ),
    (
        "dance2",
        &[
            ("Animation1", 507776043, Some(10.)),
            ("Animation2", 507776720, Some(10.)),
            ("Animation3", 507776879, Some(10.)),
        ],
    ),
    (
        "dance3",
        &[
            ("Animation1", 507777268, Some(10.)),
            ("Animation2", 507777451, Some(10.)),
            ("Animation3", 507777623, Some(10.)),
        ],
    ),
    ("fall", &[("FallAnim", 507767968, None)]),
    (
        "idle",
        &[
            ("Animation1", 507766388, Some(9.)),
            ("Animation2", 507766666, Some(1.)),
        ],
    ),
    ("jump", &[("JumpAnim", 507765000, None)]),
    ("laugh", &[("LaughAnim", 507770818, None)]),
    ("point", &[("PointAnim", 507770453, None)]),
    ("sit", &[("SitAnim", 2506281703, None)]),
    ("toollunge", &[("ToolLungeAnim", 522638767, None)]),
    ("toolnone", &[("ToolNoneAnim", 507768375, None)]),
    ("toolslash", &[("ToolSlashAnim", 522635514, None)]),
    ("wave", &[("WaveAnim", 507770239, None)]),
    ("run", &[("RunAnim", 913376220, None)]),
    ("swim", &[("Swim", 913384386, None)]),
    ("swimidle", &[("SwimIdle", 913389285, None)]),
    ("walk", &[("WalkAnim", 913402848, None)]),
    ("mood", &[("Animation1", 14366558676, None)]),
];

const R6: &[State] = &[
    (
        "idle",
        &[
            ("Animation1", 180435571, Some(9.)),
            ("Animation2", 180435792, Some(1.)),
        ],
    ),
    ("walk", &[("WalkAnim", 180426354, None)]),
    ("run", &[("RunAnim", 180426354, None)]),
    ("jump", &[("JumpAnim", 125750702, None)]),
    ("climb", &[("ClimbAnim", 180436334, None)]),
    ("toolnone", &[("ToolNoneAnim", 182393478, None)]),
    ("fall", &[("FallAnim", 180436148, None)]),
    ("sit", &[("SitAnim", 178130996, None)]),
];

fn animation_url(id: u64) -> Variant {
    Variant::Content(Content::Uri(format!(
        "http://www.roblox.com/asset/?id={id}"
    )))
}

/// Adds `Animate` under `model`.
pub(super) fn add(dom: &mut WeakDom, model: Ref, r15: bool) {
    let states = if r15 { R15 } else { R6 };
    let script = dom.new_instance("LocalScript", "Animate", Some(model));
    let _ = dom.set_property(script, "Source", Variant::String(SOURCE.into()));
    for (state, entries) in states {
        let value = dom.new_instance("StringValue", state, Some(script));
        for (name, id, weight) in *entries {
            let animation = dom.new_instance("Animation", name, Some(value));
            let _ = dom.set_property(animation, "AnimationId", animation_url(*id));
            if let Some(weight) = weight {
                let number = dom.new_instance("NumberValue", "Weight", Some(animation));
                let _ = dom.set_property(number, "Value", Variant::Float64(*weight));
            }
        }
    }
    dom.new_instance("BindableFunction", "PlayEmote", Some(script));
    let number = dom.new_instance("NumberValue", "ScaleDampeningPercent", Some(script));
    let _ = dom.set_property(number, "Value", Variant::Float64(1.));
}

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

    #[test]
    fn every_state_folder_has_an_animation() {
        for (state, entries) in R15.iter().chain(R6) {
            assert!(!entries.is_empty(), "{state}");
        }
    }
}
