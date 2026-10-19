//! Instance events replayed from the DOM's change log.
//!
//! Scripts write the DOM through plain property writes, so events are not
//! raised at the write: the host reads the changes a run made once it has
//! run and hands them to [`fire_changes`].

use mlua::{Lua, MultiValue, Result, Value};
use rbx_dom::{Change, Ref, WeakDom};

use super::engine;
use super::events::{game_signal, instance_key, read_attributes, ATTRIBUTES};
use super::signal;
use crate::ctx::Ctx;
use crate::plugin::api::Arg;

/// Drains `dom`'s change log, with the number [`fire_changes`] needs: the
/// first change's place in the DOM's mutation count.
pub(crate) fn take_changes(dom: &mut WeakDom) -> (Vec<Change>, Option<u64>) {
    let changes = dom.take_changes();
    let first = first_number(dom, changes.len());
    (changes, Some(first))
}

/// The mutation number of the first of the last `count` changes logged.
pub(crate) fn first_number(dom: &WeakDom, count: usize) -> u64 {
    dom.mutations() + 1 - count as u64
}

/// `start` and every ancestor above it, nearest first.
pub(crate) fn chain(dom: &WeakDom, start: Option<Ref>) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut at = start;
    while let Some(r) = at {
        if dom.get(r).is_none() {
            break;
        }
        out.push(r);
        at = dom.parent(r);
    }
    out
}

/// `root` and everything under it.
fn subtree(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        if let Some(instance) = dom.get(out[i]) {
            out.extend_from_slice(instance.children());
        }
        i += 1;
    }
    out
}

/// One change being fired, with what [`signal::fire_at`] filters
/// connections by.
struct Firing<'a> {
    lua: &'a Lua,
    change: Option<u64>,
}

impl Firing<'_> {
    fn emit(&self, referent: Ref, event: &str, args: &[Arg]) -> Result<()> {
        signal::fire_at(self.lua, &instance_key(referent, event), args, self.change)
    }

    fn parent(&self, referent: Ref, old: Option<Ref>, new: Option<Ref>) -> Result<()> {
        let (moved, old_chain, new_chain) = {
            let ctx = engine(self.lua).ctx.clone();
            let dom = ctx.dom();
            (subtree(&dom, referent), chain(&dom, old), chain(&dom, new))
        };
        let parent = new.map_or(Arg::Nil, Arg::Instance);
        for r in moved {
            self.emit(
                r,
                "AncestryChanged",
                &[Arg::Instance(referent), parent.clone()],
            )?;
        }
        let child = [Arg::Instance(referent)];
        if let Some(old) = old {
            self.emit(old, "ChildRemoved", &child)?;
        }
        if self.in_place(&old_chain) {
            let key = game_signal("DescendantRemoving").0;
            signal::fire_at(self.lua, &key, &child, self.change)?;
        }
        for r in old_chain {
            self.emit(r, "DescendantRemoving", &child)?;
        }
        if let Some(new) = new {
            self.emit(new, "ChildAdded", &child)?;
        }
        if self.in_place(&new_chain) {
            let key = game_signal("DescendantAdded").0;
            signal::fire_at(self.lua, &key, &child, self.change)?;
        }
        for r in new_chain {
            self.emit(r, "DescendantAdded", &child)?;
        }
        Ok(())
    }

    /// Whether an ancestor chain (nearest first) ends at one of the place's
    /// services rather than at the host's own parked instances.
    fn in_place(&self, chain: &[Ref]) -> bool {
        let Some(&top) = chain.last() else {
            return false;
        };
        let engine = engine(self.lua);
        let service = engine
            .ctx
            .dom()
            .get(top)
            .is_some_and(|i| engine.ctx.database().is_service(i.class()));
        service && !engine.host.is_parked(top)
    }

    fn property(&self, ctx: &Ctx, referent: Ref, raw: &str) -> Result<()> {
        // The log holds the spelling a file stores (`size`,
        // `Color3uint8`); a script hears `Size`, `Color`.
        let (name, value_object) = {
            let dom = ctx.dom();
            let instance = dom.get(referent);
            let name = instance.map_or_else(
                || raw.to_owned(),
                |i| ctx.database().canonical_name(i.class(), raw).to_owned(),
            );
            let value_object =
                instance.is_some_and(|i| ctx.database().is_subclass_of(i.class(), "ValueBase"));
            (name, value_object)
        };
        // A value object's `Changed` passes the new `Value` instead of a
        // property name, and fires for nothing else.
        if value_object {
            if name == "Value" {
                let value =
                    crate::property::get(self.lua, ctx, referent, "Value")?.unwrap_or(Value::Nil);
                signal::fire_values_at(
                    self.lua,
                    &instance_key(referent, "Changed"),
                    MultiValue::from_vec(vec![value]),
                    self.change,
                )?;
            }
        } else {
            self.emit(referent, "Changed", &[Arg::String(name.clone())])?;
        }
        self.emit(referent, &format!("Changed:{name}"), &[])?;
        if name == ATTRIBUTES || name == "Attributes" {
            for attribute in read_attributes(ctx, referent).into_keys() {
                self.emit(
                    referent,
                    "AttributeChanged",
                    &[Arg::String(attribute.clone())],
                )?;
                self.emit(referent, &format!("AttributeChanged:{attribute}"), &[])?;
            }
        }
        Ok(())
    }
}

/// Fires this VM's instance signals for a change log, reading the DOM as it
/// is now. `first` is the first change's number in the DOM's mutation
/// count (see [`take_changes`]), so each change reaches only connections
/// made before it, as in Roblox, where an event goes to whoever is
/// connected when it fires; `None` for changes made outside any script,
/// which every connection hears.
///
/// `Property` → `Changed(name)` and `Changed:<name>` (and
/// `AttributeChanged`/`AttributeChanged:<n>` for `Attributes`, comparing
/// nothing — every attribute name present fires); `Parent` → the child's
/// `AncestryChanged(child, parent)` (and its descendants'), the old
/// parent's `ChildRemoved`, the new one's `ChildAdded`, and
/// `DescendantRemoving`/`DescendantAdded` up both ancestor chains;
/// `Added` with a parent and no later `Parent` change → as a `Parent`
/// change from nil; `Removed` →
/// `Destroying` on the instance. Only connected signals cost anything.
pub(crate) fn fire_changes(lua: &Lua, changes: &[Change], first: Option<u64>) -> Result<()> {
    let ctx = engine(lua).ctx.clone();
    for (index, change) in changes.iter().enumerate() {
        let firing = Firing {
            lua,
            change: first.map(|first| first + index as u64),
        };
        match change {
            Change::Property { referent, name } => firing.property(&ctx, *referent, name)?,
            Change::Parent { referent, old, new } => firing.parent(*referent, *old, *new)?,
            Change::Added(referent) => {
                // `new_instance` with a parent logs the parenting too; that
                // change fires it, once.
                let reparented = changes[index..].iter().any(
                    |later| matches!(later, Change::Parent { referent: r, .. } if r == referent),
                );
                let parent = ctx.dom().parent(*referent);
                if parent.is_some() && !reparented {
                    firing.parent(*referent, None, parent)?;
                }
            }
            Change::Removed(referent) => firing.emit(*referent, "Destroying", &[])?,
            Change::Class(_) => {}
        }
    }
    Ok(())
}
