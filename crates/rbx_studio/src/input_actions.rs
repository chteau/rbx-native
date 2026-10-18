//! The Input Action System as the Input Actions dock reads it: every
//! `InputContext` in the place with its `InputAction`s and their
//! `InputBinding`s, each binding sorted into the device families its keys
//! belong to (`input/input-action-system.md`: every action should have a
//! binding for keyboard/mouse, gamepad and touch).
//!
//! Property and enum names are the API dump's; values the place does not
//! hold read as the reflection defaults, the same way the Properties panel
//! shows them. Nothing here writes: the dock edits through
//! `properties::edit::commit` like any other panel.

pub(crate) mod check;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

pub(crate) const CONTEXT_CLASS: &str = "InputContext";
pub(crate) const ACTION_CLASS: &str = "InputAction";
pub(crate) const BINDING_CLASS: &str = "InputBinding";

/// The binding properties that name an input, in the order a summary lists
/// them: the plain `KeyCode`, then the composite directions.
pub(crate) const KEY_PROPERTIES: [&str; 7] = [
    "KeyCode", "Up", "Down", "Left", "Right", "Forward", "Backward",
];
/// The two `KeyCode` modifiers a binding can require.
pub(crate) const MODIFIER_PROPERTIES: [&str; 2] = ["PrimaryModifier", "SecondaryModifier"];

/// A family of devices, one column of the dock's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Family {
    KeyboardMouse,
    Gamepad,
    Touch,
}

impl Family {
    pub(crate) const ALL: [Family; 3] = [Family::KeyboardMouse, Family::Gamepad, Family::Touch];

    /// The column header: "Keyboard & Mouse", "Gamepad", "Touch".
    pub(crate) fn label(self) -> &'static str {
        match self {
            Family::KeyboardMouse => "Keyboard & Mouse",
            Family::Gamepad => "Gamepad",
            Family::Touch => "Touch",
        }
    }
}

/// `InputAction.Type`, by the `InputActionType` item names in the dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionKind {
    Bool,
    Direction1D,
    Direction2D,
    Direction3D,
    ViewportPosition,
    /// A value newer than this build's dump knows.
    Other(u32),
}

impl ActionKind {
    /// From the stored `Enum` value: 0 Bool, 1 Direction1D, 2 Direction2D,
    /// 3 Direction3D, 4 ViewportPosition, anything else `Other`.
    pub(crate) fn from_value(value: u32) -> Self {
        match value {
            0 => Self::Bool,
            1 => Self::Direction1D,
            2 => Self::Direction2D,
            3 => Self::Direction3D,
            4 => Self::ViewportPosition,
            other => Self::Other(other),
        }
    }

    /// The families an action of this kind is expected to cover:
    /// `ViewportPosition` reads a pointer position, so keyboard/mouse and
    /// touch only; every other kind (including `Other`) all three.
    pub(crate) fn expected_families(self) -> &'static [Family] {
        match self {
            Self::ViewportPosition => &[Family::KeyboardMouse, Family::Touch],
            _ => &Family::ALL,
        }
    }
}

