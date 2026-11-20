//! The `Animate` LocalScript of the R6 and R15 rigs: our own source
//! (`Animate.luau`), and one `StringValue` per animation state holding its
//! `Animation` children with the default ids, so a character walks, jumps
//! and idles when it is played. The ids are loaded at runtime (from Roblox
//! or the asset cache); no animation file is bundled.

use rbx_dom::{Content, Ref, Variant, WeakDom};

/// Our own `Animate` script.
pub const SOURCE: &str = include_str!("Animate.luau");

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
    ("wave", &[("WaveAnim", 128777973, None)]),
    ("point", &[("PointAnim", 128853357, None)]),
    (
        "dance",
        &[
            ("Animation1", 182435998, Some(10.)),
            ("Animation2", 182491037, Some(10.)),
            ("Animation3", 182491065, Some(10.)),
        ],
    ),
    (
        "dance2",
        &[
            ("Animation1", 182436842, Some(10.)),
            ("Animation2", 182491248, Some(10.)),
            ("Animation3", 182491277, Some(10.)),
        ],
    ),
    (
        "dance3",
        &[
            ("Animation1", 182436935, Some(10.)),
            ("Animation2", 182491368, Some(10.)),
            ("Animation3", 182491423, Some(10.)),
        ],
    ),
    ("laugh", &[("LaughAnim", 129423131, None)]),
    ("cheer", &[("CheerAnim", 129423030, None)]),
];

fn animation_url(id: u64) -> Variant {
    Variant::Content(Content::Uri(format!(
        "http://www.roblox.com/asset/?id={id}"
    )))
}

/// The stock R15 `Animate` script's animation id for `state` (`idle`,
/// `walk`, ...): its first entry, the one that plays most.
pub fn r15_animation(state: &str) -> Option<u64> {
    R15.iter()
        .find(|(name, _)| *name == state)
        .and_then(|(_, entries)| entries.first())
        .map(|(_, id, _)| *id)
}

/// Adds `Animate` under `model`.
pub fn add(dom: &mut WeakDom, model: Ref, r15: bool) {
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
