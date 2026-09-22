//! Changing an instance's class in place — the decidable half of the
//! Explorer's Change Class command (`shell::change_class` runs it).
//!
//! The instance keeps its referent (`WeakDom::set_class`), so everything
//! that names it — a `Weld.Part0`, a `Model.PrimaryPart`, the selection, an
//! open script tab — still does, with nothing to retarget. What it loses is
//! decided one property at a time by [`plan`]; what the picker offers is
//! [`choices`].

mod choices;

use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::explorer;

pub(crate) use choices::{choices, Choices};

/// Whether anything can be changed *into* `class`: a class the dump knows,
/// that `Instance.new` accepts and a class picker may show, and that is no
/// service — Roblox makes exactly one of each of those itself.
pub(crate) fn is_target(database: &ReflectionDatabase, class: &str) -> bool {
    database.class(class).is_some()
        && database.is_creatable(class)
        && database.is_browsable(class)
        && !database.is_service(class)
}

/// Whether `referent`'s own class may be changed. A service's may not —
/// scripts reach it through `GetService` by that very class — and neither
/// may a root the Explorer shows as one, which covers the few services the
/// dump does not know (see `explorer::is_known_service`).
pub(crate) fn changeable(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    dom.get(referent).is_some_and(|instance| {
        let class = instance.class();
        let service_root = dom.parent(referent).is_none() && explorer::is_known_service(class);
        !database.is_service(class) && !service_root
    })
}

/// Splits `selected` into what changing it to `target` converts and what it
/// refuses. An instance already of `target`, or gone, is in neither: there is
/// nothing to do for it and nothing worth saying.
pub(crate) fn partition(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
    target: &str,
) -> (Vec<Ref>, Vec<Ref>) {
    selected
        .iter()
        .copied()
        .filter(|&referent| dom.get(referent).is_some_and(|i| i.class() != target))
        .partition(|&referent| changeable(dom, database, referent))
}

/// What changing one instance to another class does to its properties.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Plan {
    /// DOM keys carried over as they are.
    pub(crate) kept: Vec<String>,
    /// DOM keys still at the old class's default, rewritten to the new
    /// class's own.
    pub(crate) reset: Vec<String>,
    /// Reflected names of what the new class cannot hold, which is what is
    /// actually lost and what the picker shows before anything happens.
    pub(crate) dropped: Vec<String>,
    /// DOM keys taken off the instance: the `dropped` ones.
    removed: Vec<String>,
    /// What the `reset` keys become, and what a fresh instance of the new
    /// class holds that this one never had.
    defaults: Vec<(String, Variant)>,
}