/// Which family a `KeyCode` item (by dump name) belongs to. `None` for
/// `"None"` and for names this function does not recognise.
/// Gamepad: names starting `Button`, `DPad` or `Thumbstick`.
/// Touch: names starting `Touch`.
/// Keyboard/mouse: every other item of the `KeyCode` enum in `db`
/// (keyboard keys, `Mouse*`, `Trackpad*`).
pub(crate) fn family_of_key(db: &ReflectionDatabase, name: &str) -> Option<Family> {
    if name == "None" || !db.enum_items("KeyCode")?.iter().any(|(n, _)| n == name) {
        return None;
    }
    Some(
        if ["Button", "DPad", "Thumbstick"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            Family::Gamepad
        } else if name.starts_with("Touch") {
            Family::Touch
        } else {
            Family::KeyboardMouse
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Binding {
    pub(crate) referent: Ref,
    pub(crate) name: String,
    /// `Type` is `Scriptable`: fired from a script, so no device family.
    pub(crate) scriptable: bool,
    /// Every entry of [`KEY_PROPERTIES`] whose value is not `None`, in that
    /// order, as `(property, KeyCode item name)`.
    pub(crate) keys: Vec<(&'static str, String)>,
    /// The non-`None` modifiers' item names, sorted.
    pub(crate) modifiers: Vec<String>,
    /// `UIButton`, when it points at an instance, and that instance's name.
    pub(crate) ui_button: Option<(Ref, String)>,
    /// `UIModifier`, when it points at an instance.
    pub(crate) ui_modifier: Option<Ref>,
    /// Sorted and deduplicated. Empty for a scriptable binding. Each key's
    /// [`family_of_key`], plus `Touch` when `ui_button` is set.
    pub(crate) families: Vec<Family>,
    pub(crate) pressed_threshold: f32,
    pub(crate) released_threshold: f32,
    pub(crate) response_curve: f32,
    pub(crate) scale: f32,
    /// `Vector2Scale` differs from (1, 1).
    pub(crate) vector2_scaled: bool,
    /// `Vector3Scale` differs from (1, 1, 1).
    pub(crate) vector3_scaled: bool,
}

impl Binding {
    /// One short line for a table cell. `"Scriptable"` for a scriptable
    /// binding; otherwise the modifiers joined with `+` and a trailing `+`
    /// (`"LeftControl+"`), then the `KeyCode` item if set, then the
    /// composite keys joined with `/` (separated from a `KeyCode` by `", "`),
    /// then `"Button: <name>"` for a `UIButton` (separated by `", "` from
    /// anything before it). `"Unbound"` when there is no key and no button.
    /// Examples: `"E"`, `"W/S/A/D"`, `"LeftControl+S"`, `"Button: Sprint"`.
    pub(crate) fn summary(&self) -> String {
        if self.scriptable {
            return "Scriptable".to_owned();
        }
        let mut parts = Vec::new();
        if let Some((_, key)) = self.keys.iter().find(|(p, _)| *p == "KeyCode") {
            parts.push(key.clone());
        }
        let composite: Vec<&str> = self
            .keys
            .iter()
            .filter(|(p, _)| *p != "KeyCode")
            .map(|(_, n)| n.as_str())
            .collect();
        if !composite.is_empty() {
            parts.push(composite.join("/"));
        }
        if let Some((_, name)) = &self.ui_button {
            parts.push(format!("Button: {name}"));
        }
        if parts.is_empty() {
            return "Unbound".to_owned();
        }
        let mut out = String::new();
        for m in &self.modifiers {
            out.push_str(m);
            out.push('+');
        }
        out.push_str(&parts.join(", "));
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Action {
    pub(crate) referent: Ref,
    pub(crate) name: String,
    pub(crate) kind: ActionKind,
    pub(crate) enabled: bool,
    /// The action's `InputBinding` children, in child order.
    pub(crate) bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Context {
    pub(crate) referent: Ref,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) priority: i32,
    pub(crate) sink: bool,
    /// Every `InputAction` descendant reached without passing through
    /// another `InputContext`, in depth-first child order.
    pub(crate) actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Model {
    /// Every `InputContext` in the place, depth-first from the roots, in
    /// child order — nested ones included, as their own entries.
    pub(crate) contexts: Vec<Context>,
    /// `InputAction`s with no `InputContext` ancestor, depth-first.
    pub(crate) loose: Vec<Action>,
}

impl Model {
    /// The context whose row a selected instance belongs to: the context
    /// itself, or the nearest one above an action or binding. `None` for
    /// anything else.
    pub(crate) fn context_of(&self, dom: &WeakDom, referent: Ref) -> Option<Ref> {
        let class = dom.get(referent)?.class();
        if ![CONTEXT_CLASS, ACTION_CLASS, BINDING_CLASS].contains(&class) {
            return None;
        }
        let mut current = Some(referent);
        while let Some(r) = current {
            if self.contexts.iter().any(|c| c.referent == r) {
                return Some(r);
            }
            current = dom.parent(r);
        }
        None
    }
}

fn enum_value(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, prop: &str) -> u32 {
    match value_of(dom, db, r, prop) {
        Some(Variant::Enum(v)) => *v,
        _ => 0,
    }
}

fn bool_value(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, prop: &str) -> bool {
    matches!(value_of(dom, db, r, prop), Some(Variant::Bool(true)))
}

fn float_value(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, prop: &str) -> f32 {
    match value_of(dom, db, r, prop) {
        Some(Variant::Float32(v)) => *v,
        _ => 0.0,
    }
}

fn key_name(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, prop: &str) -> Option<String> {
    let name = db.enum_name("KeyCode", enum_value(dom, db, r, prop))?;
    (name != "None").then(|| name.to_owned())
}

fn live_ref(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, prop: &str) -> Option<Ref> {
    match value_of(dom, db, r, prop) {
        Some(Variant::Ref(target)) if dom.get(*target).is_some() => Some(*target),
        _ => None,
    }
}

fn read_binding(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> Binding {
    let scriptable = enum_value(dom, db, r, "Type") == 1;
    let keys: Vec<(&'static str, String)> = KEY_PROPERTIES
        .iter()
        .filter_map(|p| key_name(dom, db, r, p).map(|n| (*p, n)))
        .collect();
    let mut modifiers: Vec<String> = MODIFIER_PROPERTIES
        .iter()
        .filter_map(|p| key_name(dom, db, r, p))
        .collect();
    modifiers.sort();
    let ui_button =
        live_ref(dom, db, r, "UIButton").and_then(|t| dom.get(t).map(|i| (t, i.name().to_owned())));
    let ui_modifier = live_ref(dom, db, r, "UIModifier");
    let mut families = Vec::new();
    if !scriptable {
        families.extend(keys.iter().filter_map(|(_, n)| family_of_key(db, n)));
        if ui_button.is_some() {
            families.push(Family::Touch);
        }
        families.sort();
        families.dedup();
    }
    let vector2_scaled = match value_of(dom, db, r, "Vector2Scale") {
        Some(Variant::Vector2(v)) => v.x != 1.0 || v.y != 1.0,
        _ => false,
    };
    let vector3_scaled = match value_of(dom, db, r, "Vector3Scale") {
        Some(Variant::Vector3(v)) => v.x != 1.0 || v.y != 1.0 || v.z != 1.0,
        _ => false,
    };
    Binding {
        referent: r,
        name: name.to_owned(),
        scriptable,
        keys,
        modifiers,
        ui_button,
        ui_modifier,
        families,
        pressed_threshold: float_value(dom, db, r, "PressedThreshold"),
        released_threshold: float_value(dom, db, r, "ReleasedThreshold"),
        response_curve: float_value(dom, db, r, "ResponseCurve"),
        scale: float_value(dom, db, r, "Scale"),
        vector2_scaled,
        vector3_scaled,
    }
}

fn read_action(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> Action {
    let bindings = dom
        .get(r)
        .map(|i| i.children())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| dom.get(*c))
        .filter(|i| i.class() == BINDING_CLASS)
        .map(|i| read_binding(dom, db, i.referent(), i.name()))
        .collect();
    Action {
        referent: r,
        name: name.to_owned(),
        kind: ActionKind::from_value(enum_value(dom, db, r, "Type")),
        enabled: !matches!(
            value_of(dom, db, r, "Enabled"),
            Some(Variant::Bool(false)) | None
        ),
        bindings,
    }
}

fn walk(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, ctx: Option<usize>, out: &mut Model) {
    let Some(inst) = dom.get(r) else { return };
    let mut next = ctx;
    match inst.class() {
        CONTEXT_CLASS => {
            let priority = match value_of(dom, db, r, "Priority") {
                Some(Variant::Int32(p)) => *p,
                _ => 0,
            };
            out.contexts.push(Context {
                referent: r,
                name: inst.name().to_owned(),
                enabled: bool_value(dom, db, r, "Enabled"),
                priority,
                sink: bool_value(dom, db, r, "Sink"),
                actions: Vec::new(),
            });
            next = Some(out.contexts.len() - 1);
        }
        ACTION_CLASS => {
            let action = read_action(dom, db, r, inst.name());
            match ctx {
                Some(i) => out.contexts[i].actions.push(action),
                None => out.loose.push(action),
            }
        }
        _ => {}
    }
    for child in inst.children() {
        walk(dom, db, *child, next, out);
    }
}

/// Reads the whole place. Values come from
/// `db.stored_or_default(instance, property)`; a property with neither a
/// stored value nor a default reads as the type's zero (`false`, `0`, `None`
/// key, no reference).
pub(crate) fn model(dom: &WeakDom, db: &ReflectionDatabase) -> Model {
    let mut out = Model::default();
    for root in dom.root_refs() {
        walk(dom, db, *root, None, &mut out);
    }
    out
}

/// The value `property` of `referent` holds, or its reflection default.
pub(crate) fn value_of<'a>(
    dom: &'a WeakDom,
    db: &'a ReflectionDatabase,
    referent: Ref,
    property: &'a str,
) -> Option<&'a Variant> {
    let instance = dom.get(referent)?;
    db.stored_or_default(instance, property).map(|(_, v)| v)
}

#[cfg(test)]
mod round_trip;
#[cfg(test)]
mod tests;
