//! The Roblox enums node properties hold, keyed by the property names that
//! hold them. Roblox stores these as `EnumItem` attributes; the model keeps
//! the item's name as text, which is what the editor and the runtime read.

pub struct EnumType {
    /// The `Enum.<name>` the attribute's `EnumItem` belongs to.
    pub name: &'static str,
    /// Item names with their values, as the API dump numbers them.
    pub items: &'static [(&'static str, u32)],
}

impl EnumType {
    pub fn value(&self, item: &str) -> Option<u32> {
        self.items.iter().find(|(n, _)| *n == item).map(|(_, v)| *v)
    }

    pub fn item(&self, value: u32) -> Option<&'static str> {
        self.items
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(n, _)| *n)
    }
}

pub static PLAY_MODE: EnumType = EnumType {
    name: "AnimationNodePlayMode",
    items: &[
        ("Loop", 0),
        ("PingPong", 1),
        ("OnceAndHold", 2),
        ("OnceAndReset", 3),
    ],
};

pub static PHASE_SYNC: EnumType = EnumType {
    name: "AnimationNodePhaseSync",
    items: &[("Synced", 0), ("Unsynced", 1)],
};

pub static INPUT_MODE: EnumType = EnumType {
    name: "AnimationNodeBlend2DInputMode",
    items: &[("Cartesian", 0), ("Polar", 1)],
};

pub static BLEND_MODE: EnumType = EnumType {
    name: "AnimationNodeBlendMode",
    items: &[("Over", 0), ("Add", 1), ("Subtract", 2)],
};

pub static INTERRUPTIBLE: EnumType = EnumType {
    name: "AnimationNodeInterruptible",
    items: &[("Always", 0), ("Finished", 1), ("Trigger", 2)],
};

pub static WAIT_FOR: EnumType = EnumType {
    name: "AnimationNodeWaitFor",
    items: &[("Finished", 0), ("Trigger", 1)],
};

pub static WHEN: EnumType = EnumType {
    name: "AnimationNodeTransitionWhen",
    items: &[("Finished", 0), ("BeforeFinished", 1)],
};

/// Every item exists, but the runtime honours only Linear and CubicV2 (the
/// editor's "In/Out") and treats the rest as Linear.
pub static CURVE: EnumType = EnumType {
    name: "PoseEasingStyle",
    items: &[
        ("Linear", 0),
        ("Constant", 1),
        ("Elastic", 2),
        ("Cubic", 3),
        ("Bounce", 4),
        ("CubicV2", 5),
    ],
};

/// The enum a node, input-pin or transition attribute holds, if any.
pub fn of_prop(prop: &str) -> Option<&'static EnumType> {
    Some(match prop {
        "PlayMode" => &PLAY_MODE,
        "PhaseSync" => &PHASE_SYNC,
        "InputMode" => &INPUT_MODE,
        "BlendMode" => &BLEND_MODE,
        "DefaultInterruptible" | "TransitionOverrideInterruptible" => &INTERRUPTIBLE,
        "DefaultWaitFor" | "TransitionOverrideWaitFor" | "WaitFor" => &WAIT_FOR,
        "DefaultTransitionWhen" | "TransitionOverrideWhen" | "TransitionOutWhen" | "When" => &WHEN,
        "DefaultTransitionCurve"
        | "TransitionOverrideCurve"
        | "TransitionInCurve"
        | "TransitionOutCurve"
        | "Curve" => &CURVE,
        _ => return None,
    })
}