/// Decides, for every property `instance` holds, whether it survives the
/// change to `target`:
///
/// - **Unknown to the dump** (`Tags`, `AttributesSerialize`, anything else
///   only the file format knows): kept. Nothing here can say the new class
///   has no use for it, and a class change must never be how a tag or an
///   attribute goes missing.
/// - **Not on `target`**, or on it with another type (`BasePart.Size` is a
///   `Vector3`, `GuiObject.Size` a `UDim2`): dropped.
/// - **At the old class's default**: reset to the new class's, so a stock
///   `Part`'s 4 × 1.2 × 2 becomes a stock `TrussPart`'s 2 × 2 × 2 and a
///   stock `PointLight`'s range of 8 a `SpotLight`'s 16, rather than keeping
///   a value nobody chose (see [`restock`]).
/// - Anything else: kept.
///
/// Keys are the file's own spelling (`size`, `Color3uint8`), so each is read
/// through `ReflectionDatabase::canonical_name` — the same mapping scripts
/// and the Properties panel use — before it is compared with the dump. `fill` is what a fresh `target` is
/// given where the instance has no value at all, in that same spelling.
pub(crate) fn plan(
    database: &ReflectionDatabase,
    instance: &Instance,
    target: &str,
    fill: &[(&'static str, Variant)],
) -> Plan {
    let mut plan = Plan::default();
    for (key, value) in instance.properties() {
        let class = instance.class();
        let Some(property) = database.resolve_property(class, database.canonical_name(class, key))
        else {
            plan.kept.push(key.clone());
            continue;
        };
        let holds = database
            .resolve_property(target, &property.name)
            .is_some_and(|there| there.value_type == property.value_type);
        if !holds {
            plan.dropped.push(property.name.clone());
            plan.removed.push(key.clone());
        } else if let Some(fresh) =
            restock(database, instance.class(), target, &property.name, value)
        {
            plan.reset.push(key.clone());
            plan.defaults.push((key.clone(), fresh.clone()));
        } else {
            plan.kept.push(key.clone());
        }
    }
    let held = |key: &str| plan.kept.iter().chain(&plan.reset).any(|held| held == key);
    let filled: Vec<(String, Variant)> = fill
        .iter()
        .filter(|(key, _)| !held(key))
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    plan.defaults.extend(filled);
    plan
}

/// What a fresh `target` holds the property `name` at, when `value` is still
/// what a fresh `source` holds it at. `None` where either class records no
/// default, where the two agree, and where the new default is kept in
/// another type than the file stores the value in (the table records
/// `BasePart.Color` as a `Color3`, a file as a `Color3uint8`): the value is
/// then kept rather than rewritten in a type its key does not hold.
fn restock<'a>(
    database: &'a ReflectionDatabase,
    source: &str,
    target: &str,
    name: &'a str,
    value: &Variant,
) -> Option<&'a Variant> {
    let fresh = stock(database, target, name)?;
    (stock(database, source, name)? == value
        && fresh != value
        && std::mem::discriminant(fresh) == std::mem::discriminant(value))
    .then_some(fresh)
}

/// What a freshly created `class` holds the property `name` at, as Studio
/// reports it (`ReflectionDatabase::default_value`), under whichever of the
/// property's spellings the table records it by.
pub(crate) fn stock<'a>(
    database: &'a ReflectionDatabase,
    class: &str,
    name: &'a str,
) -> Option<&'a Variant> {
    database
        .stored_names(class, name)
        .into_iter()
        .find_map(|stored| database.default_value(class, stored))
}

/// Carries out `plan` on `referent`: the class first, then the properties.
pub(crate) fn apply(dom: &mut WeakDom, referent: Ref, target: &str, plan: &Plan) {
    if dom.set_class(referent, target).is_err() {
        return;
    }
    if let Some(instance) = dom.get_mut(referent) {
        // Untracked, and safely so: the `Change::Class` just logged already
        // has every consumer re-read this instance whole.
        for key in &plan.removed {
            instance.properties_mut().remove(key);
        }
    }
    for (key, value) in &plan.defaults {
        let _ = dom.set_property(referent, key, value.clone());
    }
}

/// How many names the footer spells out before it trails off.
const NAMED: usize = 3;

/// The picker's one-line account of what a change would cost, from the
/// [`plan`] of each instance it would convert — so a lossy conversion is
/// visible before it happens rather than after.
pub(crate) fn summary(plans: &[Plan]) -> String {
    let mut dropped: Vec<&str> = plans
        .iter()
        .flat_map(|plan| plan.dropped.iter().map(String::as_str))
        .collect();
    dropped.sort_unstable();
    dropped.dedup();
    let mut names = dropped[..dropped.len().min(NAMED)].join(", ");
    if dropped.len() > NAMED {
        names.push_str(", …");
    }
    let properties = |plan: &Plan| match plan.kept.len() + plan.reset.len() {
        1 => "1 property".to_owned(),
        count => format!("{count} properties"),
    };
    match (plans, dropped.len()) {
        ([], _) => String::new(),
        ([plan], 0) => format!("Keeps {} · drops none", properties(plan)),
        ([plan], count) => format!("Keeps {} · drops {count}: {names}", properties(plan)),
        (_, 0) => format!("Keeps every property of all {}", plans.len()),
        (_, count) => format!("Drops {count} across {}: {names}", plans.len()),
    }
}

#[cfg(test)]
#[path = "change_class/tests.rs"]
mod tests;
