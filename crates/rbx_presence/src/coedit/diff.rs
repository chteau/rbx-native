//! What changed locally: the ops that take `shadow` (the DOM as last synced)
//! to `current`, found by comparing the two by referent. Every way the
//! editor mutates the DOM, undo and redo included, shows up here, so nothing
//! has to hook each mutation site.

use std::collections::HashSet;

use rbx_dom::{Ref, Variant, WeakDom};
use serde_json::{json, Map, Value};

use super::engine::Engine;
use super::ids::IdMap;
use super::value;
use super::{Op, Target};
use crate::paths;

const SCRIPT_CLASSES: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

fn target_of(dom: &mut WeakDom, ids: &mut IdMap, r: Ref) -> Option<Target> {
    let id = ids.id_of(dom, r)?;
    let p = paths::path_of(dom, r)?;
    Some(Target { id, p })
}

fn encode_prop(current: &mut WeakDom, ids: &mut IdMap, v: &Variant) -> Option<Value> {
    if let Variant::Ref(t) = v {
        target_of(current, ids, *t)?;
    }
    let (dom, ids) = (&*current, &*ids);
    value::encode(v, &|t| {
        Some(Target {
            id: ids.known(t)?.to_string(),
            p: paths::path_of(dom, t)?,
        })
    })
}

fn string_value(s: &str) -> Value {
    json!({"t": "String", "v": s})
}

fn parent_value(current: &mut WeakDom, ids: &mut IdMap, r: Ref) -> Option<Value> {
    match current.parent(r) {
        Some(p) => {
            let t = target_of(current, ids, p)?;
            Some(json!({"t": "Ref", "v": t}))
        }
        None => Some(json!({"t": "Ref", "v": Value::Null})),
    }
}

/// Shadow referents to delete (top-most only) and every shadow referent
/// that a delete takes with it.
fn collect_deletes(
    shadow: &WeakDom,
    current: &WeakDom,
    r: Ref,
    under_deleted: bool,
    deleted: &mut HashSet<Ref>,
    dels: &mut Vec<Ref>,
) {
    let Some(old) = shadow.get(r) else { return };
    let mut inside = under_deleted;
    if under_deleted {
        deleted.insert(r);
    } else if current.get(r).is_none_or(|c| c.class() != old.class()) {
        deleted.insert(r);
        dels.push(r);
        inside = true;
    }
    for &child in old.children() {
        collect_deletes(shadow, current, child, inside, deleted, dels);
    }
}

fn tree_order(dom: &WeakDom, refs: &[Ref], out: &mut Vec<Ref>) {
    for &r in refs {
        out.push(r);
        if let Some(i) = dom.get(r) {
            tree_order(dom, i.children(), out);
        }
    }
}

