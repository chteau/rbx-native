//! Instance events replayed from the DOM's change log.
//!
//! Scripts write the DOM through plain property writes, so events are not
//! raised at the write: the host reads the changes a run made once it has
//! run and hands them to [`fire_changes`].

use std::collections::HashMap;

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
    /// This change's place in its batch, and where each instance's last
    /// parenting in the batch is: a descendant parented after this change
    /// wasn't under the instance yet, so its own change fires it.
    index: usize,
    last_parented: &'a HashMap<Ref, usize>,
}

impl Firing<'_> {
    fn emit(&self, referent: Ref, event: &str, args: &[Arg]) -> Result<()> {
        signal::fire_at(self.lua, &instance_key(referent, event), args, self.change)
    }

    /// One reparenting, in a live server's order (rbx_sim's ReparentOrder
    /// golden): the old ancestors' `DescendantRemoving` for the instance
    /// and each of its descendants, the old parent's `ChildRemoved`; the
    /// new ancestors' `DescendantAdded` the same way, the new parent's
    /// `ChildAdded`; then `AncestryChanged`, deepest first, and last the
    /// instance's `Changed("Parent")` and `Changed:Parent`.
    fn parent(&self, referent: Ref, old: Option<Ref>, new: Option<Ref>) -> Result<()> {
        let (moved, old_chain, new_chain) = {
            let ctx = engine(self.lua).ctx.clone();
            let dom = ctx.dom();
            let mut moved = subtree(&dom, referent);
            moved.retain(|r| {
                *r == referent || self.last_parented.get(r).is_none_or(|i| *i <= self.index)
            });
            (moved, chain(&dom, old), chain(&dom, new))
        };
        let child = [Arg::Instance(referent)];
        let up = |chain: &[Ref], name: &str, parent: Option<Ref>, last: &str| -> Result<()> {
            let in_place = self.in_place(chain);
            for &r in &moved {
                let it = [Arg::Instance(r)];
                if in_place {
                    let key = game_signal(name).0;
                    signal::fire_at(self.lua, &key, &it, self.change)?;
                }
                for &a in chain {
                    self.emit(a, name, &it)?;
                }
            }
            if let Some(parent) = parent {
                self.emit(parent, last, &child)?;
            }
            Ok(())
        };
        up(&old_chain, "DescendantRemoving", old, "ChildRemoved")?;
        up(&new_chain, "DescendantAdded", new, "ChildAdded")?;
        super::collection::changed(self.lua, &[referent])?;
        self.host_signals(referent, old, new, false)?;
        let parent = new.map_or(Arg::Nil, Arg::Instance);
        for &r in moved.iter().rev() {
            self.emit(
                r,
                "AncestryChanged",
                &[Arg::Instance(referent), parent.clone()],
            )?;
        }
        // A value object's `Changed` fires for its `Value` only.
        let value_object = {
            let ctx = engine(self.lua).ctx.clone();
            let dom = ctx.dom();
            dom.get(referent)
                .is_some_and(|i| ctx.database().is_subclass_of(i.class(), "ValueBase"))
        };
        if !value_object {
            self.emit(referent, "Changed", &[Arg::String("Parent".into())])?;
        }
        self.emit(referent, "Changed:Parent", &[])?;
        self.host_signals(referent, old, new, true)
    }

    fn host_signals(
        &self,
        child: Ref,
        old: Option<Ref>,
        new: Option<Ref>,
        after: bool,
    ) -> Result<()> {
        let fire = engine(self.lua).host.reparented(child, old, new, after);
        for (owner, name, args) in fire {
            signal::fire(self.lua, &instance_key(owner, name.as_str()), &args)?;
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
        if raw == "Tags" {
            super::collection::changed(self.lua, &[referent])?;
        }
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
/// nothing, every attribute name present fires); `Parent` → see
/// `Firing::parent`;
/// `Added` with a parent and no later `Parent` change → as a `Parent`
/// change from nil; `Removed` →
/// `Destroying` on the instance. Only connected signals cost anything.
pub(crate) fn fire_changes(lua: &Lua, changes: &[Change], first: Option<u64>) -> Result<()> {
    let engine = engine(lua);
    let ctx = engine.ctx.clone();
    let mut last_parented = HashMap::new();
    for (index, change) in changes.iter().enumerate() {
        if let Change::Parent { referent, .. } | Change::Added(referent) = change {
            last_parented.insert(*referent, index);
        }
    }
    for (index, change) in changes.iter().enumerate() {
        let firing = Firing {
            lua,
            change: first.map(|first| first + index as u64),
            index,
            last_parented: &last_parented,
        };
        match change {
            Change::Property { referent, name } => firing.property(&ctx, *referent, name)?,
            // Gone by now (an undo took back an insert): its `Destroying`
            // comes from `Removed`, and a `ChildAdded` handler reading the
            // child's `Name` would only hit a destroyed instance.
            Change::Parent { referent, .. } if ctx.dom().get(*referent).is_none() => {}
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
            Change::Removed(referent) => {
                if !engine.destroyed.borrow_mut().remove(referent) {
                    firing.emit(*referent, "Destroying", &[])?;
                }
            }
            Change::Class(_) => {}
        }
    }
    Ok(())
}

/// `Instance:Destroy`, in a live server's order (the ReparentOrder golden):
/// `root`'s `Destroying`, then its unparenting with that reparenting's
/// events (fired there and then), then each child destroyed the same way,
/// so a child's `Destroying` still finds the child under `root`. Fusion's
/// cleanup (Lemonade's widget) unparents each child from its `Destroying`;
/// a child that left `root` that way is spared. The `Removed` these leave
/// in the change log fire nothing more in this VM. A VM with no engine
/// (the command bar's) has no listeners to tell. Then, as Roblox's
/// `Destroy` does, every connection to an instance it removed is
/// disconnected.
pub(crate) fn destroy(lua: &Lua, ctx: &Ctx, root: Ref) -> Result<()> {
    let mut gone = Vec::new();
    let destroyed = destroy_tree(lua, ctx, root, &mut gone);
    if let Some(engine) = lua.app_data_ref::<std::rc::Rc<super::Engine>>() {
        if !gone.is_empty() {
            let gone: std::collections::HashSet<Ref> = gone.into_iter().collect();
            engine
                .signals
                .borrow_mut()
                .connections
                .retain(|key, _| match key.owner {
                    signal::Owner::Instance(owner) => !gone.contains(&owner),
                    _ => true,
                });
        }
    }
    destroyed
}

fn destroy_tree(lua: &Lua, ctx: &Ctx, root: Ref, gone: &mut Vec<Ref>) -> Result<()> {
    if ctx.dom().get(root).is_none() {
        return Ok(());
    }
    {
        let mut dom = ctx.dom_mut();
        // A model losing its `PrimaryPart` keeps the pivot it had.
        crate::pivot::keep_pivots(&mut dom, ctx.database(), &[root])
            .map_err(mlua::Error::runtime)?;
        ctx.bury(&dom, root);
    }
    let engine = lua
        .app_data_ref::<std::rc::Rc<super::Engine>>()
        .map(|e| e.clone());
    if let Some(engine) = engine {
        engine.destroyed.borrow_mut().insert(root);
        signal::fire(lua, &instance_key(root, "Destroying"), &[])?;
        if ctx.dom().get(root).is_none() {
            return Ok(());
        }
        if ctx.dom().parent(root).is_some() {
            ctx.dom_mut().set_parent(root, None);
            crate::hooks::wrote(lua)?;
        }
        let children = ctx
            .dom()
            .get(root)
            .map(|i| i.children().to_vec())
            .unwrap_or_default();
        for child in children {
            if ctx.dom().parent(child) == Some(root) {
                destroy_tree(lua, ctx, child, gone)?;
            }
        }
    }
    ctx.dom_mut().remove(root);
    gone.push(root);
    Ok(())
}
