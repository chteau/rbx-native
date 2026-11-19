//! Which `Script`s a server runs.
//!
//! The `Script` docs: a script runs once it is `Enabled` and a descendant
//! of `Workspace` or `ServerScriptService`. With `RunContext.Server` it
//! also runs in `ReplicatedStorage` (creator-docs `scripting/locations`).
//! `LocalScript` is a subclass of `Script` but never runs on a server, and
//! neither does `RunContext.Client` or `Plugin`.

use rbx_dom::{Ref, Variant, WeakDom};

const LEGACY: u32 = 0;
const SERVER: u32 = 1;

fn enabled(dom: &WeakDom, script: Ref) -> bool {
    let Some(instance) = dom.get(script) else {
        return false;
    };
    let props = instance.properties();
    !matches!(props.get("Disabled"), Some(Variant::Bool(true)))
        && !matches!(props.get("Enabled"), Some(Variant::Bool(false)))
}

/// Whether `script` would run on a server where it stands now.
pub(crate) fn runs(dom: &WeakDom, script: Ref) -> bool {
    let Some(instance) = dom.get(script) else {
        return false;
    };
    if instance.class() != "Script" || !enabled(dom, script) {
        return false;
    }
    let context = match instance.properties().get("RunContext") {
        Some(Variant::Enum(value)) => *value,
        _ => LEGACY,
    };
    let containers: &[&str] = match context {
        LEGACY => &["Workspace", "ServerScriptService"],
        SERVER => &["Workspace", "ServerScriptService", "ReplicatedStorage"],
        _ => return false,
    };
    let mut at = dom.parent(script);
    while let Some(r) = at {
        let Some(ancestor) = dom.get(r) else {
            return false;
        };
        // The service itself, which is a root of the place's DOM.
        if dom.parent(r).is_none() {
            return containers.contains(&ancestor.class());
        }
        at = dom.parent(r);
    }
    false
}

/// `root` and its descendants, depth first in child order: the order a
/// server starts the scripts it loads.
pub(crate) fn subtree(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        if let Some(instance) = dom.get(at) {
            out.push(at);
            stack.extend(instance.children().iter().rev());
        }
    }
    out
}
