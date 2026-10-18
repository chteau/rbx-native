//! Applying the op log: last-writer-wins registers, the script-text log, and
//! the DOM writes themselves (through `WeakDom`'s own mutation methods, the
//! ones every editor action uses, so the change log and undo see them).

use std::collections::HashMap;

use rbx_dom::{Ref, Variant, WeakDom};
use serde_json::Value;

use super::ids::{parse_unique, IdMap};
use super::merge::{merge3, text_hash};
use super::{value, Batch, Clock, Op, Target};
use crate::paths;

/// What applying one batch did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    /// Ops that changed the DOM.
    pub applied: usize,
    /// Plain sentences for the user: a merge that kept both sides of a
    /// conflict, an op whose instance could not be found, ….
    pub notes: Vec<String>,
    /// Ops this editor must send again (its own `Src` rejected for a stale
    /// base, re-made against the current log text).
    pub resend: Vec<Op>,
}

/// One script's text as the log has settled it.
#[derive(Debug, Default, Clone)]
struct ScriptLog {
    text: String,
    /// Texts the log has held, by hash, so a stale base can still be merged
    /// from (bounded to the 64 most recent).
    seen: Vec<(String, String)>,
}

const SEEN_MAX: usize = 64;
const EXISTS: &str = "\u{0}exists";

impl ScriptLog {
    fn seeded(text: &str) -> ScriptLog {
        let mut log = ScriptLog {
            text: text.to_string(),
            seen: Vec::new(),
        };
        log.remember(text);
        log
    }

    fn remember(&mut self, text: &str) {
        let hash = text_hash(text);
        self.seen.retain(|(h, _)| *h != hash);
        self.seen.push((hash, text.to_string()));
        if self.seen.len() > SEEN_MAX {
            let excess = self.seen.len() - SEEN_MAX;
            self.seen.drain(..excess);
        }
    }

    fn lookup(&self, hash: &str) -> Option<&str> {
        self.seen
            .iter()
            .find(|(h, _)| h == hash)
            .map(|(_, t)| t.as_str())
    }
}

pub struct Engine {
    actor: String,
    counter: u64,
    regs: HashMap<(String, String), Clock>,
    scripts: HashMap<String, ScriptLog>,
}

/// Read-only resolution: by id (if the id's instance still exists), then by path.
fn lookup(dom: &WeakDom, ids: &IdMap, t: &Target) -> Option<Ref> {
    if !t.id.is_empty() {
        if let Some(r) = ids.get(&t.id).filter(|r| dom.get(*r).is_some()) {
            return Some(r);
        }
    }
    if t.p.is_empty() {
        return None;
    }
    paths::resolve(dom, &t.p)
}

/// [`lookup`], recording an id the map did not know yet for the instance found.
fn resolve(dom: &WeakDom, ids: &mut IdMap, t: &Target) -> Option<Ref> {
    let r = lookup(dom, ids, t)?;
    if !t.id.is_empty() && ids.get(&t.id).is_none() {
        ids.insert(r, t.id.clone());
    }
    Some(r)
}

fn key_id(t: &Target, ids: &IdMap, r: Option<Ref>) -> String {
    if !t.id.is_empty() {
        return t.id.clone();
    }
    r.and_then(|r| ids.known(r))
        .map_or_else(|| t.p.clone(), str::to_string)
}

fn would_cycle(dom: &WeakDom, child: Ref, parent: Option<Ref>) -> bool {
    let mut at = parent;
    while let Some(x) = at {
        if x == child {
            return true;
        }
        at = dom.parent(x);
    }
    false
}

fn source_of(dom: &WeakDom, r: Option<Ref>) -> String {
    match r.and_then(|r| dom.get(r)?.properties().get("Source").cloned()) {
        Some(Variant::String(s)) => s,
        _ => String::new(),
    }
}

fn is_unresolved_ref(v: &Value) -> bool {
    v.get("t").and_then(Value::as_str) == Some("Ref") && v.get("v").is_some_and(|x| !x.is_null())
}

/// A deferred `Ref` write: the instance, the property, the value.
type Pending = (Ref, String, Value);

struct Ctx<'a> {
    dom: &'a mut WeakDom,
    ids: &'a mut IdMap,
    report: Report,
    pending: Vec<Pending>,
}