/// Ops for every difference, parents before children:
/// - removed referents → `Del` (only the top-most of a removed subtree;
///   id from `ids.known`, path from `shadow`);
/// - added referents → `Add` with all encodable properties plus `Name`
///   (`Source` included as a String);
/// - class changed → `Del` then `Add`;
/// - name / parent changed → `Set` `Name` / `Parent`;
/// - a property changed or added → `Set` (removed → `Set` with `null`),
///   except `Source` on a script, which becomes `Src` against
///   `engine.log_text_hash(id)`.
///
/// Unsendable types ([`super::value`]) are skipped. Clocks come from
/// `engine.tick()`. Paths are `crate::paths::path_of` in `current` (in
/// `shadow` for `Del`).
pub fn local_ops(
    shadow: &WeakDom,
    current: &mut WeakDom,
    ids: &mut IdMap,
    engine: &mut Engine,
) -> Vec<Op> {
    let mut deleted = HashSet::new();
    let mut dels = Vec::new();
    for &r in shadow.root_refs() {
        collect_deletes(shadow, current, r, false, &mut deleted, &mut dels);
    }
    let mut ops = Vec::new();
    for r in dels {
        let id = ids.known(r).unwrap_or_default().to_string();
        let p = paths::path_of(shadow, r).unwrap_or_default();
        ops.push(Op::Del {
            at: Target { id, p },
            c: engine.tick(),
        });
    }

    let mut order = Vec::new();
    tree_order(current, current.root_refs(), &mut order);
    for r in order {
        let Some(at) = target_of(current, ids, r) else {
            continue;
        };
        let old = shadow.get(r).filter(|_| !deleted.contains(&r));
        let Some(inst) = current.get(r) else { continue };
        let class = inst.class().to_string();
        let name = inst.name().to_string();
        let props: Vec<(String, Variant)> = inst
            .properties()
            .iter()
            .filter(|(k, _)| k.as_str() != "UniqueId")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let Some(old) = old else {
            let parent = match current.parent(r) {
                Some(p) => target_of(current, ids, p),
                None => None,
            };
            let mut map = Map::new();
            map.insert("Name".to_string(), string_value(&name));
            for (k, v) in &props {
                if let Some(e) = encode_prop(current, ids, v) {
                    map.insert(k.clone(), e);
                }
            }
            ops.push(Op::Add {
                at,
                cls: class,
                parent,
                props: map,
                c: engine.tick(),
            });
            continue;
        };

        if name != old.name() {
            ops.push(Op::Set {
                at: at.clone(),
                n: "Name".to_string(),
                v: string_value(&name),
                c: engine.tick(),
            });
        }
        if current.parent(r) != shadow.parent(r) {
            if let Some(v) = parent_value(current, ids, r) {
                ops.push(Op::Set {
                    at: at.clone(),
                    n: "Parent".to_string(),
                    v,
                    c: engine.tick(),
                });
            }
        }
        let is_script = SCRIPT_CLASSES.contains(&class.as_str());
        for (k, v) in &props {
            if old.properties().get(k) == Some(v) {
                continue;
            }
            if is_script && k == "Source" {
                if let Variant::String(text) = v {
                    let before = match old.properties().get("Source") {
                        Some(Variant::String(s)) => s.as_str(),
                        _ => "",
                    };
                    let base = engine.log_text_hash(&at.id, before);
                    ops.push(Op::Src {
                        at: at.clone(),
                        base,
                        text: text.clone(),
                        c: engine.tick(),
                    });
                    continue;
                }
            }
            if let Some(e) = encode_prop(current, ids, v) {
                ops.push(Op::Set {
                    at: at.clone(),
                    n: k.clone(),
                    v: e,
                    c: engine.tick(),
                });
            }
        }
        for (k, v) in old.properties() {
            if k == "UniqueId" || props.iter().any(|(n, _)| n == k) {
                continue;
            }
            let sendable = value::encode(v, &|_| {
                Some(Target {
                    id: String::new(),
                    p: String::new(),
                })
            })
            .is_some();
            if sendable {
                ops.push(Op::Set {
                    at: at.clone(),
                    n: k.clone(),
                    v: Value::Null,
                    c: engine.tick(),
                });
            }
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coedit::Batch;
    use rbx_dom::Vector3Data;

    struct Fixture {
        dom: WeakDom,
        ws: Ref,
        part: Ref,
        script: Ref,
    }

    fn fixture() -> Fixture {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "P", Some(ws));
        dom.set_property(part, "Anchored", Variant::Bool(true))
            .unwrap();
        dom.set_property(
            part,
            "Size",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
        )
        .unwrap();
        let script = dom.new_instance("Script", "S", Some(ws));
        dom.set_property(script, "Source", Variant::String("a\n".into()))
            .unwrap();
        dom.take_changes();
        Fixture {
            dom,
            ws,
            part,
            script,
        }
    }

    fn diff(shadow: &WeakDom, current: &mut WeakDom) -> (Vec<Op>, IdMap) {
        let mut ids = IdMap::build(shadow);
        let mut eng = Engine::new("1:studio".into());
        let ops = local_ops(shadow, current, &mut ids, &mut eng);
        (ops, ids)
    }

    fn dump(dom: &WeakDom, r: Ref) -> String {
        let i = dom.get(r).unwrap();
        let props: std::collections::BTreeMap<_, _> = i
            .properties()
            .iter()
            .filter(|(k, _)| k.as_str() != "UniqueId")
            .collect();
        let mut kids: Vec<String> = i.children().iter().map(|&c| dump(dom, c)).collect();
        kids.sort();
        format!("{}:{}{:?}[{}]", i.name(), i.class(), props, kids.join(","))
    }

    fn dump_all(dom: &WeakDom) -> Vec<String> {
        let mut v: Vec<String> = dom.root_refs().iter().map(|&r| dump(dom, r)).collect();
        v.sort();
        v
    }

    #[test]
    fn no_change_no_ops() {
        let f = fixture();
        let mut cur = f.dom.clone();
        assert!(diff(&f.dom, &mut cur).0.is_empty());
    }

    #[test]
    fn rename_and_reparent() {
        let f = fixture();
        let mut cur = f.dom.clone();
        cur.set_name(f.part, "Q").unwrap();
        let ops = diff(&f.dom, &mut cur).0;
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::Set { n, .. } if n == "Name"));

        let mut cur = f.dom.clone();
        cur.set_parent(f.part, Some(f.script));
        let ops = diff(&f.dom, &mut cur).0;
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Set { n, v, at, .. } => {
                assert_eq!(n, "Parent");
                assert_eq!(v["v"]["p"], "Workspace.S");
                assert_eq!(at.p, "Workspace.S.P");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn property_set_and_remove() {
        let f = fixture();
        let mut cur = f.dom.clone();
        cur.set_property(f.part, "Anchored", Variant::Bool(false))
            .unwrap();
        cur.remove_property(f.part, "Size").unwrap();
        let ops = diff(&f.dom, &mut cur).0;
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], Op::Set { n, v, .. } if n == "Anchored" && v["v"] == false));
        assert!(matches!(&ops[1], Op::Set { n, v, .. } if n == "Size" && v.is_null()));
    }

    #[test]
    fn add_subtree_parent_first() {
        let f = fixture();
        let mut cur = f.dom.clone();
        let m = cur.new_instance("Model", "M", Some(f.ws));
        let c = cur.new_instance("Part", "C", Some(m));
        cur.set_property(c, "Anchored", Variant::Bool(true))
            .unwrap();
        let ops = diff(&f.dom, &mut cur).0;
        assert_eq!(ops.len(), 2);
        match (&ops[0], &ops[1]) {
            (
                Op::Add {
                    cls, props, parent, ..
                },
                Op::Add {
                    props: cp,
                    parent: cparent,
                    ..
                },
            ) => {
                assert_eq!(cls, "Model");
                assert_eq!(props["Name"]["v"], "M");
                assert_eq!(parent.as_ref().unwrap().p, "Workspace");
                assert_eq!(cp["Name"]["v"], "C");
                assert_eq!(cp["Anchored"]["v"], true);
                assert_eq!(cparent.as_ref().unwrap().p, "Workspace.M");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn remove_subtree_one_del() {
        let mut f = fixture();
        let m = f.dom.new_instance("Model", "M", Some(f.ws));
        f.dom.new_instance("Part", "C", Some(m));
        let mut cur = f.dom.clone();
        cur.remove(m);
        let ops = diff(&f.dom, &mut cur).0;
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::Del { at, .. } if at.p == "Workspace.M"));
    }

    #[test]
    fn class_change_del_then_add() {
        let f = fixture();
        let mut cur = f.dom.clone();
        let _ = cur.set_class(f.part, "Model");
        let ops = diff(&f.dom, &mut cur).0;
        assert!(matches!(&ops[0], Op::Del { .. }));
        assert!(matches!(&ops[1], Op::Add { cls, .. } if cls == "Model"));
    }

    #[test]
    fn script_edit_is_src_against_old_text() {
        let f = fixture();
        let mut cur = f.dom.clone();
        cur.set_property(f.script, "Source", Variant::String("b\n".into()))
            .unwrap();
        let ops = diff(&f.dom, &mut cur).0;
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Src { base, text, .. } => {
                assert_eq!(*base, crate::coedit::merge::text_hash("a\n"));
                assert_eq!(text, "b\n");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    fn converge(shadow: &WeakDom, current: &mut WeakDom) {
        let (ops, ids) = diff(shadow, current);
        let mut target = shadow.clone();
        let mut ids = ids.clone();
        let mut eng = Engine::new("2:rbx-native".into());
        let batch = Batch {
            a: "1:studio".into(),
            ops,
        };
        eng.apply(&mut target, &mut ids, &batch);
        assert_eq!(dump_all(&target), dump_all(current));
    }

    #[test]
    fn round_trip_converges() {
        let f = fixture();
        let mut cur = f.dom.clone();
        cur.set_name(f.part, "Q").unwrap();
        cur.set_property(f.part, "Anchored", Variant::Bool(false))
            .unwrap();
        cur.remove_property(f.part, "Size").unwrap();
        cur.set_property(f.script, "Source", Variant::String("b\n".into()))
            .unwrap();
        let m = cur.new_instance("Model", "M", Some(f.ws));
        let c = cur.new_instance("Part", "C", Some(m));
        cur.set_property(c, "Anchored", Variant::Bool(true))
            .unwrap();
        cur.set_parent(f.part, Some(m));
        converge(&f.dom, &mut cur);
    }

    #[test]
    fn undo_reverts() {
        let f = fixture();
        let mut modified = f.dom.clone();
        modified.set_name(f.part, "Q").unwrap();
        modified
            .set_property(f.part, "Anchored", Variant::Bool(false))
            .unwrap();
        modified.new_instance("Part", "New", Some(f.ws));
        let mut older = f.dom.clone();
        converge(&modified, &mut older);
    }
}
