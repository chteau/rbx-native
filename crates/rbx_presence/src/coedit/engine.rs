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
mod tests {
    use super::*;
    use serde_json::json;

    fn t(id: &str, p: &str) -> Target {
        Target {
            id: id.into(),
            p: p.into(),
        }
    }

    fn s(v: &str) -> Value {
        json!({"t": "String", "v": v})
    }

    fn world() -> (WeakDom, IdMap, Ref, Ref) {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(ws));
        let script = dom.new_instance("Script", "S", Some(ws));
        dom.set_property(script, "Source", Variant::String("a\nb\nc\n".into()))
            .unwrap();
        dom.take_changes();
        let ids = IdMap::build(&dom);
        (dom, ids, part, script)
    }

    fn id(ids: &IdMap, r: Ref) -> String {
        ids.known(r).unwrap().to_string()
    }

    fn batch(a: &str, ops: Vec<Op>) -> Batch {
        Batch { a: a.into(), ops }
    }

    fn prop(dom: &WeakDom, r: Ref, n: &str) -> Option<Variant> {
        dom.get(r)?.properties().get(n).cloned()
    }

    fn source(dom: &WeakDom, r: Ref) -> String {
        source_of(dom, Some(r))
    }

    fn set(at: Target, n: &str, v: Value, c: Clock) -> Op {
        Op::Set {
            at,
            n: n.into(),
            v,
            c,
        }
    }

    #[test]
    fn crossed_clocks_converge_and_older_skips() {
        let (dom0, ids0, part, _) = world();
        let pid = id(&ids0, part);
        let a = batch(
            "1:studio",
            vec![set(t(&pid, ""), "Tag", s("a"), Clock(5, "1:studio".into()))],
        );
        let b = batch(
            "2:studio",
            vec![set(t(&pid, ""), "Tag", s("b"), Clock(5, "2:studio".into()))],
        );
        let mut results = Vec::new();
        for order in [[&a, &b], [&b, &a]] {
            let (mut dom, mut ids) = (dom0.clone(), ids0.clone());
            let mut e = Engine::new("9:studio".into());
            for x in order {
                e.apply(&mut dom, &mut ids, x);
            }
            results.push(prop(&dom, part, "Tag"));
        }
        assert_eq!(results[0], results[1]);
        assert_eq!(results[0], Some(Variant::String("b".into())));

        let (mut dom, mut ids) = (dom0.clone(), ids0.clone());
        let mut e = Engine::new("9:studio".into());
        e.apply(&mut dom, &mut ids, &b);
        let old = batch(
            "1:studio",
            vec![set(
                t(&pid, ""),
                "Tag",
                s("old"),
                Clock(3, "1:studio".into()),
            )],
        );
        assert_eq!(e.apply(&mut dom, &mut ids, &old).applied, 0);
        assert_eq!(prop(&dom, part, "Tag"), Some(Variant::String("b".into())));
    }

    fn add(idv: &str, parent: Option<Target>, props: Value, c: u64) -> Op {
        Op::Add {
            at: t(idv, ""),
            cls: "Part".into(),
            parent,
            props: props.as_object().unwrap().clone(),
            c: Clock(c, "1:studio".into()),
        }
    }

    #[test]
    fn add_set_upsert_del() {
        let (mut dom, mut ids, part, _) = world();
        let ws = dom.parent(part).unwrap();
        let wid = id(&ids, ws);
        let nid = "0000000100000002000000000000000f".to_string();
        let mut e = Engine::new("9:studio".into());
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![
                    add(
                        &nid,
                        Some(t(&wid, "")),
                        json!({"Name": s("New"), "Tag": s("x")}),
                        1,
                    ),
                    set(t(&nid, ""), "Tag", s("y"), Clock(2, "1:studio".into())),
                ],
            ),
        );
        assert_eq!(r.applied, 2);
        let n = ids.get(&nid).unwrap();
        assert_eq!(dom.get(n).unwrap().name(), "New");
        assert_eq!(dom.parent(n), Some(ws));
        assert_eq!(prop(&dom, n, "Tag"), Some(Variant::String("y".into())));
        assert!(matches!(
            prop(&dom, n, "UniqueId"),
            Some(Variant::UniqueId(_))
        ));

        e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![add(&nid, None, json!({"Name": s("Renamed")}), 3)],
            ),
        );
        assert_eq!(ids.get(&nid), Some(n));
        assert_eq!(dom.get(n).unwrap().name(), "Renamed");
        assert_eq!(dom.parent(n), None);

        e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![Op::Del {
                    at: t(&nid, ""),
                    c: Clock(4, "1:studio".into()),
                }],
            ),
        );
        assert!(dom.get(n).is_none());
        let late = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "2:studio",
                vec![set(
                    t(&nid, "Renamed"),
                    "Tag",
                    s("z"),
                    Clock(9, "2:studio".into()),
                )],
            ),
        );
        assert_eq!(late.applied, 0);
        assert!(dom.get(n).is_none());
    }

    #[test]
    fn ref_resolves_to_add_later_in_batch_and_path_fallback() {
        let (mut dom, mut ids, part, _) = world();
        let pid = id(&ids, part);
        let mut e = Engine::new("9:studio".into());
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![
                    set(
                        t(&pid, ""),
                        "Link",
                        json!({"t": "Ref", "v": {"id": "fresh", "p": ""}}),
                        Clock(1, "1:studio".into()),
                    ),
                    add("fresh", None, json!({"Name": s("F")}), 2),
                    set(
                        t("", "Workspace.Part"),
                        "Tag",
                        s("p"),
                        Clock(3, "1:studio".into()),
                    ),
                ],
            ),
        );
        let fresh = ids.get("fresh").unwrap();
        assert_eq!(prop(&dom, part, "Link"), Some(Variant::Ref(fresh)));
        assert_eq!(prop(&dom, part, "Tag"), Some(Variant::String("p".into())));
        assert_eq!(r.applied, 2);
    }

    #[test]
    fn parent_cycle_refused() {
        let (mut dom, mut ids, part, _) = world();
        let ws = dom.parent(part).unwrap();
        let (wid, pid) = (id(&ids, ws), id(&ids, part));
        let mut e = Engine::new("9:studio".into());
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![set(
                    t(&wid, ""),
                    "Parent",
                    json!({"t": "Ref", "v": {"id": pid, "p": ""}}),
                    Clock(1, "1:studio".into()),
                )],
            ),
        );
        assert_eq!(r.applied, 0);
        assert_eq!(r.notes.len(), 1);
        assert_eq!(dom.parent(ws), None);
    }

    fn src(sid: &str, base: &str, text: &str, c: Clock) -> Op {
        Op::Src {
            at: t(sid, "Workspace.S"),
            base: base.into(),
            text: text.into(),
            c,
        }
    }

    #[test]
    fn src_fast_forward_and_stale_rejected() {
        let (mut dom, mut ids, _, script) = world();
        let sid = id(&ids, script);
        let mut e = Engine::new("9:studio".into());
        let h0 = e.log_text_hash(&sid, "a\nb\nc\n");
        e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![src(&sid, &h0, "a\nB\nc\n", Clock(1, "1:studio".into()))],
            ),
        );
        assert_eq!(source(&dom, script), "a\nB\nc\n");
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "2:studio",
                vec![src(&sid, &h0, "X\nb\nc\n", Clock(2, "2:studio".into()))],
            ),
        );
        assert_eq!(r.applied, 0);
        assert!(r.resend.is_empty());
        assert_eq!(source(&dom, script), "a\nB\nc\n");
        assert_eq!(e.log_text_hash(&sid, ""), text_hash("a\nB\nc\n"));
    }

    #[test]
    fn own_rejected_src_resends_merged() {
        let (mut dom, mut ids, _, script) = world();
        let sid = id(&ids, script);
        let mut e = Engine::new("1:studio".into());
        let h0 = e.log_text_hash(&sid, "a\nb\nc\n");
        e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "2:studio",
                vec![src(&sid, &h0, "A\nb\nc\n", Clock(4, "2:studio".into()))],
            ),
        );
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "1:studio",
                vec![src(&sid, &h0, "a\nb\nC\n", Clock(1, "1:studio".into()))],
            ),
        );
        assert_eq!(r.resend.len(), 1);
        match &r.resend[0] {
            Op::Src { base, text, c, .. } => {
                assert_eq!(base, &text_hash("A\nb\nc\n"));
                assert_eq!(text, "A\nb\nC\n");
                assert!(c.0 > 4);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(source(&dom, script), "A\nb\nC\n");
    }

    #[test]
    fn local_unsent_text_merges_with_remote() {
        let (mut dom, mut ids, _, script) = world();
        let sid = id(&ids, script);
        let mut e = Engine::new("1:studio".into());
        let h0 = e.log_text_hash(&sid, "a\nb\nc\n");
        dom.set_property(script, "Source", Variant::String("a\nb\nC\n".into()))
            .unwrap();
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "2:studio",
                vec![src(&sid, &h0, "A\nb\nc\n", Clock(1, "2:studio".into()))],
            ),
        );
        assert!(r.notes.is_empty());
        assert_eq!(source(&dom, script), "A\nb\nC\n");

        let h1 = e.log_text_hash(&sid, "");
        dom.set_property(script, "Source", Variant::String("A\nmine\nC\n".into()))
            .unwrap();
        let r = e.apply(
            &mut dom,
            &mut ids,
            &batch(
                "2:studio",
                vec![src(
                    &sid,
                    &h1,
                    "A\ntheirs\nc\n",
                    Clock(2, "2:studio".into()),
                )],
            ),
        );
        assert_eq!(r.notes.len(), 1);
        assert!(r.notes[0].contains("Workspace.S"));
        assert!(source(&dom, script).contains("<<<<<<<"));
    }

    #[test]
    fn replay_is_idempotent_and_tick_exceeds() {
        let (mut dom, mut ids, part, _) = world();
        let pid = id(&ids, part);
        let log = batch(
            "1:studio",
            vec![
                add("n1", None, json!({"Name": s("N")}), 7),
                set(t(&pid, ""), "Tag", s("v"), Clock(8, "1:studio".into())),
                Op::Del {
                    at: t("n1", ""),
                    c: Clock(41, "1:studio".into()),
                },
            ],
        );
        let mut e = Engine::new("9:studio".into());
        e.apply(&mut dom, &mut ids, &log);
        let once = (dom.root_refs().to_vec(), prop(&dom, part, "Tag"));
        let r = e.apply(&mut dom, &mut ids, &log);
        assert_eq!(r.applied, 0);
        assert_eq!((dom.root_refs().to_vec(), prop(&dom, part, "Tag")), once);
        assert!(e.tick().0 > 41);
    }
}