impl Ctx<'_> {
    fn reparent(&mut self, r: Ref, parent: Option<Ref>) -> bool {
        if self.dom.parent(r) == parent {
            return false;
        }
        if would_cycle(self.dom, r, parent) {
            let path = paths::path_of(self.dom, r).unwrap_or_default();
            self.report
                .notes
                .push(format!("Refused to move {path} inside itself."));
            return false;
        }
        self.dom.set_parent(r, parent);
        true
    }

    /// Writes property `n` = `v` on `r`; `true` if the DOM changed. A value
    /// the DOM already holds is not written again, so an editor's own ops
    /// coming back from the log change nothing and log nothing.
    fn write(&mut self, r: Ref, n: &str, v: &Value) -> bool {
        match n {
            "Name" => match v.get("v").and_then(Value::as_str) {
                Some(name) if self.dom.get(r).is_some_and(|i| i.name() == name) => false,
                Some(name) => self.dom.set_name(r, name).is_ok(),
                None => false,
            },
            "Parent" => {
                let parent = match v.get("v") {
                    Some(t) if !t.is_null() => serde_json::from_value::<Target>(t.clone())
                        .ok()
                        .and_then(|t| lookup(self.dom, self.ids, &t)),
                    _ => None,
                };
                self.reparent(r, parent)
            }
            _ if v.is_null() => matches!(self.dom.remove_property(r, n), Ok(Some(_))),
            _ => {
                let (dom, ids) = (&*self.dom, &*self.ids);
                let decoded = value::decode(v, &|t| lookup(dom, ids, t));
                match decoded {
                    Some(val)
                        if self
                            .dom
                            .get(r)
                            .is_some_and(|i| i.properties().get(n) == Some(&val)) =>
                    {
                        false
                    }
                    Some(val) => self.dom.set_property(r, n, val).is_ok(),
                    None if is_unresolved_ref(v) => {
                        self.pending.push((r, n.to_string(), v.clone()));
                        false
                    }
                    None if v.get("t").and_then(Value::as_str) == Some("Ref") => {
                        matches!(self.dom.remove_property(r, n), Ok(Some(_)))
                    }
                    None => {
                        let path = paths::path_of(self.dom, r).unwrap_or_default();
                        self.report
                            .notes
                            .push(format!("Could not read the value of {path}.{n}."));
                        false
                    }
                }
            }
        }
    }

    fn not_found(&mut self, t: &Target) {
        let name = if t.p.is_empty() { &t.id } else { &t.p };
        self.report
            .notes
            .push(format!("Could not find {name} to apply an edit to it."));
    }
}

impl Engine {
    /// `actor` is `"<userId>:<client word>:<session>"` (see [`super::Batch`]).
    pub fn new(actor: String) -> Engine {
        Engine {
            actor,
            counter: 0,
            regs: HashMap::new(),
            scripts: HashMap::new(),
        }
    }
    pub fn actor(&self) -> &str {
        &self.actor
    }
    /// The next local clock: counter = max seen + 1.
    pub fn tick(&mut self) -> Clock {
        self.counter += 1;
        Clock(self.counter, self.actor.clone())
    }
    /// The hash of the log text for script `id`, seeding the log from
    /// `current` the first time the script is seen.
    pub fn log_text_hash(&mut self, id: &str, current: &str) -> String {
        let log = self
            .scripts
            .entry(id.to_string())
            .or_insert_with(|| ScriptLog::seeded(current));
        text_hash(&log.text)
    }

    /// Stamps the register and says whether `c` won it.
    fn claim(&mut self, id: &str, prop: &str, c: &Clock) -> bool {
        let key = (id.to_string(), prop.to_string());
        if matches!(self.regs.get(&key), Some(old) if old >= c) {
            return false;
        }
        self.regs.insert(key, c.clone());
        true
    }

    /// Applies one logged batch to `dom`, in op order. `Set`/`Add`/`Del`:
    /// applied only if the op's clock beats the register for (id, property)
    /// (`Add`/`Del` use the register (id, "\u{0}exists"); `Add` also stamps
    /// every property it carries); an `Add` whose instance already exists is
    /// an upsert; instances resolve by id, then by path; an `Add` with a
    /// UniqueId-shaped id writes it as the new instance's `UniqueId`; `Ref`
    /// values in the same batch resolve after its `Add`s. `Src`: accepted iff
    /// `base` is the log text's hash (then the log text moves); the DOM's
    /// `Source` follows: unchanged local text takes the new text, local text
    /// ahead of the log is merged three ways ([`super::merge::merge3`]) unless
    /// the op is this editor's own; a rejected own op is merged against the
    /// current log text and put in `resend`. Every clock seen raises `counter`.
    pub fn apply(&mut self, dom: &mut WeakDom, ids: &mut IdMap, batch: &Batch) -> Report {
        let mut cx = Ctx {
            dom,
            ids,
            report: Report::default(),
            pending: Vec::new(),
        };
        for op in &batch.ops {
            self.counter = self.counter.max(op.clock().0);
            let changed = match op {
                Op::Set { at, n, v, c } => self.set(&mut cx, at, n, v, c),
                Op::Add {
                    at,
                    cls,
                    parent,
                    props,
                    c,
                } => self.add(&mut cx, at, cls, parent.as_ref(), props, c),
                Op::Del { at, c } => self.del(&mut cx, at, c),
                Op::Src { at, base, text, c } => self.src(&mut cx, at, base, text, c),
            };
            if changed {
                cx.report.applied += 1;
            }
        }
        for (r, n, v) in std::mem::take(&mut cx.pending) {
            let (dom, ids) = (&*cx.dom, &*cx.ids);
            let decoded = value::decode(&v, &|t| lookup(dom, ids, t));
            match decoded {
                Some(val) => {
                    let _ = cx.dom.set_property(r, &n, val);
                }
                None => {
                    let _ = cx.dom.remove_property(r, &n);
                }
            }
        }
        cx.report
    }

    fn set(&mut self, cx: &mut Ctx, at: &Target, n: &str, v: &Value, c: &Clock) -> bool {
        let Some(r) = resolve(cx.dom, cx.ids, at) else {
            cx.not_found(at);
            return false;
        };
        let id = key_id(at, cx.ids, Some(r));
        if n != "Source" && !self.claim(&id, n, c) {
            return false;
        }
        cx.write(r, n, v)
    }

    fn add(
        &mut self,
        cx: &mut Ctx,
        at: &Target,
        cls: &str,
        parent: Option<&Target>,
        props: &serde_json::Map<String, Value>,
        c: &Clock,
    ) -> bool {
        let existing = if at.id.is_empty() {
            None
        } else {
            cx.ids.get(&at.id).filter(|r| cx.dom.get(*r).is_some())
        };
        let id = key_id(at, cx.ids, existing);
        if !self.claim(&id, EXISTS, c) {
            return false;
        }
        let parent_ref = match parent {
            Some(t) => match resolve(cx.dom, cx.ids, t) {
                Some(p) => Some(p),
                None => {
                    cx.not_found(t);
                    return false;
                }
            },
            None => None,
        };
        let r = match existing {
            Some(r) => {
                cx.reparent(r, parent_ref);
                r
            }
            None => {
                let name = props
                    .get("Name")
                    .and_then(|v| v.get("v"))
                    .and_then(Value::as_str)
                    .unwrap_or("Instance");
                let r = cx.dom.new_instance(cls, name, parent_ref);
                if !at.id.is_empty() {
                    cx.ids.insert(r, at.id.clone());
                }
                if let Some(u) = parse_unique(&at.id) {
                    let _ = cx.dom.set_property(r, "UniqueId", Variant::UniqueId(u));
                }
                r
            }
        };
        for (n, v) in props {
            if n == "Parent" || (existing.is_none() && n == "Name") {
                continue;
            }
            if n == "Source" {
                if let Some(Value::String(s)) = v.get("v") {
                    self.scripts
                        .entry(id.clone())
                        .or_insert_with(|| ScriptLog::seeded(s));
                }
            } else if !self.claim(&id, n, c) {
                continue;
            }
            cx.write(r, n, v);
        }
        true
    }

    fn del(&mut self, cx: &mut Ctx, at: &Target, c: &Clock) -> bool {
        let r = resolve(cx.dom, cx.ids, at);
        let id = key_id(at, cx.ids, r);
        if !self.claim(&id, EXISTS, c) {
            return false;
        }
        match r {
            Some(r) => !cx.dom.remove(r).is_empty(),
            None => false,
        }
    }

    fn src(&mut self, cx: &mut Ctx, at: &Target, base: &str, text: &str, c: &Clock) -> bool {
        let r = resolve(cx.dom, cx.ids, at);
        let id = key_id(at, cx.ids, r);
        let current = source_of(cx.dom, r);
        let own = c.1 == self.actor;
        let log = self
            .scripts
            .entry(id)
            .or_insert_with(|| ScriptLog::seeded(&current));
        let path = r
            .and_then(|r| paths::path_of(cx.dom, r))
            .unwrap_or_else(|| at.p.clone());

        if base == text_hash(&log.text) {
            let old = std::mem::replace(&mut log.text, text.to_string());
            log.remember(text);
            let Some(r) = r else { return false };
            if own || current == text {
                return false;
            }
            let next = if current == old {
                text.to_string()
            } else {
                let m = merge3(&old, &current, text);
                if m.conflicted {
                    cx.report.notes.push(format!(
                        "{path}: your unsent edits and someone else's overlapped; both were kept between conflict markers."
                    ));
                }
                m.text
            };
            return cx
                .dom
                .set_property(r, "Source", Variant::String(next))
                .is_ok();
        }

        if !own || text == log.text {
            return false;
        }
        let base_text = log.lookup(base).unwrap_or(&log.text).to_string();
        let log_text = log.text.clone();
        let m = merge3(&base_text, text, &log_text);
        if m.conflicted {
            cx.report.notes.push(format!(
                "{path}: your edit and someone else's overlapped; both were kept between conflict markers."
            ));
        }
        let clock = self.tick();
        cx.report.resend.push(Op::Src {
            at: at.clone(),
            base: text_hash(&log_text),
            text: m.text.clone(),
            c: clock,
        });
        match r {
            Some(r) if current != m.text => cx
                .dom
                .set_property(r, "Source", Variant::String(m.text))
                .is_ok(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
